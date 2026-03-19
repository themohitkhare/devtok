// src/worker.rs
//
// Each worker runs as a tokio task, polling its inbox and spawning Claude Code
// in a git worktree when it receives a ticket_assignment message.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use tokio::sync::watch;

use crate::config::Config;
use crate::db::Db;
use crate::prompts;
use crate::spawner::Spawner;

pub async fn worker_loop(
    worker_id: String,
    db: Arc<Mutex<Db>>,
    config: Config,
    project_dir: PathBuf,
    mut shutdown: watch::Receiver<bool>,
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
                    &format!("ignoring unknown msg_type '{}': {}", message.msg_type, message.payload),
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
    project_dir: &PathBuf,
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

    tracing_log(worker_id, &format!("received ticket_assignment for {}", ticket_id));

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

    // --- (c) Create spawner ---
    let spawner = Spawner::new(project_dir, &config.agents.claude_path, &config.agents.tool_path);

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
    );

    // --- (f) Build task prompt ---
    let task_prompt = format!(
        "You are assigned ticket {ticket_id}: {title}\n\nDescription:\n{description}\n\nExecute this ticket. Use Bash to call acs commands.",
        ticket_id = ticket_id,
        title = title,
        description = description,
    );

    // --- (g) Spawn Claude ---
    let mut child = spawner.spawn_claude(worker_id, &worktree, &task_prompt, &system_prompt)?;
    let pid: u32 = child.id();

    // Update agent record with the PID
    {
        let db = db.lock().unwrap();
        db.update_agent(worker_id, "working", Some(&ticket_id), Some(pid))?;
    }

    // --- (h) Wait for completion with timeout ---
    let timeout = Duration::from_secs(config.manager.worker_timeout_seconds);

    let result = tokio::time::timeout(
        timeout,
        tokio::task::spawn_blocking(move || child.wait()),
    )
    .await;

    match result {
        // Timed out
        Err(_elapsed) => {
            tracing_log(worker_id, &format!("ticket {} timed out, killing process", ticket_id));

            // --- (4a) Kill process ---
            let _ = Spawner::kill_process(pid);

            // --- (4b) Update DB ---
            {
                let db = db.lock().unwrap();
                db.update_ticket(&ticket_id, "blocked", Some("Worker timed out"), None, Some(None))?;
                db.update_agent(worker_id, "idle", None, None)?;
                db.log_event(
                    Some(worker_id),
                    "ticket_timeout",
                    &format!("ticket {} timed out after {} s", ticket_id, config.manager.worker_timeout_seconds),
                    None,
                )?;
            }

            // --- (4c) Remove worktree ---
            let _ = spawner.remove_worktree(worker_id);
        }

        // spawn_blocking itself panicked — treat as crash
        Ok(Err(join_err)) => {
            tracing_log(worker_id, &format!("spawn_blocking error for ticket {}: {}", ticket_id, join_err));
            {
                let db = db.lock().unwrap();
                db.update_ticket(&ticket_id, "pending", None, None, Some(None))?;
                db.update_agent(worker_id, "idle", None, None)?;
                db.log_event(
                    Some(worker_id),
                    "ticket_crash",
                    &format!("ticket {} spawn_blocking join error: {}", ticket_id, join_err),
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
                    tracing_log(worker_id, &format!("ticket {} completed successfully", ticket_id));

                    // --- (3a) Parse log file for token usage (best effort) ---
                    let tokens = parse_token_usage_from_log(&spawner.log_path(worker_id));

                    // --- (3b) Update DB ---
                    {
                        let db = db.lock().unwrap();
                        db.push_inbox(
                            "mgr",
                            "ticket_completed",
                            &serde_json::json!({
                                "ticket_id": ticket_id,
                                "worker_id": worker_id,
                                "status": "review_pending",
                            })
                            .to_string(),
                            worker_id,
                        )?;
                        db.update_agent(worker_id, "idle", None, None)?;
                        db.log_event(
                            Some(worker_id),
                            "ticket_complete",
                            &format!("ticket {} completed", ticket_id),
                            tokens,
                        )?;
                    }

                    // --- (3c) Remove worktree ---
                    let _ = spawner.remove_worktree(worker_id);
                }

                // --- (5) Non-zero exit (crash) ---
                Ok(status) => {
                    let code = status.code().unwrap_or(-1);
                    tracing_log(worker_id, &format!("ticket {} exited with code {}", ticket_id, code));

                    {
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
                    tracing_log(worker_id, &format!("wait() error for ticket {}: {}", ticket_id, io_err));

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

/// Attempts to extract total token usage from the worker's log file.
///
/// Claude's JSON output mode emits a top-level object with a `"usage"` key
/// containing `{ "input_tokens": N, "output_tokens": M }`. This function
/// scans every line of the log looking for such an object and returns the
/// sum of input + output tokens from the last matching entry found.
///
/// Returns `None` if parsing fails or the log doesn't exist.
fn parse_token_usage_from_log(log_path: &std::path::Path) -> Option<i64> {
    let contents = std::fs::read_to_string(log_path).ok()?;
    let mut total: Option<i64> = None;

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
                    total = Some(input + output);
                }
            }
        }
    }

    total
}

/// Lightweight structured logging to stderr.
fn tracing_log(worker_id: &str, msg: &str) {
    eprintln!("[worker:{}] {}", worker_id, msg);
}
