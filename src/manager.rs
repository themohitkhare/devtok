use std::sync::{Arc, Mutex};
use anyhow::Result;
use serde_json::json;
use tokio::sync::watch;
use tokio::time::{sleep, Duration};

use crate::config::Config;
use crate::db::Db;
use crate::models::*;
use crate::spawner::Spawner;

pub async fn run_loop(
    db: Arc<Mutex<Db>>,
    config: &Config,
    project_dir: std::path::PathBuf,
    mut shutdown: watch::Receiver<bool>,
) {
    let cycle = Duration::from_secs(config.manager.cycle_seconds);

    loop {
        if let Err(e) = run_cycle(&db, config, &project_dir) {
            eprintln!("[manager] cycle error: {}", e);
        }

        // Sleep for cycle_seconds, but wake immediately if shutdown fires.
        // Release all locks before this await point.
        tokio::select! {
            _ = sleep(cycle) => {}
            _ = shutdown.changed() => {
                eprintln!("[manager] shutdown received, exiting loop");
                break;
            }
        }

        if *shutdown.borrow() {
            break;
        }
    }
}

fn run_cycle(db: &Arc<Mutex<Db>>, config: &Config, project_dir: &std::path::Path) -> Result<()> {
    let mut assignments = 0usize;
    let mut completions = 0usize;
    let mut unblocked = 0usize;
    let mut reviewed = 0usize;
    let mut merged = 0usize;

    // -----------------------------------------------------------------------
    // 0. Auto-review: promote review_pending → completed (v1 — no code review)
    // -----------------------------------------------------------------------
    {
        let review_tickets = {
            let guard = db.lock().unwrap();
            guard.list_tickets(Some("review_pending"))?
        };

        for ticket in review_tickets {
            {
                let guard = db.lock().unwrap();
                guard.update_ticket(&ticket.id, "completed", Some("Auto-reviewed by manager"), None)?;
                guard.log_event(
                    Some("mgr"),
                    "ticket_reviewed",
                    &format!("ticket {} auto-reviewed and completed", ticket.id),
                    None,
                )?;
            }
            eprintln!("[manager] auto-reviewed ticket {} → completed", ticket.id);
            reviewed += 1;
        }
    }

    // -----------------------------------------------------------------------
    // 1. Claim and assign tickets to idle workers
    // -----------------------------------------------------------------------
    {
        let guard = db.lock().unwrap();
        let agents = guard.list_agents()?;
        drop(guard);

        let idle_workers: Vec<Agent> = agents
            .into_iter()
            .filter(|a| a.status == "idle")
            .collect();

        for worker in idle_workers {
            let ticket_opt = {
                let guard = db.lock().unwrap();
                guard.claim_next_ticket(&worker.id)?
            };

            if let Some(ticket) = ticket_opt {
                let persona = config.persona_for_domain(&ticket.domain).to_string();
                let payload = json!({
                    "ticket_id":   ticket.id,
                    "title":       ticket.title,
                    "description": ticket.description,
                    "persona":     persona,
                }).to_string();

                {
                    let guard = db.lock().unwrap();
                    guard.push_inbox(&worker.id, "ticket_assignment", &payload, "mgr")?;
                    guard.log_event(
                        Some("mgr"),
                        "ticket_assigned",
                        &format!("assigned {} to {}", ticket.id, worker.id),
                        None,
                    )?;
                }

                eprintln!(
                    "[manager] assigned ticket {} to worker {}",
                    ticket.id, worker.id
                );
                assignments += 1;
            }
        }
    }

    // -----------------------------------------------------------------------
    // 2. Process completions from mgr inbox
    // -----------------------------------------------------------------------
    loop {
        let msg_opt = {
            let guard = db.lock().unwrap();
            guard.pop_inbox("mgr")?
        };

        match msg_opt {
            None => break,
            Some(msg) if msg.msg_type == "completion" || msg.msg_type == "ticket_completed" => {
                // Payload is expected to be JSON with at least { "ticket_id": "..." }
                let ticket_id: String = serde_json::from_str::<serde_json::Value>(&msg.payload)
                    .ok()
                    .and_then(|v| v.get("ticket_id").and_then(|t| t.as_str()).map(|s| s.to_string()))
                    .unwrap_or_else(|| msg.payload.trim().to_string());

                {
                    let guard = db.lock().unwrap();
                    guard.update_ticket(&ticket_id, "completed", None, None)?;
                    guard.log_event(
                        Some(&msg.sender),
                        "ticket_completed",
                        &format!("ticket {} completed by {}", ticket_id, msg.sender),
                        None,
                    )?;
                }

                eprintln!(
                    "[manager] ticket {} completed by {}",
                    ticket_id, msg.sender
                );
                completions += 1;

                // Attempt to merge the worker branch into main
                let spawner = Spawner::new(project_dir, &config.agents.claude_path, &config.agents.tool_path);
                match spawner.find_branch_for_ticket(&ticket_id) {
                    Ok(Some(branch)) => {
                        match spawner.merge_branch(&branch) {
                            Ok(true) => {
                                // Merge succeeded — clean up the branch
                                spawner.delete_branch(&branch);
                                {
                                    let guard = db.lock().unwrap();
                                    guard.log_event(
                                        Some("mgr"),
                                        "branch_merged",
                                        &format!("merged {} into main for ticket {}", branch, ticket_id),
                                        None,
                                    )?;
                                }
                                eprintln!("[manager] merged branch {} for ticket {}", branch, ticket_id);
                                merged += 1;
                            }
                            Ok(false) => {
                                // Merge conflict — block the ticket and re-assign
                                {
                                    let guard = db.lock().unwrap();
                                    guard.update_ticket(
                                        &ticket_id,
                                        "blocked",
                                        Some(&format!("Merge conflict on branch {}", branch)),
                                        None,
                                    )?;
                                    guard.log_event(
                                        Some("mgr"),
                                        "merge_conflict",
                                        &format!("merge conflict for ticket {} on branch {}", ticket_id, branch),
                                        None,
                                    )?;
                                }
                                eprintln!(
                                    "[manager] merge conflict for ticket {} on branch {}",
                                    ticket_id, branch
                                );
                            }
                            Err(e) => {
                                eprintln!("[manager] merge error for ticket {}: {}", ticket_id, e);
                                let guard = db.lock().unwrap();
                                guard.log_event(
                                    Some("mgr"),
                                    "merge_error",
                                    &format!("merge error for ticket {}: {}", ticket_id, e),
                                    None,
                                )?;
                            }
                        }
                    }
                    Ok(None) => {
                        // No branch found — nothing to merge (may have been manually merged)
                        eprintln!("[manager] no branch found for ticket {}, skipping merge", ticket_id);
                    }
                    Err(e) => {
                        eprintln!("[manager] error finding branch for ticket {}: {}", ticket_id, e);
                    }
                }
            }
            Some(_) => {
                // Unknown message type — ignore
            }
        }
    }

    // -----------------------------------------------------------------------
    // 3. Unblock tickets whose blocker is now completed
    // -----------------------------------------------------------------------
    {
        let blocked_tickets = {
            let guard = db.lock().unwrap();
            guard.list_tickets(Some("blocked"))?
        };

        for ticket in blocked_tickets {
            if let Some(ref blocker_id) = ticket.blocked_by {
                let blocker_done = {
                    let guard = db.lock().unwrap();
                    guard.get_ticket(blocker_id)?
                        .map(|t| t.status == "completed")
                        .unwrap_or(false)
                };

                if blocker_done {
                    // Reset to pending, clear assignee and blocked_by
                    {
                        let guard = db.lock().unwrap();
                        // update_ticket only sets status/notes/blocked_by; we also need
                        // to clear assignee. Use the underlying update with blocked_by=None.
                        guard.update_ticket(&ticket.id, "pending", None, Some(""))?;
                        guard.log_event(
                            Some("mgr"),
                            "ticket_unblocked",
                            &format!("ticket {} unblocked (blocker {} completed)", ticket.id, blocker_id),
                            None,
                        )?;
                    }

                    eprintln!(
                        "[manager] ticket {} unblocked (blocker {} completed)",
                        ticket.id, blocker_id
                    );
                    unblocked += 1;
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // 4. Log summary
    // -----------------------------------------------------------------------
    eprintln!(
        "[manager] cycle complete — assigned: {}, reviewed: {}, completions: {}, merged: {}, unblocked: {}",
        assignments, reviewed, completions, merged, unblocked
    );

    Ok(())
}
