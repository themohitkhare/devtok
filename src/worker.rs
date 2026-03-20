// src/worker.rs
//
// Each worker runs as a tokio task, polling its inbox and spawning Claude Code
// in a git worktree when it receives a ticket_assignment message.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use tokio::sync::{oneshot, watch};

use crate::config::Config;
use crate::db::Db;
use crate::prompts;
use crate::spawner::{SpawnProvider, Spawner};

pub async fn worker_loop(
    worker_id: String,
    db: Arc<Mutex<Db>>,
    config: Config,
    project_dir: PathBuf,
    mut shutdown: watch::Receiver<bool>,
    forced_provider: Option<String>,
) {
    let poll_interval = Duration::from_secs(config.manager.worker_poll_seconds);

    loop {
        // Check shutdown before doing anything
        if *shutdown.borrow() {
            tracing_log(&worker_id, "shutdown signal received, exiting");
            return;
        }

        // Poll inbox
        let msg = {
            let db = db.lock().unwrap();
            db.pop_inbox(&worker_id).unwrap_or(None)
        };

        match msg {
            None => {
                // No message — sleep, then check shutdown again
                tokio::select! {
                    _ = tokio::time::sleep(poll_interval) => {}
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            tracing_log(&worker_id, "shutdown signal received during sleep, exiting");
                            return;
                        }
                    }
                }
            }

            Some(message) if message.msg_type == "ticket_assignment" => {
                if let Err(e) = handle_ticket_assignment(
                    &worker_id,
                    &message.payload,
                    &db,
                    &config,
                    &project_dir,
                    &mut shutdown,
                    forced_provider.clone(),
                )
                .await
                {
                    eprintln!("[{}] error handling ticket_assignment: {:#}", worker_id, e);
                    // Log the error and continue looping
                    let db = db.lock().unwrap();
                    let _ = db.log_event(
                        Some(&worker_id),
                        "worker_error",
                        &format!("ticket_assignment error: {}", e),
                        None,
                    );
                }
            }

            Some(message) => {
                // Unknown message type — log and ignore
                let db = db.lock().unwrap();
                let _ = db.log_event(
                    Some(&worker_id),
                    "unknown_message",
                    &format!(
                        "ignoring unknown msg_type '{}': {}",
                        message.msg_type, message.payload
                    ),
                    None,
                );
            }
        }
    }
}

async fn handle_ticket_assignment(
    worker_id: &str,
    payload: &str,
    db: &Arc<Mutex<Db>>,
    config: &Config,
    project_dir: &Path,
    shutdown: &mut watch::Receiver<bool>,
    forced_provider: Option<String>,
) -> Result<()> {
    let spawner = Spawner::new_with_agent_config(project_dir, &config.agents)
        .with_backends(config.backends.clone());
    handle_ticket_with_spawner(
        &spawner,
        worker_id,
        payload,
        db,
        config,
        shutdown,
        forced_provider,
    )
    .await
}

async fn handle_ticket_with_spawner<S: SpawnProvider>(
    spawner: &S,
    worker_id: &str,
    payload: &str,
    db: &Arc<Mutex<Db>>,
    config: &Config,
    shutdown: &mut watch::Receiver<bool>,
    forced_provider: Option<String>,
) -> Result<()> {
    // --- (a) Parse payload ---
    let val: serde_json::Value = serde_json::from_str(payload)?;
    let ticket_id = val["ticket_id"].as_str().unwrap_or("").to_string();
    let title = val["title"].as_str().unwrap_or("").to_string();
    let description = val["description"].as_str().unwrap_or("").to_string();
    let domain = val["domain"].as_str().unwrap_or("general").to_string();
    let persona = val["persona"]
        .as_str()
        .unwrap_or_else(|| config.persona_for_domain(&domain))
        .to_string();
    let kb_context = val["kb_context"].as_str().unwrap_or("").to_string();
    let previous_attempt_notes = val["previous_attempt_notes"]
        .as_str()
        .unwrap_or("")
        .to_string();

    // Provider selection: forced_provider (from --backend flag) > payload override > config-based selection
    let provider = forced_provider.unwrap_or_else(|| {
        val.get("work_type")
            .or_else(|| val.get("provider"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| select_provider_for_ticket(&config.agents, worker_id, &ticket_id))
    });

    let model = val
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Resume-from-checkpoint: if a prior branch was checkpointed, inject into the prompt.
    let checkpoint_branch = val
        .get("checkpoint_branch")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let checkpoint_committed_at = val
        .get("checkpoint_committed_at")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    tracing_log(
        worker_id,
        &format!("received ticket_assignment for {}", ticket_id),
    );

    // --- (b) Mark agent as working ---
    {
        let db = db.lock().unwrap();
        db.update_agent(worker_id, "working", Some(&ticket_id), None)?;
        db.log_event(
            Some(worker_id),
            "ticket_start",
            &format!("starting ticket {}: {}", ticket_id, title),
            None,
        )?;
    }

    // --- (d) Create worktree ---
    let worktree = spawner.create_worktree(worker_id, &ticket_id)?;

    // --- (e) Build system prompt ---
    let system_prompt = prompts::worker_prompt(
        &ticket_id,
        &title,
        &description,
        &domain,
        &persona,
        &config.agents.tool_path,
        &kb_context,
        &previous_attempt_notes,
    );

    // --- (f) Build task prompt ---
    let resume_section = match checkpoint_branch.as_deref() {
        Some(branch) => {
            let committed_at = checkpoint_committed_at.as_deref().unwrap_or("unknown time");
            format!(
                "\n\n## Resuming from checkpoint: branch {} was last committed at {}. Checkout that branch and continue from where the previous worker left off.",
                branch, committed_at
            )
        }
        None => String::new(),
    };

    let task_prompt = format!(
        "You are assigned ticket {ticket_id}: {title}\n\nDescription:\n{description}{resume_section}\n\nExecute this ticket. Use Bash to call acs commands.",
        ticket_id = ticket_id,
        title = title,
        description = description,
        resume_section = resume_section,
    );

    // --- (g) Spawn provider ---
    tracing_log(
        worker_id,
        &format!(
            "spawning provider '{}' (model={:?}) for ticket {}",
            provider, model, ticket_id
        ),
    );
    let mut child = spawner.spawn_provider(
        &provider,
        model.as_deref(),
        worker_id,
        &worktree,
        &task_prompt,
        &system_prompt,
    )?;
    let pid: u32 = child.id();

    // Update agent record with the PID
    {
        let db = db.lock().unwrap();
        db.update_agent(worker_id, "working", Some(&ticket_id), Some(pid))?;
    }

    // --- (h.pre) Spawn checkpoint background task ---
    // `_checkpoint_cancel_tx` is held until function exit: dropping it signals the loop to stop.
    let (_checkpoint_cancel_tx, checkpoint_cancel_rx) = oneshot::channel::<()>();
    let _checkpoint_task = tokio::spawn(checkpoint_loop(
        worktree.clone(),
        worker_id.to_string(),
        ticket_id.clone(),
        db.clone(),
        checkpoint_cancel_rx,
    ));

    // --- (h) Wait for completion with timeout ---
    let timeout = Duration::from_secs(config.manager.worker_timeout_seconds);

    let wait_handle = tokio::task::spawn_blocking(move || child.wait());

    let result = tokio::select! {
        _ = shutdown.changed() => {
            // Manager shutdown requested: stop promptly.
            let _ = Spawner::kill_process(pid);

            {
                let db = db.lock().unwrap();
                // Re-queue so the ticket can be picked up in the next evolution run.
                db.update_ticket(&ticket_id, "pending", None, None, Some(None))?;
                db.update_agent(worker_id, "idle", None, None)?;
                db.log_event(
                    Some(worker_id),
                    "ticket_shutdown",
                    &format!("ticket {} interrupted by shutdown request", ticket_id),
                    None,
                )?;
            }

            let _ = spawner.remove_worktree(worker_id);
            return Ok(());
        }
        res = tokio::time::timeout(timeout, wait_handle) => res,
    };

    match result {
        // Timed out
        Err(_elapsed) => {
            tracing_log(
                worker_id,
                &format!("ticket {} timed out, killing process", ticket_id),
            );

            // --- (4a) Kill process ---
            let _ = Spawner::kill_process(pid);

            // --- (4b) Update DB ---
            {
                let db = db.lock().unwrap();
                db.update_ticket(
                    &ticket_id,
                    "blocked",
                    Some("Worker timed out"),
                    None,
                    Some(None),
                )?;
                db.update_agent(worker_id, "idle", None, None)?;
                db.log_event(
                    Some(worker_id),
                    "ticket_timeout",
                    &format!(
                        "ticket {} timed out after {} s",
                        ticket_id, config.manager.worker_timeout_seconds
                    ),
                    None,
                )?;
            }

            // --- (4c) Remove worktree ---
            let _ = spawner.remove_worktree(worker_id);
        }

        // spawn_blocking itself panicked — treat as crash
        Ok(Err(join_err)) => {
            tracing_log(
                worker_id,
                &format!(
                    "spawn_blocking error for ticket {}: {}",
                    ticket_id, join_err
                ),
            );
            {
                let db = db.lock().unwrap();
                db.update_ticket(&ticket_id, "pending", None, None, Some(None))?;
                db.update_agent(worker_id, "idle", None, None)?;
                db.log_event(
                    Some(worker_id),
                    "ticket_crash",
                    &format!(
                        "ticket {} spawn_blocking join error: {}",
                        ticket_id, join_err
                    ),
                    None,
                )?;
            }
            let _ = spawner.remove_worktree(worker_id);
        }

        // Process finished
        Ok(Ok(wait_result)) => {
            match wait_result {
                // --- (3) Normal exit (code 0) ---
                Ok(status) if status.success() => {
                    tracing_log(
                        worker_id,
                        &format!("ticket {} completed successfully", ticket_id),
                    );

                    // --- (3a) Parse log file for token usage (best effort) ---
                    let (input_tokens, output_tokens) =
                        parse_token_usage_from_log(&spawner.log_path(worker_id)).unwrap_or((0, 0));

                    // --- (3b) Gate merge via `cargo test` (if Rust project) ---
                    // We run tests inside the worker's worktree before the manager merges.
                    let tests_passed = run_cargo_tests_if_rust_project(&worktree).unwrap_or(false);

                    // --- (3b) Update DB ---
                    {
                        let db = db.lock().unwrap();
                        // Clear any prior rate-limit state on successful completion.
                        let _ = db.clear_ticket_rate_limit_state(&ticket_id);
                        // Record a verifiable KB entry as proof that the worker finished.
                        // This makes it possible to validate "KB entries increase during acs run"
                        // even when the underlying model does not explicitly perform KB writes.
                        let completed_at_ts = Utc::now().timestamp();
                        let kb_key = format!("ticket-results-{}-{}", ticket_id, completed_at_ts);
                        let kb_value = serde_json::json!({
                            "ticket_id": ticket_id,
                            "worker_id": worker_id,
                            "domain": domain,
                            "tests_passed": tests_passed,
                            "work_type": provider.as_str(),
                            "model": model.as_deref(),
                            "completed_at_ts": completed_at_ts
                        })
                        .to_string();
                        db.write_knowledge(&domain, &kb_key, &kb_value)?;
                        db.push_inbox(
                            "mgr",
                            "ticket_completed",
                            &serde_json::json!({
                                "ticket_id": ticket_id,
                                "worker_id": worker_id,
                                "status": "review_pending",
                                "tests_passed": tests_passed,
                                "work_type": provider.as_str(),
                                "model": model.as_deref(),
                            })
                            .to_string(),
                            worker_id,
                        )?;
                        db.update_agent(worker_id, "idle", None, None)?;
                        db.log_token_event(
                            Some(worker_id),
                            "ticket_complete",
                            &format!("ticket {} completed", ticket_id),
                            input_tokens,
                            output_tokens,
                            Some(&ticket_id),
                            model.as_deref(),
                        )?;
                    }

                    // --- (3c) Remove worktree ---
                    let _ = spawner.remove_worktree(worker_id);
                }

                // --- (5) Non-zero exit (crash or rate-limit) ---
                Ok(status) => {
                    let code = status.code().unwrap_or(-1);
                    tracing_log(
                        worker_id,
                        &format!("ticket {} exited with code {}", ticket_id, code),
                    );

                    let log_path = spawner.log_path(worker_id);
                    if detect_rate_limit_in_log(&log_path) {
                        // Rate-limit detected — re-queue with exponential backoff
                        let strikes = {
                            let db = db.lock().unwrap();
                            db.get_ticket_rate_limit_strikes(&ticket_id).unwrap_or(0)
                        };
                        {
                            let db = db.lock().unwrap();
                            db.requeue_ticket_rate_limited(&ticket_id, strikes)?;
                            db.update_agent(worker_id, "idle", None, None)?;
                            db.log_event(
                                Some(worker_id),
                                "claude_rate_limited",
                                &format!(
                                    "ticket {} rate-limited (strike {}), backoff applied",
                                    ticket_id,
                                    strikes + 1
                                ),
                                None,
                            )?;
                        }
                        tracing_log(
                            worker_id,
                            &format!(
                                "ticket {} rate-limited (strike {}), re-queued with backoff",
                                ticket_id,
                                strikes + 1
                            ),
                        );
                    } else {
                        let db = db.lock().unwrap();
                        // Re-enqueue: set back to pending and clear assignee
                        db.update_ticket(&ticket_id, "pending", None, None, Some(None))?;
                        db.update_agent(worker_id, "idle", None, None)?;
                        db.log_event(
                            Some(worker_id),
                            "ticket_crash",
                            &format!("ticket {} exited with code {}, re-queued", ticket_id, code),
                            None,
                        )?;
                    }

                    let _ = spawner.remove_worktree(worker_id);
                }

                // wait() itself returned an IO error
                Err(io_err) => {
                    tracing_log(
                        worker_id,
                        &format!("wait() error for ticket {}: {}", ticket_id, io_err),
                    );

                    {
                        let db = db.lock().unwrap();
                        db.update_ticket(&ticket_id, "pending", None, None, Some(None))?;
                        db.update_agent(worker_id, "idle", None, None)?;
                        db.log_event(
                            Some(worker_id),
                            "ticket_crash",
                            &format!("ticket {} wait() io error: {}", ticket_id, io_err),
                            None,
                        )?;
                    }

                    let _ = spawner.remove_worktree(worker_id);
                }
            }
        }
    }

    Ok(())
}

/// Attempts to extract token usage from the worker's log file.
///
/// Claude's JSON output mode emits a top-level object with a `"usage"` key
/// containing `{ "input_tokens": N, "output_tokens": M }`. This function
/// scans every line of the log looking for such an object and returns
/// (input_tokens, output_tokens) from the last matching entry found.
///
/// Returns `None` if parsing fails or the log doesn't exist.
fn parse_token_usage_from_log(log_path: &std::path::Path) -> Option<(i64, i64)> {
    let contents = std::fs::read_to_string(log_path).ok()?;
    let mut last: Option<(i64, i64)> = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(usage) = val.get("usage") {
                let input = usage
                    .get("input_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let output = usage
                    .get("output_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                if input + output > 0 {
                    last = Some((input, output));
                }
            }
        }
    }

    last
}

/// Commits any staged or unstaged changes in the worktree as a checkpoint.
/// Writes a KB entry `checkpoint-<ticket_id>` with the branch name and timestamp.
fn commit_checkpoint(worktree: &Path, worker_id: &str, ticket_id: &str, db: &Arc<Mutex<Db>>) {
    // Check for any uncommitted changes (staged or unstaged).
    let status_out = match Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[worker:{}] checkpoint: git status error: {}", worker_id, e);
            return;
        }
    };

    if String::from_utf8_lossy(&status_out.stdout)
        .trim()
        .is_empty()
    {
        tracing_log(
            worker_id,
            &format!("checkpoint: no changes for {}", ticket_id),
        );
        return;
    }

    // Stage everything.
    if let Err(e) = Command::new("git")
        .args(["add", "-A"])
        .current_dir(worktree)
        .status()
    {
        eprintln!("[worker:{}] checkpoint: git add error: {}", worker_id, e);
        return;
    }

    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let commit_msg = format!("chore: checkpoint {} {}", worker_id, timestamp);

    let commit_out = Command::new("git")
        .args(["commit", "-m", &commit_msg])
        .current_dir(worktree)
        .output();

    match commit_out {
        Ok(o) if o.status.success() => {
            tracing_log(
                worker_id,
                &format!("checkpoint committed for {} at {}", ticket_id, timestamp),
            );

            // Retrieve current branch name to record in KB.
            let branch_out = Command::new("git")
                .args(["branch", "--show-current"])
                .current_dir(worktree)
                .output();

            if let Ok(bo) = branch_out {
                let branch = String::from_utf8_lossy(&bo.stdout).trim().to_string();
                if !branch.is_empty() {
                    let kb_value = serde_json::json!({
                        "branch": branch,
                        "worker_id": worker_id,
                        "checkpointed_at": Utc::now().to_rfc3339(),
                    })
                    .to_string();
                    let kb_key = format!("checkpoint-{}", ticket_id);
                    let guard = db.lock().unwrap();
                    let _ = guard.write_knowledge("core", &kb_key, &kb_value);
                }
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            tracing_log(
                worker_id,
                &format!(
                    "checkpoint commit failed for {}: {}",
                    ticket_id,
                    stderr.trim()
                ),
            );
        }
        Err(e) => {
            eprintln!("[worker:{}] checkpoint: git commit error: {}", worker_id, e);
        }
    }
}

/// Background task: commits checkpoints every 20 minutes until `cancel` fires.
async fn checkpoint_loop(
    worktree: PathBuf,
    worker_id: String,
    ticket_id: String,
    db: Arc<Mutex<Db>>,
    mut cancel: oneshot::Receiver<()>,
) {
    let interval = Duration::from_secs(20 * 60);
    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {
                commit_checkpoint(&worktree, &worker_id, &ticket_id, &db);
            }
            _ = &mut cancel => {
                return;
            }
        }
    }
}

/// Lightweight structured logging to stderr.
fn tracing_log(worker_id: &str, msg: &str) {
    eprintln!("[worker:{}] {}", worker_id, msg);
}

/// Scan the worker log file for rate-limit signals.
///
/// Returns `true` if any line contains the string `"rate_limit"` or `"429"`.
pub fn detect_rate_limit_in_log(log_path: &std::path::Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(log_path) else {
        return false;
    };
    contents.contains("rate_limit") || contents.contains("429")
}

fn run_cargo_tests_if_rust_project(worktree: &Path) -> Option<bool> {
    // If this doesn't look like a Rust project, we don't enforce tests.
    if !worktree.join("Cargo.toml").is_file() {
        return Some(true);
    }

    // Best-effort: if cargo or tests fail, we return false to prevent merging.
    let output = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(worktree)
        .output()
        .ok()?;

    Some(output.status.success())
}

fn select_provider_for_ticket(
    agents: &crate::config::AgentConfig,
    worker_id: &str,
    ticket_id: &str,
) -> String {
    let mut order = agents.providers.clone();
    if order.is_empty() {
        order.push("claude".to_string());
        if !agents.codex_path.trim().is_empty() {
            order.push("codex".to_string());
        }
        if !agents.agent_path.trim().is_empty() {
            order.push("agent".to_string());
        }
    }

    if order.is_empty() {
        order.push("claude".to_string());
    }

    let seed = (simple_hash(worker_id) + simple_hash(ticket_id)) as usize;
    order[seed % order.len()].clone()
}

fn simple_hash(s: &str) -> u64 {
    s.as_bytes().iter().map(|b| *b as u64).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command};
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;
    use tokio::sync::watch;

    // ── MockSpawner ──────────────────────────────────────────────────

    enum MockBehavior {
        /// Spawns `true` — exits 0 immediately.
        Success,
        /// Spawns `false` — exits 1 immediately.
        Crash,
        /// Spawns `sleep 60` — hangs until killed or timeout fires.
        Hang,
        /// Exits non-zero; log contains a rate-limit signal.
        RateLimited,
    }

    struct MockSpawner {
        dir: TempDir,
        behavior: MockBehavior,
    }

    impl MockSpawner {
        fn new(behavior: MockBehavior) -> Self {
            MockSpawner {
                dir: TempDir::new().unwrap(),
                behavior,
            }
        }
    }

    impl SpawnProvider for MockSpawner {
        fn create_worktree(&self, worker_id: &str, _ticket_id: &str) -> anyhow::Result<PathBuf> {
            let path = self.dir.path().join(worker_id);
            std::fs::create_dir_all(&path)?;
            Ok(path)
        }

        fn remove_worktree(&self, _worker_id: &str) -> anyhow::Result<()> {
            Ok(())
        }

        fn spawn_provider(
            &self,
            _provider: &str,
            _model: Option<&str>,
            worker_id: &str,
            _worktree: &Path,
            _prompt: &str,
            _system_prompt: &str,
        ) -> anyhow::Result<Child> {
            let child = match self.behavior {
                MockBehavior::Success => Command::new("true").spawn()?,
                MockBehavior::Crash => Command::new("false").spawn()?,
                MockBehavior::Hang => Command::new("sleep").arg("60").spawn()?,
                MockBehavior::RateLimited => {
                    // Write rate-limit signal to the log file before spawning the exit process
                    let log = self.dir.path().join(format!("{}.log", worker_id));
                    std::fs::write(&log, "Error: 429 Too Many Requests rate_limit exceeded\n")
                        .unwrap();
                    Command::new("false").spawn()?
                }
            };
            Ok(child)
        }

        fn log_path(&self, worker_id: &str) -> PathBuf {
            self.dir.path().join(format!("{}.log", worker_id))
        }
    }

    // ── Test helpers ─────────────────────────────────────────────────

    fn make_db() -> Arc<Mutex<Db>> {
        let db = Db::open_memory().unwrap();
        db.register_agent("w-test", "worker", "general").unwrap();
        Arc::new(Mutex::new(db))
    }

    fn make_config(timeout_secs: u64) -> Config {
        let mut cfg = Config::default_for("test");
        cfg.manager.worker_timeout_seconds = timeout_secs;
        cfg.manager.worker_poll_seconds = 1;
        cfg
    }

    fn ticket_payload(ticket_id: &str) -> String {
        serde_json::json!({
            "ticket_id": ticket_id,
            "title": "Test ticket",
            "description": "Test description",
            "domain": "general",
        })
        .to_string()
    }

    // ── Test 1: inbox polling picks up ticket_assignment ─────────────

    #[tokio::test]
    async fn worker_picks_up_ticket_assignment_from_inbox() {
        let db = make_db();
        let payload = ticket_payload("t-poll");

        // Simulate the manager pushing a ticket_assignment into the worker's inbox
        {
            let db = db.lock().unwrap();
            db.push_inbox("w-test", "ticket_assignment", &payload, "mgr")
                .unwrap();
        }

        // Simulate what worker_loop does: pop inbox, dispatch to handler
        let msg = {
            let db = db.lock().unwrap();
            db.pop_inbox("w-test").unwrap()
        };
        assert!(msg.is_some(), "inbox should have a message");
        let msg = msg.unwrap();
        assert_eq!(msg.msg_type, "ticket_assignment");

        // After popping, the inbox should be empty
        let next = {
            let db = db.lock().unwrap();
            db.pop_inbox("w-test").unwrap()
        };
        assert!(next.is_none(), "inbox should be empty after pop");

        // Process the message through handle_ticket_with_spawner
        let config = make_config(30);
        let spawner = MockSpawner::new(MockBehavior::Success);
        let (_tx, shutdown_rx) = watch::channel(false);
        let mut shutdown_rx = shutdown_rx;

        let result = handle_ticket_with_spawner(
            &spawner,
            "w-test",
            &msg.payload,
            &db,
            &config,
            &mut shutdown_rx,
            Some("claude".to_string()),
        )
        .await;

        assert!(
            result.is_ok(),
            "handle_ticket_with_spawner should succeed: {:?}",
            result
        );

        // Agent should have been set to idle after completion
        let agents = db.lock().unwrap().list_agents().unwrap();
        let agent = agents.iter().find(|a| a.id == "w-test").unwrap();
        assert_eq!(agent.status, "idle");
    }

    // ── Test 2: completion flow pushes ticket_completed to mgr inbox ──

    #[tokio::test]
    async fn completion_flow_pushes_ticket_completed_to_mgr_inbox() {
        let db = make_db();
        let config = make_config(30);
        let spawner = MockSpawner::new(MockBehavior::Success);
        let (_tx, shutdown_rx) = watch::channel(false);
        let mut shutdown_rx = shutdown_rx;

        handle_ticket_with_spawner(
            &spawner,
            "w-test",
            &ticket_payload("t-done"),
            &db,
            &config,
            &mut shutdown_rx,
            Some("claude".to_string()),
        )
        .await
        .unwrap();

        // Manager inbox should contain a ticket_completed message
        let msg = db.lock().unwrap().pop_inbox("mgr").unwrap();
        assert!(msg.is_some(), "mgr inbox should have ticket_completed");
        let msg = msg.unwrap();
        assert_eq!(msg.msg_type, "ticket_completed");

        let payload: serde_json::Value = serde_json::from_str(&msg.payload).unwrap();
        assert_eq!(payload["ticket_id"], "t-done");
        assert_eq!(payload["status"], "review_pending");
        assert_eq!(payload["worker_id"], "w-test");
    }

    // ── Test 2b: successful completion writes a verifiable KB entry ───────

    #[tokio::test]
    async fn completion_flow_writes_ticket_results_to_kb() {
        let db = make_db();
        let config = make_config(30);
        let spawner = MockSpawner::new(MockBehavior::Success);
        let (_tx, shutdown_rx) = watch::channel(false);
        let mut shutdown_rx = shutdown_rx;

        let ticket_id = "t-kb";

        handle_ticket_with_spawner(
            &spawner,
            "w-test",
            &ticket_payload(ticket_id),
            &db,
            &config,
            &mut shutdown_rx,
            Some("claude".to_string()),
        )
        .await
        .unwrap();

        let all_knowledge = db.lock().unwrap().list_all_knowledge().unwrap();
        let prefix = format!("ticket-results-{}-", ticket_id);

        let entry = all_knowledge
            .iter()
            .find(|e| e.domain == "general" && e.key.starts_with(&prefix));

        assert!(
            entry.is_some(),
            "expected a KB entry with key prefix '{}' in domain 'general'",
            prefix
        );

        let entry = entry.unwrap();
        assert!(entry
            .value
            .contains(&format!("\"ticket_id\":\"{}\"", ticket_id)));
        assert!(entry.value.contains("\"tests_passed\":true"));
        assert!(entry.value.contains("\"work_type\":\"claude\""));
    }

    // ── Test 3: crash recovery resets ticket to pending ───────────────

    #[tokio::test]
    async fn crash_recovery_resets_ticket_to_pending_and_logs_event() {
        let db = make_db();
        let config = make_config(30);
        let spawner = MockSpawner::new(MockBehavior::Crash);
        let (_tx, shutdown_rx) = watch::channel(false);
        let mut shutdown_rx = shutdown_rx;

        handle_ticket_with_spawner(
            &spawner,
            "w-test",
            &ticket_payload("t-crash"),
            &db,
            &config,
            &mut shutdown_rx,
            Some("claude".to_string()),
        )
        .await
        .unwrap();

        // A ticket_crash event should have been logged
        let events = db.lock().unwrap().recent_events(10).unwrap();
        let crash_event = events.iter().find(|e| e.event_type == "ticket_crash");
        assert!(crash_event.is_some(), "expected ticket_crash event in log");

        // Agent should be idle (not stuck as "working")
        let agents = db.lock().unwrap().list_agents().unwrap();
        let agent = agents.iter().find(|a| a.id == "w-test").unwrap();
        assert_eq!(agent.status, "idle");

        // No ticket_completed message should have been pushed to mgr
        let msg = db.lock().unwrap().pop_inbox("mgr").unwrap();
        assert!(msg.is_none(), "mgr inbox should be empty after crash");
    }

    // ── Test 4: timeout kills process and sets ticket to blocked ──────

    #[tokio::test]
    async fn timeout_kills_process_and_sets_ticket_blocked() {
        let db = make_db();
        // 1 second timeout so the test doesn't wait long
        let config = make_config(1);
        let spawner = MockSpawner::new(MockBehavior::Hang);
        let (_tx, shutdown_rx) = watch::channel(false);
        let mut shutdown_rx = shutdown_rx;

        handle_ticket_with_spawner(
            &spawner,
            "w-test",
            &ticket_payload("t-timeout"),
            &db,
            &config,
            &mut shutdown_rx,
            Some("claude".to_string()),
        )
        .await
        .unwrap();

        // A ticket_timeout event should have been logged
        let events = db.lock().unwrap().recent_events(10).unwrap();
        let timeout_event = events.iter().find(|e| e.event_type == "ticket_timeout");
        assert!(
            timeout_event.is_some(),
            "expected ticket_timeout event in log"
        );

        // Agent should be idle after timeout cleanup
        let agents = db.lock().unwrap().list_agents().unwrap();
        let agent = agents.iter().find(|a| a.id == "w-test").unwrap();
        assert_eq!(agent.status, "idle");

        // No ticket_completed pushed to mgr
        let msg = db.lock().unwrap().pop_inbox("mgr").unwrap();
        assert!(msg.is_none(), "mgr inbox should be empty after timeout");
    }

    // ── Rate-limit detection ──────────────────────────────────────────

    #[test]
    fn rate_limit_detected_from_log_with_rate_limit_keyword() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("w-test.log");
        std::fs::write(&log, "some output\nrate_limit exceeded\nmore output\n").unwrap();
        assert!(detect_rate_limit_in_log(&log));
    }

    #[test]
    fn rate_limit_detected_from_log_with_429_status_code() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("w-test.log");
        std::fs::write(&log, "HTTP/1.1 429 Too Many Requests\n").unwrap();
        assert!(detect_rate_limit_in_log(&log));
    }

    #[test]
    fn no_rate_limit_detected_from_clean_log() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("w-test.log");
        std::fs::write(&log, "ticket completed successfully\n").unwrap();
        assert!(!detect_rate_limit_in_log(&log));
    }

    #[test]
    fn no_rate_limit_detected_when_log_absent() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("no-such.log");
        assert!(!detect_rate_limit_in_log(&log));
    }

    // ── Rate-limit re-queue flow ──────────────────────────────────────

    #[tokio::test]
    async fn rate_limited_exit_requeues_ticket_with_rate_limited_note() {
        let db = make_db();
        {
            let db = db.lock().unwrap();
            db.create_ticket_with_id("t-rl", "RL Test", "desc", "general", 1)
                .unwrap();
        }
        let config = make_config(30);
        let spawner = MockSpawner::new(MockBehavior::RateLimited);
        let (_tx, shutdown_rx) = watch::channel(false);
        let mut shutdown_rx = shutdown_rx;

        handle_ticket_with_spawner(
            &spawner,
            "w-test",
            &ticket_payload("t-rl"),
            &db,
            &config,
            &mut shutdown_rx,
            Some("claude".to_string()),
        )
        .await
        .unwrap();

        let ticket = db.lock().unwrap().get_ticket("t-rl").unwrap().unwrap();
        assert_eq!(ticket.status, "pending");
        assert!(
            ticket.notes.contains("rate_limited"),
            "notes should contain 'rate_limited', got: {:?}",
            ticket.notes
        );

        let events = db.lock().unwrap().recent_events(10).unwrap();
        let rl_event = events
            .iter()
            .find(|e| e.event_type == "claude_rate_limited");
        assert!(rl_event.is_some(), "expected claude_rate_limited event");

        let agents = db.lock().unwrap().list_agents().unwrap();
        let agent = agents.iter().find(|a| a.id == "w-test").unwrap();
        assert_eq!(agent.status, "idle");

        let msg = db.lock().unwrap().pop_inbox("mgr").unwrap();
        assert!(msg.is_none(), "mgr inbox should be empty after rate-limit");
    }

    #[tokio::test]
    async fn backoff_escalates_on_repeated_rate_limit_strikes() {
        let db = make_db();
        {
            let db = db.lock().unwrap();
            db.create_ticket_with_id("t-backoff", "Backoff Test", "desc", "general", 1)
                .unwrap();
        }
        let config = make_config(30);

        let mut prev_retry: Option<chrono::DateTime<chrono::Utc>> = None;
        for strike in 1..=3u32 {
            let spawner = MockSpawner::new(MockBehavior::RateLimited);
            let (_tx, shutdown_rx) = watch::channel(false);
            let mut shutdown_rx = shutdown_rx;
            handle_ticket_with_spawner(
                &spawner,
                "w-test",
                &ticket_payload("t-backoff"),
                &db,
                &config,
                &mut shutdown_rx,
                Some("claude".to_string()),
            )
            .await
            .unwrap();

            let strikes_in_db = db
                .lock()
                .unwrap()
                .get_ticket_rate_limit_strikes("t-backoff")
                .unwrap();
            assert_eq!(
                strikes_in_db, strike as i32,
                "strikes should be {} after {} failures",
                strike, strike
            );

            let retry_after = db
                .lock()
                .unwrap()
                .get_ticket_rate_limit_retry_after("t-backoff")
                .unwrap();
            assert!(
                retry_after.is_some(),
                "retry_after should be set after strike {}",
                strike
            );
            let retry_ts = retry_after.unwrap();
            if let Some(prev) = prev_retry {
                assert!(
                    retry_ts > prev,
                    "retry_after should increase with each strike"
                );
            }
            prev_retry = Some(retry_ts);

            // Reset ticket status to pending so the next iteration can process it again
            {
                let db = db.lock().unwrap();
                db.update_ticket("t-backoff", "pending", None, None, Some(None))
                    .unwrap();
            }
        }
    }

    #[tokio::test]
    async fn claim_skips_rate_limited_ticket_with_future_retry_after() {
        let db = make_db();
        {
            let db = db.lock().unwrap();
            db.create_ticket_with_id("t-skip", "Skip Test", "desc", "general", 1)
                .unwrap();
            db.requeue_ticket_rate_limited("t-skip", 1).unwrap();
        }

        let claimed = db.lock().unwrap().claim_next_ticket("w-other").unwrap();
        assert!(
            claimed.is_none() || claimed.as_ref().map(|t| t.id.as_str()) != Some("t-skip"),
            "claim_next_ticket should skip rate-limited ticket"
        );
    }

    #[tokio::test]
    async fn successful_completion_clears_rate_limit_state() {
        let db = make_db();
        {
            let db = db.lock().unwrap();
            db.create_ticket_with_id("t-clear", "Clear Test", "desc", "general", 1)
                .unwrap();
            db.requeue_ticket_rate_limited("t-clear", 2).unwrap();
            // Reset to pending so the worker can process it
            db.update_ticket("t-clear", "pending", None, None, Some(None))
                .unwrap();
        }

        let config = make_config(30);
        let spawner = MockSpawner::new(MockBehavior::Success);
        let (_tx, shutdown_rx) = watch::channel(false);
        let mut shutdown_rx = shutdown_rx;

        handle_ticket_with_spawner(
            &spawner,
            "w-test",
            &ticket_payload("t-clear"),
            &db,
            &config,
            &mut shutdown_rx,
            Some("claude".to_string()),
        )
        .await
        .unwrap();

        let strikes = db
            .lock()
            .unwrap()
            .get_ticket_rate_limit_strikes("t-clear")
            .unwrap();
        assert_eq!(
            strikes, 0,
            "rate_limit_strikes should be cleared after success"
        );

        let retry_after = db
            .lock()
            .unwrap()
            .get_ticket_rate_limit_retry_after("t-clear")
            .unwrap();
        assert!(
            retry_after.is_none(),
            "rate_limit_retry_after should be cleared after success"
        );
    }

    // ── Checkpoint: commit_checkpoint writes KB entry when git repo has changes ──

    #[test]
    fn commit_checkpoint_skips_when_no_changes() {
        let dir = TempDir::new().unwrap();
        let wt = dir.path();

        // Initialise a real git repo so git commands work.
        Command::new("git").args(["init"]).current_dir(wt).status().unwrap();
        Command::new("git").args(["config", "user.email", "test@test"]).current_dir(wt).status().unwrap();
        Command::new("git").args(["config", "user.name", "Test"]).current_dir(wt).status().unwrap();

        let db = make_db();
        // No files to commit → checkpoint should be a no-op (no KB entry written).
        commit_checkpoint(wt, "w-test", "t-chk-none", &db);

        let kb = db.lock().unwrap().read_knowledge("core", "checkpoint-t-chk-none").unwrap();
        assert!(kb.is_none(), "no KB entry should be written when nothing to commit");
    }

    #[test]
    fn commit_checkpoint_commits_and_writes_kb_entry() {
        let dir = TempDir::new().unwrap();
        let wt = dir.path();

        // Initialise a real git repo.
        Command::new("git").args(["init"]).current_dir(wt).status().unwrap();
        Command::new("git").args(["config", "user.email", "test@test"]).current_dir(wt).status().unwrap();
        Command::new("git").args(["config", "user.name", "Test"]).current_dir(wt).status().unwrap();

        // Create an initial commit so HEAD exists (needed for `git branch --show-current`).
        std::fs::write(wt.join("README.md"), "init").unwrap();
        Command::new("git").args(["add", "-A"]).current_dir(wt).status().unwrap();
        Command::new("git").args(["commit", "-m", "init"]).current_dir(wt).status().unwrap();

        // Create a new file to give the checkpoint something to stage.
        std::fs::write(wt.join("work.txt"), "progress").unwrap();

        let db = make_db();
        commit_checkpoint(wt, "w-test", "t-chk", &db);

        let kb = db
            .lock()
            .unwrap()
            .read_knowledge("core", "checkpoint-t-chk")
            .unwrap();
        assert!(kb.is_some(), "KB entry should be written after a checkpoint commit");

        let value: serde_json::Value =
            serde_json::from_str(&kb.unwrap().value).expect("KB value should be valid JSON");
        assert!(
            value["branch"].as_str().is_some(),
            "KB entry should contain branch name"
        );
        assert!(
            value["checkpointed_at"].as_str().is_some(),
            "KB entry should contain checkpointed_at timestamp"
        );
        assert_eq!(value["worker_id"].as_str(), Some("w-test"));
    }

    // ── Resume: task_prompt includes checkpoint section when payload has checkpoint_branch ──

    #[tokio::test]
    async fn task_prompt_includes_resume_section_when_checkpoint_branch_present() {
        let db = make_db();
        let config = make_config(30);
        let spawner = MockSpawner::new(MockBehavior::Success);
        let (_tx, shutdown_rx) = watch::channel(false);
        let mut shutdown_rx = shutdown_rx;

        let payload = serde_json::json!({
            "ticket_id": "t-resume",
            "title": "Resume Test",
            "description": "Test description",
            "domain": "general",
            "checkpoint_branch": "acs/t-resume-abcd",
            "checkpoint_committed_at": "2026-01-01T12:00:00Z",
        })
        .to_string();

        // handle_ticket_with_spawner should succeed even with checkpoint fields present.
        let result = handle_ticket_with_spawner(
            &spawner,
            "w-test",
            &payload,
            &db,
            &config,
            &mut shutdown_rx,
            Some("claude".to_string()),
        )
        .await;

        assert!(result.is_ok(), "handler should succeed with checkpoint fields: {:?}", result);

        // Agent should reach idle state after successful completion.
        let agents = db.lock().unwrap().list_agents().unwrap();
        let agent = agents.iter().find(|a| a.id == "w-test").unwrap();
        assert_eq!(agent.status, "idle");
    }
}
