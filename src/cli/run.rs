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
    execute_with_dir(&cwd, workers.unwrap_or(0), backend, autoscale, min_workers, profile)
}

fn execute_with_dir(
    cwd: &std::path::Path,
    workers: usize,
    backend: Option<String>,
    autoscale: bool,
    min_workers: usize,
    profile: Option<String>,
) -> Result<()> {
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(cwd)?;
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
    let workers = if workers == 0 { config.project.default_workers } else { workers };
    let db = Arc::new(Mutex::new(Db::open(&acs_dir.join("project.db"))?));
    write_run_pid(&acs_dir)?;

    // Startup diagnostics: if any health check reports `warn` or `error`,
    // print a one-line warning but still proceed to start manager/workers.
    let report = health::run_health_checks(&project_dir, &acs_dir, &config);
    if report.overall != "ok" {
        eprintln!("Health warning: {}", report.short_summary());
    }

    // Test hook: skip the long-running manager/worker loop (see unit tests).
    let skip_loop = std::env::var("ACS_SKIP_RUN_LOOP").is_ok();

    let rt = tokio::runtime::Runtime::new()?;
    let run_result = rt.block_on(async {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Spawn manager task
        let mgr_db = db.clone();
        let mgr_config = config.clone();
        let mgr_shutdown = shutdown_rx.clone();
        let mgr_dir = project_dir.clone();
        let skip_loop_mgr = skip_loop;
        let mgr_handle = tokio::spawn(async move {
            if !skip_loop_mgr {
                let _ = manager::run_loop(mgr_db, &mgr_config, mgr_dir, mgr_shutdown, false).await;
            }
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

        // Build per-worker backend list (one entry per worker slot).
        let worker_backends = build_worker_backends(backend.as_deref(), workers);

        for i in 0..min_active {
            let worker_id = format!("w-{}", i);
            let backend_name = worker_backends.get(i).map(|s| s.as_str()).unwrap_or("claude");
            {
                let db = db.lock().unwrap();
                db.register_agent_with_backend(&worker_id, "worker", "general", backend_name)?;
            }
            let (worker_shutdown_tx, worker_shutdown_rx) = watch::channel(false);
            let w_db = db.clone();
            let w_config = config.clone();
            let w_dir = project_dir.clone();
            let forced_provider = Some(provider_for_backend(backend_name));
            let handle = tokio::spawn(async move {
                if !skip_loop {
                    worker::worker_loop(
                        worker_id,
                        w_db,
                        w_config,
                        w_dir,
                        worker_shutdown_rx,
                        forced_provider,
                    )
                    .await
                }
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

        // Production event loop: wait for Ctrl+C, autoscale workers every 2 s.
        // Excluded from test builds to avoid blocking on a signal that never fires.
        #[cfg(not(test))]
        loop {
            if skip_loop && !autoscale {
                break;
            }
            tokio::select! {
                _ = tokio::signal::ctrl_c(), if !skip_loop => {
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
                            let backend_name = worker_backends.get(i).map(|s| s.as_str()).unwrap_or("claude");
                            {
                                let db = db.lock().unwrap();
                                db.register_agent_with_backend(&worker_id, "worker", "general", backend_name)?;
                            }
                            let (worker_shutdown_tx, worker_shutdown_rx) = watch::channel(false);
                            let w_db = db.clone();
                            let w_config = config.clone();
                            let w_dir = project_dir.clone();
                            let forced_provider = Some(provider_for_backend(backend_name));
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

/// Adjust the number of running workers to match the current queue depth.
///
/// Called every 2 seconds from the autoscale loop. Starts workers when more
/// are needed and shuts down idle workers when fewer are needed.
async fn do_autoscale_adjust(
    db: &Arc<Mutex<Db>>,
    config: &Config,
    project_dir: &std::path::PathBuf,
    worker_shutdown_txs: &mut Vec<Option<watch::Sender<bool>>>,
    worker_handles: &mut Vec<Option<JoinHandle<()>>>,
    active_workers: &mut Vec<bool>,
    min_active: usize,
    max_workers: usize,
    backend: Option<&str>,
) -> Result<()> {
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
    let desired = desired_workers_from_queue(queue_depth, min_active, max_workers);
    let current = active_workers.iter().filter(|is_active| **is_active).count();

    if desired > current {
        let mut need = desired - current;
        for i in 0..max_workers {
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
            let forced_provider = resolve_worker_provider(i, max_workers, backend);
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
        for i in (0..max_workers).rev() {
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

/// Parse an allocation string like `"claude:2,cursor:2"` into a list of
/// `(backend_name, count)` pairs.  Returns an error if the string is not in
/// the `name:N` format or any count is zero.
pub fn parse_backend_allocation(s: &str) -> Result<Vec<(String, usize)>> {
    let mut result = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        let colon = part.rfind(':')
            .ok_or_else(|| anyhow::anyhow!("expected 'backend:N' format, got '{}'", part))?;
        let name = part[..colon].trim().to_string();
        let count_str = part[colon + 1..].trim();
        let count: usize = count_str.parse()
            .map_err(|_| anyhow::anyhow!("invalid count '{}' for backend '{}'", count_str, name))?;
        if count == 0 {
            return Err(anyhow::anyhow!("count for backend '{}' must be > 0", name));
        }
        result.push((name, count));
    }
    Ok(result)
}

/// Build a flat list of backend names, one per worker slot.
///
/// Handles:
/// - `None`            → all slots default to `"claude"`
/// - `"claude:2,cursor:2"` (allocation format) → expanded list
/// - `"claude"` / `"codex"` / etc. → all slots use that backend
/// - `"mixed"`         → first half `"claude"`, second half `"cursor"`
///
/// If the allocation sum differs from `total`, the list is truncated or padded
/// with the last backend so the result always has exactly `total` entries.
pub fn build_worker_backends(backend: Option<&str>, total: usize) -> Vec<String> {
    if total == 0 {
        return vec![];
    }
    let s = match backend {
        None => return vec!["claude".to_string(); total],
        Some(s) => s,
    };

    // Detect allocation format: must contain both ':' and ','  OR a single 'name:N'
    if s.contains(':') {
        if let Ok(allocs) = parse_backend_allocation(s) {
            let mut flat: Vec<String> = allocs
                .into_iter()
                .flat_map(|(name, count)| std::iter::repeat(name).take(count))
                .collect();
            // Truncate or pad to exactly `total`
            flat.truncate(total);
            while flat.len() < total {
                let last = flat.last().cloned().unwrap_or_else(|| "claude".to_string());
                flat.push(last);
            }
            return flat;
        }
    }

    // Simple / legacy names
    match s {
        "mixed" => {
            let split = total / 2;
            let mut v: Vec<String> = std::iter::repeat("claude".to_string()).take(split).collect();
            v.extend(std::iter::repeat("cursor".to_string()).take(total - split));
            v
        }
        other => vec![other.to_string(); total],
    }
}

/// Map the user-facing backend name to the internal spawner provider name.
/// Currently only `"cursor"` needs remapping (→ `"agent"`).
pub fn provider_for_backend(backend: &str) -> String {
    match backend {
        "cursor" => "agent".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    /// Create a minimal `.acs/` project directory for tests.
    fn make_test_project(dir: &std::path::Path) {
        let acs_dir = dir.join(".acs");
        std::fs::create_dir_all(&acs_dir).unwrap();
        std::fs::write(
            acs_dir.join("config.toml"),
            "[project]\nname = \"test\"\n",
        )
        .unwrap();
        Db::open(&acs_dir.join("project.db")).unwrap();
    }

    // -----------------------------------------------------------------------
    // write_run_pid / remove_run_pid_if_owned
    // -----------------------------------------------------------------------

    #[test]
    fn pid_file_contains_current_process_id() {
        let tmp = TempDir::new().unwrap();
        write_run_pid(tmp.path()).unwrap();
        let content = std::fs::read_to_string(tmp.path().join("run.pid")).unwrap();
        assert_eq!(
            content.trim().parse::<u32>().unwrap(),
            std::process::id()
        );
    }

    #[test]
    fn remove_pid_deletes_file_when_pid_matches() {
        let tmp = TempDir::new().unwrap();
        write_run_pid(tmp.path()).unwrap();
        assert!(tmp.path().join("run.pid").exists());
        remove_run_pid_if_owned(tmp.path()).unwrap();
        assert!(!tmp.path().join("run.pid").exists());
    }

    #[test]
    fn remove_pid_keeps_file_when_pid_differs() {
        let tmp = TempDir::new().unwrap();
        // Write PID 0 — can never match the current process id.
        std::fs::write(tmp.path().join("run.pid"), "0\n").unwrap();
        remove_run_pid_if_owned(tmp.path()).unwrap();
        assert!(tmp.path().join("run.pid").exists());
    }

    // -----------------------------------------------------------------------
    // execute_with_dir — exercises setup + teardown without blocking on Ctrl+C
    // (the production loop is excluded from test builds via #[cfg(not(test))])
    // -----------------------------------------------------------------------

    #[test]
    fn execute_no_autoscale_completes_successfully() {
        let tmp = TempDir::new().unwrap();
        make_test_project(tmp.path());
        execute_with_dir(tmp.path(), 1, None, false, 1).unwrap();
        // PID file should be cleaned up on exit.
        assert!(!tmp.path().join(".acs").join("run.pid").exists());
    }

    #[test]
    fn execute_autoscale_without_backend_completes_successfully() {
        let tmp = TempDir::new().unwrap();
        make_test_project(tmp.path());
        execute_with_dir(tmp.path(), 2, None, true, 1).unwrap();
    }

    #[test]
    fn execute_with_backend_no_autoscale_completes_successfully() {
        let tmp = TempDir::new().unwrap();
        make_test_project(tmp.path());
        execute_with_dir(tmp.path(), 1, Some("claude".to_string()), false, 1).unwrap();
    }

    #[test]
    fn execute_autoscale_with_backend_completes_successfully() {
        let tmp = TempDir::new().unwrap();
        make_test_project(tmp.path());
        execute_with_dir(tmp.path(), 2, Some("mixed".to_string()), true, 1).unwrap();
    }

    // -----------------------------------------------------------------------
    // do_autoscale_adjust
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn autoscale_adjust_starts_workers_when_queue_nonempty() {
        let db = Arc::new(Mutex::new(Db::open_memory().unwrap()));
        {
            let g = db.lock().unwrap();
            g.create_ticket("Test ticket", "desc", "core", 1).unwrap();
        }
        let config = Config::default_for("test");
        let project_dir = std::env::temp_dir();
        let mut txs: Vec<Option<watch::Sender<bool>>> = vec![None; 2];
        let mut handles: Vec<Option<JoinHandle<()>>> =
            (0..2).map(|_| None).collect();
        let mut active = vec![false, false];

        do_autoscale_adjust(
            &db,
            &config,
            &project_dir,
            &mut txs,
            &mut handles,
            &mut active,
            1,
            2,
            None,
        )
        .await
        .unwrap();

        // queue_depth=1, desired=1, current=0 → start w-0
        assert_eq!(active.iter().filter(|&&a| a).count(), 1);
        assert!(active[0]);
        // Clean up: signal the started worker to shut down.
        if let Some(tx) = txs[0].take() {
            tx.send(true).ok();
        }
    }

    #[tokio::test]
    async fn autoscale_adjust_stops_idle_worker_when_queue_empty() {
        let db = Arc::new(Mutex::new(Db::open_memory().unwrap()));
        {
            let g = db.lock().unwrap();
            g.register_agent("w-0", "worker", "general").unwrap();
            g.register_agent("w-1", "worker", "general").unwrap();
        }
        let config = Config::default_for("test");
        let project_dir = std::env::temp_dir();
        let (tx0, _rx0) = watch::channel::<bool>(false);
        let (tx1, _rx1) = watch::channel::<bool>(false);
        let h0: JoinHandle<()> = tokio::spawn(async {});
        let h1: JoinHandle<()> = tokio::spawn(async {});
        let mut txs: Vec<Option<watch::Sender<bool>>> = vec![Some(tx0), Some(tx1)];
        let mut handles: Vec<Option<JoinHandle<()>>> = vec![Some(h0), Some(h1)];
        let mut active = vec![true, true];

        // queue=0, min=1, current=2 → stop one idle worker (highest index first → w-1)
        do_autoscale_adjust(
            &db,
            &config,
            &project_dir,
            &mut txs,
            &mut handles,
            &mut active,
            1,
            2,
            None,
        )
        .await
        .unwrap();

        assert_eq!(active.iter().filter(|&&a| a).count(), 1);
        assert!(active[0]);
        assert!(!active[1]);
    }

    #[tokio::test]
    async fn autoscale_adjust_no_change_when_desired_equals_current() {
        let db = Arc::new(Mutex::new(Db::open_memory().unwrap()));
        {
            let g = db.lock().unwrap();
            g.create_ticket("Test ticket", "desc", "core", 1).unwrap();
            g.register_agent("w-0", "worker", "general").unwrap();
        }
        let config = Config::default_for("test");
        let project_dir = std::env::temp_dir();
        let (tx0, _rx0) = watch::channel::<bool>(false);
        let h0: JoinHandle<()> = tokio::spawn(async {});
        let mut txs: Vec<Option<watch::Sender<bool>>> = vec![Some(tx0), None];
        let mut handles: Vec<Option<JoinHandle<()>>> = vec![Some(h0), None];
        let mut active = vec![true, false];

        // queue=1, desired=1, current=1 → no change
        do_autoscale_adjust(
            &db,
            &config,
            &project_dir,
            &mut txs,
            &mut handles,
            &mut active,
            1,
            2,
            None,
        )
        .await
        .unwrap();

        assert!(active[0]);
        assert!(!active[1]);
    }

    // -----------------------------------------------------------------------
    // resolve_worker_provider (pre-existing tests kept intact)
    // -----------------------------------------------------------------------

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
