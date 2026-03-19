use anyhow::{Context, Result};
use std::fs;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

use crate::cli::cleanup;
use crate::cli::health;
use crate::config::Config;
use crate::db::Db;
use crate::manager;
use crate::worker;

pub fn execute(workers: usize, backend: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let project_dir = acs_dir
        .parent()
        .context("Expected `.acs/` to be inside a project directory")?
        .to_path_buf();

    let config = Config::load(&acs_dir.join("config.toml"))?;
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

        // Register workers
        {
            let db = db.lock().unwrap();
            for i in 0..workers {
                let worker_id = format!("w-{}", i);
                db.register_agent(&worker_id, "worker", "general")?;
            }
        }

        // Spawn manager task
        let mgr_db = db.clone();
        let mgr_config = config.clone();
        let mgr_shutdown = shutdown_rx.clone();
        let mgr_dir = project_dir.clone();
        let mgr_handle = tokio::spawn(async move {
            manager::run_loop(mgr_db, &mgr_config, mgr_dir, mgr_shutdown).await
        });

        // Spawn worker tasks — each gets a per-worker provider resolved from --backend
        let mut worker_handles = vec![];
        for i in 0..workers {
            let worker_id = format!("w-{}", i);
            let w_db = db.clone();
            let w_config = config.clone();
            let w_dir = project_dir.clone();
            let w_shutdown = shutdown_rx.clone();
            let forced_provider = resolve_worker_provider(i, workers, backend.as_deref());
            let handle = tokio::spawn(async move {
                worker::worker_loop(worker_id, w_db, w_config, w_dir, w_shutdown, forced_provider).await
            });
            worker_handles.push(handle);
        }

        if let Some(ref b) = backend {
            println!("ACS running with {} workers (backend: {}). Press Ctrl+C to stop.", workers, b);
        } else {
            println!("ACS running with {} workers. Press Ctrl+C to stop.", workers);
        }

        // Wait for Ctrl+C
        tokio::signal::ctrl_c().await.ok();
        println!("\nShutting down...");
        shutdown_tx.send(true).ok();

        // Wait for all tasks
        mgr_handle.await.ok();
        for h in worker_handles {
            h.await.ok();
        }

        // Cleanup
        {
            let db = db.lock().unwrap();
            for i in 0..workers {
                db.deregister_agent(&format!("w-{}", i)).ok();
            }
        }

        // Run cleanup to remove stale branches and orphaned worktrees
        println!("Running cleanup...");
        {
            let db = db.lock().unwrap();
            match cleanup::run_cleanup(&project_dir, &*db) {
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
pub fn resolve_worker_provider(index: usize, total: usize, backend: Option<&str>) -> Option<String> {
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
        assert_eq!(resolve_worker_provider(0, 4, Some("mixed")), Some("claude".to_string()));
        assert_eq!(resolve_worker_provider(1, 4, Some("mixed")), Some("claude".to_string()));
        assert_eq!(resolve_worker_provider(2, 4, Some("mixed")), Some("agent".to_string()));
        assert_eq!(resolve_worker_provider(3, 4, Some("mixed")), Some("agent".to_string()));
    }

    #[test]
    fn mixed_backend_with_odd_total() {
        // total=5: workers 0,1 → claude (floor(5/2)=2); workers 2,3,4 → agent
        assert_eq!(resolve_worker_provider(0, 5, Some("mixed")), Some("claude".to_string()));
        assert_eq!(resolve_worker_provider(1, 5, Some("mixed")), Some("claude".to_string()));
        assert_eq!(resolve_worker_provider(2, 5, Some("mixed")), Some("agent".to_string()));
        assert_eq!(resolve_worker_provider(3, 5, Some("mixed")), Some("agent".to_string()));
        assert_eq!(resolve_worker_provider(4, 5, Some("mixed")), Some("agent".to_string()));
    }

    #[test]
    fn mixed_backend_single_worker_uses_claude() {
        assert_eq!(resolve_worker_provider(0, 1, Some("mixed")), Some("claude".to_string()));
    }
}
