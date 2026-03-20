use anyhow::{Context, Result};
use std::fs;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};

use crate::cli::cleanup;
use crate::cli::health;
use crate::config::Config;
use crate::db::Db;
use crate::manager;
use crate::worker;

/// Resolve the active profile name from the CLI flag or `ACS_PROFILE` env var.
/// Returns `None` if neither is set.
pub fn resolve_profile_name(cli_profile: Option<&str>) -> Option<String> {
    cli_profile
        .map(|s| s.to_string())
        .or_else(|| std::env::var("ACS_PROFILE").ok().filter(|s| !s.is_empty()))
}

pub fn execute(
    workers: Option<usize>,
    backend: Option<String>,
    autoscale: bool,
    min_workers: usize,
    profile: Option<String>,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let project_dir = acs_dir
        .parent()
        .context("Expected `.acs/` to be inside a project directory")?
        .to_path_buf();

    let mut config = Config::load(&acs_dir.join("config.toml"))?;

    // Apply named profile: --profile flag > ACS_PROFILE env > default "dev" (if defined).
    if let Some(name) = resolve_profile_name(profile.as_deref()) {
        config.apply_profile(&name)?;
    } else if config.profile.contains_key("dev") {
        config.apply_profile("dev").ok();
    }

    // ANTHROPIC_MODEL env var overrides all claude model tiers.
    config.apply_anthropic_model_env();

    // --workers overrides the profile/config default.
    let workers = workers.unwrap_or(config.project.default_workers);
    let db = Arc::new(Mutex::new(Db::open(&acs_dir.join("project.db"))?));
    write_run_pid(&acs_dir)?;

    // Startup diagnostics: if any health check reports `warn` or `error`,
    // print a one-line warning but still proceed to start manager/workers.
    let report = health::run_health_checks(&project_dir, &acs_dir, &config);
    if report.overall != "ok" {
        eprintln!("Health warning: {}", report.short_summary());
    }

    let rt = tokio::runtime::Runtime::new()?;
    let run_result = rt.block_on(async {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Spawn manager task
        let mgr_db = db.clone();
        let mgr_config = config.clone();
        let mgr_shutdown = shutdown_rx.clone();
        let mgr_dir = project_dir.clone();
        let mgr_handle = tokio::spawn(async move {
            manager::run_loop(mgr_db, &mgr_config, mgr_dir, mgr_shutdown).await
        });

        // Track workers so autoscaling can start/stop them at runtime.
        let mut worker_shutdown_txs: Vec<Option<watch::Sender<bool>>> = vec![None; workers];
        let mut worker_handles: Vec<Option<JoinHandle<()>>> = (0..workers).map(|_| None).collect();
        let mut active_workers = vec![false; workers];
        let min_active = if autoscale {
            min_workers.min(workers)
        } else {
            workers
        };

        for i in 0..min_active {
            let worker_id = format!("w-{}", i);
            {
                let db = db.lock().unwrap();
                db.register_agent(&worker_id, "worker", "general")?;
            }
            let (worker_shutdown_tx, worker_shutdown_rx) = watch::channel(false);
            let w_db = db.clone();
            let w_config = config.clone();
            let w_dir = project_dir.clone();
            let forced_provider = resolve_worker_provider(i, workers, backend.as_deref());
            let handle = tokio::spawn(async move {
                worker::worker_loop(
                    worker_id,
                    w_db,
                    w_config,
                    w_dir,
                    worker_shutdown_rx,
                    forced_provider,
                )
                .await
            });
            worker_shutdown_txs[i] = Some(worker_shutdown_tx);
            worker_handles[i] = Some(handle);
            active_workers[i] = true;
        }

        if autoscale {
            if let Some(ref b) = backend {
                println!(
                    "ACS running with autoscaling (min {}, max {}, backend: {}). Press Ctrl+C to stop.",
                    min_active, workers, b
                );
            } else {
                println!(
                    "ACS running with autoscaling (min {}, max {}). Press Ctrl+C to stop.",
                    min_active, workers
                );
            }
        } else if let Some(ref b) = backend {
            println!(
                "ACS running with {} workers (backend: {}). Press Ctrl+C to stop.",
                workers, b
            );
        } else {
            println!("ACS running with {} workers. Press Ctrl+C to stop.", workers);
        }

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
                _ = sleep(Duration::from_secs(2)), if autoscale => {
                    let queue_depth = {
                        let db = db.lock().unwrap();
                        let counts = db.count_by_status()?;
                        counts
                            .into_iter()
                            .filter_map(|(status, count)| {
                                if status == "pending"
                                    || status == "in_progress"
                                    || status == "review_pending"
                                {
                                    usize::try_from(count).ok()
                                } else {
                                    None
                                }
                            })
                            .sum::<usize>()
                    };
                    let desired = desired_workers_from_queue(queue_depth, min_active, workers);
                    let current = active_workers.iter().filter(|is_active| **is_active).count();

                    if desired > current {
                        let mut need = desired - current;
                        for i in 0..workers {
                            if need == 0 {
                                break;
                            }
                            if active_workers[i] {
                                continue;
                            }
                            let worker_id = format!("w-{}", i);
                            {
                                let db = db.lock().unwrap();
                                db.register_agent(&worker_id, "worker", "general")?;
                            }
                            let (worker_shutdown_tx, worker_shutdown_rx) = watch::channel(false);
                            let w_db = db.clone();
                            let w_config = config.clone();
                            let w_dir = project_dir.clone();
                            let forced_provider = resolve_worker_provider(i, workers, backend.as_deref());
                            let handle = tokio::spawn(async move {
                                worker::worker_loop(
                                    worker_id,
                                    w_db,
                                    w_config,
                                    w_dir,
                                    worker_shutdown_rx,
                                    forced_provider,
                                )
                                .await
                            });
                            worker_shutdown_txs[i] = Some(worker_shutdown_tx);
                            worker_handles[i] = Some(handle);
                            active_workers[i] = true;
                            need -= 1;
                        }
                    } else if desired < current {
                        let mut removable = current - desired;
                        let idle_worker_ids = {
                            let db = db.lock().unwrap();
                            db.list_agents()?
                                .into_iter()
                                .filter(|a| a.role == "worker" && a.status == "idle")
                                .map(|a| a.id)
                                .collect::<Vec<_>>()
                        };
                        for i in (0..workers).rev() {
                            if removable == 0 {
                                break;
                            }
                            if !active_workers[i] {
                                continue;
                            }
                            let worker_id = format!("w-{}", i);
                            if !idle_worker_ids.contains(&worker_id) {
                                continue;
                            }

                            if let Some(tx) = worker_shutdown_txs[i].take() {
                                tx.send(true).ok();
                            }
                            if let Some(handle) = worker_handles[i].take() {
                                handle.await.ok();
                            }
                            {
                                let db = db.lock().unwrap();
                                db.deregister_agent(&worker_id).ok();
                            }
                            active_workers[i] = false;
                            removable -= 1;
                        }
                    }
                }
            }
        }

        println!("\nShutting down...");
        shutdown_tx.send(true).ok();

        // Wait for all tasks
        mgr_handle.await.ok();

        for (i, is_active) in active_workers.iter().copied().enumerate().take(workers) {
            if !is_active {
                continue;
            }
            if let Some(tx) = worker_shutdown_txs[i].take() {
                tx.send(true).ok();
            }
            if let Some(handle) = worker_handles[i].take() {
                handle.await.ok();
            }
        }

        // Cleanup
        {
            let db = db.lock().unwrap();
            for (i, is_active) in active_workers.iter().copied().enumerate().take(workers) {
                if is_active {
                    db.deregister_agent(&format!("w-{}", i)).ok();
                }
            }
        }

        // Run cleanup to remove stale branches and orphaned worktrees
        println!("Running cleanup...");
        {
            let db = db.lock().unwrap();
            match cleanup::run_cleanup(&project_dir, &db) {
                Ok(report) => {
                    if !report.branches_deleted.is_empty() {
                        println!(
                            "Cleaned up {} stale branch(es): {}",
                            report.branches_deleted.len(),
                            report.branches_deleted.join(", ")
                        );
                    }
                    if !report.worktrees_removed.is_empty() {
                        println!(
                            "Removed {} orphaned worktree(s): {}",
                            report.worktrees_removed.len(),
                            report.worktrees_removed.join(", ")
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Warning: cleanup failed: {:#}", e);
                }
            }
        }

        println!("Stopped.");
        Ok::<(), anyhow::Error>(())
    });

    let _ = remove_run_pid_if_owned(&acs_dir);
    run_result?;

    Ok(())
}

fn desired_workers_from_queue(queue_depth: usize, min_workers: usize, max_workers: usize) -> usize {
    queue_depth.clamp(min_workers, max_workers)
}

fn write_run_pid(acs_dir: &std::path::Path) -> Result<()> {
    let pid_path = acs_dir.join("run.pid");
    fs::write(&pid_path, format!("{}\n", std::process::id()))
        .with_context(|| format!("Failed to write {}", pid_path.display()))?;
    Ok(())
}

fn remove_run_pid_if_owned(acs_dir: &std::path::Path) -> Result<()> {
    let pid_path = acs_dir.join("run.pid");
    let current = std::process::id().to_string();
    if let Ok(existing) = fs::read_to_string(&pid_path) {
        if existing.trim() == current {
            let _ = fs::remove_file(&pid_path);
        }
    }
    Ok(())
}

/// Resolves the provider for a specific worker index given the --backend flag.
///
/// - `None` → no forced provider; worker uses config-based selection
/// - `"claude"` → all workers use claude
/// - `"cursor"` → all workers use the cursor agent (mapped to "agent" internally)
/// - `"codex"` → all workers use codex
/// - `"mixed"` → first half of workers use claude, second half use cursor/agent
/// - anything else → treated as a literal provider name
pub fn resolve_worker_provider(
    index: usize,
    total: usize,
    backend: Option<&str>,
) -> Option<String> {
    match backend {
        None => None,
        Some("mixed") => {
            // First half (floor division) → claude; second half → cursor agent
            let split = total / 2;
            if total <= 1 || index < split {
                Some("claude".to_string())
            } else {
                Some("agent".to_string())
            }
        }
        Some("cursor") => Some("agent".to_string()),
        Some(other) => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_backend_returns_none() {
        assert_eq!(resolve_worker_provider(0, 4, None), None);
        assert_eq!(resolve_worker_provider(3, 4, None), None);
    }

    #[test]
    fn cursor_backend_maps_to_agent_for_all_workers() {
        for i in 0..4 {
            assert_eq!(
                resolve_worker_provider(i, 4, Some("cursor")),
                Some("agent".to_string())
            );
        }
    }

    #[test]
    fn claude_backend_maps_to_claude_for_all_workers() {
        for i in 0..4 {
            assert_eq!(
                resolve_worker_provider(i, 4, Some("claude")),
                Some("claude".to_string())
            );
        }
    }

    #[test]
    fn codex_backend_maps_to_codex_for_all_workers() {
        for i in 0..4 {
            assert_eq!(
                resolve_worker_provider(i, 4, Some("codex")),
                Some("codex".to_string())
            );
        }
    }

    #[test]
    fn mixed_backend_splits_at_midpoint() {
        // total=4: workers 0,1 → claude; workers 2,3 → agent
        assert_eq!(
            resolve_worker_provider(0, 4, Some("mixed")),
            Some("claude".to_string())
        );
        assert_eq!(
            resolve_worker_provider(1, 4, Some("mixed")),
            Some("claude".to_string())
        );
        assert_eq!(
            resolve_worker_provider(2, 4, Some("mixed")),
            Some("agent".to_string())
        );
        assert_eq!(
            resolve_worker_provider(3, 4, Some("mixed")),
            Some("agent".to_string())
        );
    }

    #[test]
    fn mixed_backend_with_odd_total() {
        // total=5: workers 0,1 → claude (floor(5/2)=2); workers 2,3,4 → agent
        assert_eq!(
            resolve_worker_provider(0, 5, Some("mixed")),
            Some("claude".to_string())
        );
        assert_eq!(
            resolve_worker_provider(1, 5, Some("mixed")),
            Some("claude".to_string())
        );
        assert_eq!(
            resolve_worker_provider(2, 5, Some("mixed")),
            Some("agent".to_string())
        );
        assert_eq!(
            resolve_worker_provider(3, 5, Some("mixed")),
            Some("agent".to_string())
        );
        assert_eq!(
            resolve_worker_provider(4, 5, Some("mixed")),
            Some("agent".to_string())
        );
    }

    #[test]
    fn mixed_backend_single_worker_uses_claude() {
        assert_eq!(
            resolve_worker_provider(0, 1, Some("mixed")),
            Some("claude".to_string())
        );
    }

    #[test]
    fn desired_workers_respects_minimum() {
        assert_eq!(desired_workers_from_queue(0, 1, 8), 1);
    }

    #[test]
    fn desired_workers_respects_maximum() {
        assert_eq!(desired_workers_from_queue(99, 1, 8), 8);
    }

    #[test]
    fn desired_workers_matches_queue_in_range() {
        assert_eq!(desired_workers_from_queue(4, 1, 8), 4);
    }

    // ── Profile resolution tests ────────────────────────────────────

    #[test]
    fn resolve_profile_name_uses_cli_flag_over_env() {
        std::env::set_var("ACS_PROFILE", "ci");
        let result = resolve_profile_name(Some("prod"));
        std::env::remove_var("ACS_PROFILE");
        assert_eq!(result, Some("prod".to_string()));
    }

    #[test]
    fn resolve_profile_name_falls_back_to_env_when_no_flag() {
        std::env::set_var("ACS_PROFILE", "ci");
        let result = resolve_profile_name(None);
        std::env::remove_var("ACS_PROFILE");
        assert_eq!(result, Some("ci".to_string()));
    }

    #[test]
    fn resolve_profile_name_returns_none_when_neither_set() {
        std::env::remove_var("ACS_PROFILE");
        assert_eq!(resolve_profile_name(None), None);
    }

    #[test]
    fn resolve_profile_name_ignores_empty_env() {
        std::env::set_var("ACS_PROFILE", "");
        let result = resolve_profile_name(None);
        std::env::remove_var("ACS_PROFILE");
        assert_eq!(result, None);
    }
}
