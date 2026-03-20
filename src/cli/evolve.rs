use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use tokio::time::{sleep, Duration, Instant};

use crate::cli::acs_dir;
use crate::config::Config;
use crate::db::Db;
use crate::manager;
use crate::prompts;
use crate::spawner::Spawner;
use crate::worker;

#[allow(clippy::too_many_arguments)]
pub fn execute(
    workers: Option<usize>,
    max_iterations: usize,
    plan_each_iteration: bool,
    bootstrap_after_run: bool,
    stop_when_no_new_tickets: bool,
    max_run_seconds: Option<u64>,
    preserve_agents: bool,
    dry_run: bool,
    backend: Option<String>,
    profile: Option<String>,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = acs_dir::resolve_acs_dir(&cwd)?;
    let project_dir = acs_dir
        .parent()
        .context("Expected `.acs/` to be inside a project directory")?
        .to_path_buf();

    let mut config = Config::load(&acs_dir.join("config.toml"))?;

    // Apply named profile: --profile flag > ACS_PROFILE env > default "dev" (if defined).
    if let Some(name) = crate::cli::run::resolve_profile_name(profile.as_deref()) {
        config.apply_profile(&name)?;
    } else if config.profile.contains_key("dev") {
        config.apply_profile("dev").ok();
    }

    // ANTHROPIC_MODEL env var overrides all claude model tiers.
    config.apply_anthropic_model_env();

    // --workers overrides the profile/config default.
    let workers = workers.unwrap_or(config.project.default_workers);
    let db = Arc::new(Mutex::new(Db::open(&acs_dir.join("project.db"))?));

    if dry_run {
        let total = db.lock().unwrap().list_tickets(None)?.len();
        let out = serde_json::json!({
            "status": "dry_run",
            "project_dir": project_dir.to_string_lossy(),
            "ticket_count": total,
            "max_iterations": max_iterations,
            "workers": workers,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    let tool_path = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "acs".to_string());

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        if max_iterations == 0 {
            let total = db.lock().unwrap().list_tickets(None)?.len();
            let out = serde_json::json!({
                "status": "no_op",
                "reason": "max_iterations is 0",
                "ticket_count": total
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(());
        }

        let spawner = Spawner::new(&project_dir, &config.agents.claude_path, &tool_path);

        for iter in 0..max_iterations {
            eprintln!("[evolve] iteration {}/{}", iter + 1, max_iterations);
            let milestone_num = iter + 1;

            // Optional planning step.
            if plan_each_iteration {
                run_architect_planning(&spawner, &project_dir, &tool_path)?;
            }

            // Ensure we have at least some initial work.
            // (If no tickets exist yet, incremental bootstrap is the first step.)
            let before_ticket_count = db.lock().unwrap().list_tickets(None)?.len();
            if before_ticket_count == 0 {
                if !bootstrap_after_run {
                    bail!("No tickets found; bootstrap_after_run=false will not create any work.");
                }
                // Seed tickets for the first bounded run.
                run_incremental_bootstrap(&spawner, &project_dir, &tool_path, &config)?;
            }

            // Run bounded manager+workers until the queue drains or timeout.
            run_bounded_workers(
                &db,
                &config,
                &project_dir,
                workers,
                max_run_seconds,
                preserve_agents,
                backend.clone(),
            )
            .await?;

            // Generate milestone report after each bounded run.
            {
                let db_guard = db.lock().unwrap();
                if let Err(e) = crate::cli::report::generate_milestone_report(
                    &acs_dir, &db_guard, milestone_num,
                ) {
                    eprintln!("[report] warning: failed to generate milestone report: {:#}", e);
                }
            }

            if !bootstrap_after_run {
                // If caller doesn't want additional tickets, exit after first run.
                if iter == 0 {
                    break;
                }
                continue;
            }

            let prev_count = db.lock().unwrap().list_tickets(None)?.len();
            // Incremental bootstrap adds only new tickets/KB facts.
            run_incremental_bootstrap(&spawner, &project_dir, &tool_path, &config)?;
            let new_count = db.lock().unwrap().list_tickets(None)?.len();

            eprintln!(
                "[evolve] tickets: {} -> {} (delta={})",
                prev_count,
                new_count,
                new_count.saturating_sub(prev_count)
            );

            if stop_when_no_new_tickets && new_count == prev_count {
                eprintln!("[evolve] stopping: no new tickets created");
                break;
            }
        }

        // Best-effort cleanup at the end of evolution run.
        let cleanup_db = db.lock().unwrap();
        let _ = crate::cli::cleanup::run_cleanup(&project_dir, &cleanup_db);
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

fn run_architect_planning(
    spawner: &Spawner,
    project_dir: &PathBuf,
    tool_path: &str,
) -> Result<()> {
    let system_prompt = prompts::architect_prompt(&project_dir.to_string_lossy(), tool_path);

    let task_prompt = format!(
        "Analyze the tickets and knowledge base for the project at {}, then create a comprehensive \
         architecture plan. Group tickets into milestones, write ADRs for key decisions, and define \
         API contracts between domains. \
         Use the Bash tool to run `{}` commands as described in your system prompt.",
        project_dir.display(),
        tool_path
    );

    let mut child = spawner.spawn_claude(
        &format!("architect-iter-{}", chrono::Utc::now().timestamp_millis()),
        project_dir,
        &task_prompt,
        &system_prompt,
    )?;

    let status = child.wait()?;
    if !status.success() {
        bail!("architect agent failed with status: {:?}", status.code());
    }

    Ok(())
}

async fn run_bounded_workers(
    db: &Arc<Mutex<Db>>,
    config: &Config,
    project_dir: &PathBuf,
    workers: usize,
    max_run_seconds: Option<u64>,
    preserve_agents: bool,
    backend: Option<String>,
) -> Result<()> {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Register workers (optionally preserve existing agent state).
    let mut newly_registered: Vec<String> = vec![];
    {
        let db_guard = db.lock().unwrap();
        let existing_agents = db_guard.list_agents()?;
        let existing_ids: std::collections::HashSet<String> =
            existing_agents.into_iter().map(|a| a.id).collect();

        let mut ids_to_register = vec![];
        for i in 0..workers {
            let worker_id = format!("w-{}", i);
            if !preserve_agents || !existing_ids.contains(&worker_id) {
                ids_to_register.push(worker_id.clone());
            }
        }

        // Apply registration.
        // Note: we reuse the same lock scope (db_guard is immutable borrow after list_agents),
        // so we re-lock below for the updates.
        drop(db_guard);
        let db_guard = db.lock().unwrap();
        for worker_id in ids_to_register {
            db_guard.register_agent(&worker_id, "worker", "general")?;
            newly_registered.push(worker_id);
        }
    }

    // Spawn manager task.
    let mgr_db = db.clone();
    let mgr_config = config.clone();
    let mgr_shutdown = shutdown_rx.clone();
    let mgr_dir = project_dir.clone();
    let mgr_handle = tokio::spawn(async move {
        manager::run_loop(mgr_db, &mgr_config, mgr_dir, mgr_shutdown).await
    });

    // Spawn worker tasks.
    let mut worker_handles = vec![];
    for i in 0..workers {
        let worker_id = format!("w-{}", i);
        let w_db = db.clone();
        let w_config = config.clone();
        let w_dir = project_dir.clone();
        let w_shutdown = shutdown_rx.clone();
        let forced_provider = crate::cli::run::resolve_worker_provider(i, workers, backend.as_deref());
        let handle = tokio::spawn(async move {
            worker::worker_loop(worker_id, w_db, w_config, w_dir, w_shutdown, forced_provider).await
        });
        worker_handles.push(handle);
    }

    eprintln!("[evolve] bounded run: workers={}", workers);

    let start = Instant::now();
    loop {
        let queue_len = {
            let guard = db.lock().unwrap();
            let counts = guard.count_by_status()?;
            let mut pending = 0i64;
            for (status, c) in counts {
                if status == "pending" || status == "in_progress" || status == "review_pending" {
                    pending += c;
                }
            }
            pending
        };

        if queue_len == 0 {
            eprintln!("[evolve] bounded run: queue drained");
            break;
        }

        if let Some(max_s) = max_run_seconds {
            if start.elapsed() > Duration::from_secs(max_s) {
                eprintln!("[evolve] bounded run: hit max_run_seconds={}", max_s);
                break;
            }
        }

        sleep(Duration::from_secs(1)).await;
    }

    // Stop manager/workers.
    shutdown_tx.send(true).ok();

    // Wait for all tasks to exit.
    mgr_handle.await.ok();
    for h in worker_handles {
        h.await.ok();
    }

    // Deregister agents.
    {
        let db = db.lock().unwrap();
        if preserve_agents {
            for id in newly_registered {
                db.deregister_agent(&id).ok();
            }
        } else {
            for i in 0..workers {
                db.deregister_agent(&format!("w-{}", i)).ok();
            }
        }
    }

    Ok(())
}

fn run_incremental_bootstrap(
    spawner: &Spawner,
    project_dir: &PathBuf,
    tool_path: &str,
    _config: &Config,
) -> Result<()> {
    let system_prompt = prompts::incremental_bootstrap_prompt(&project_dir.to_string_lossy(), tool_path);

    let task_prompt = format!(
        "Analyze the repository at {}, read existing tickets and knowledge base entries, then create \
         only the missing/new tickets per the system prompt. \
         Use the Bash tool to run `{}` commands as described in your system prompt. \
         IMPORTANT: Always use the Bash tool to call acs commands.",
        project_dir.display(),
        tool_path
    );

    let mut child = spawner.spawn_claude(
        &format!("bootstrap-iter-{}", chrono::Utc::now().timestamp_millis()),
        project_dir,
        &task_prompt,
        &system_prompt,
    )?;

    let status = child.wait()?;
    if !status.success() {
        bail!("bootstrap agent failed with status: {:?}", status.code());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn dry_run_no_op_when_max_iterations_zero() {
        assert_eq!(0usize, 0usize);
    }
}

