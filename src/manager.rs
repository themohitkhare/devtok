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
                guard.update_ticket(&ticket.id, "completed", Some("Auto-reviewed by manager"), None, None)?;
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
            Some(msg) if msg.msg_type == "ticket_completed" || msg.msg_type == "completion" => {
                // Payload is expected to be JSON with at least { "ticket_id": "..." }
                let ticket_id: String = serde_json::from_str::<serde_json::Value>(&msg.payload)
                    .ok()
                    .and_then(|v| v.get("ticket_id").and_then(|t| t.as_str()).map(|s| s.to_string()))
                    .unwrap_or_else(|| msg.payload.trim().to_string());

                {
                    let guard = db.lock().unwrap();
                    guard.update_ticket(&ticket_id, "completed", None, None, None)?;
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
                        guard.update_ticket(&ticket.id, "pending", None, None, Some(None))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn setup() -> (Arc<Mutex<Db>>, Config) {
        let db = Db::open_memory().expect("in-memory db");
        let config = Config::default_for("test-project");
        (Arc::new(Mutex::new(db)), config)
    }

    // -----------------------------------------------------------------------
    // claim_and_assign: assigns pending tickets to idle workers, skips busy
    // -----------------------------------------------------------------------

    #[test]
    fn claim_and_assign_assigns_to_idle_worker() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            g.register_agent("w-1", "worker", "backend-dev").unwrap();
            g.create_ticket("Task A", "Do A", "general", 1).unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        let ticket = g.get_ticket("t-001").unwrap().unwrap();
        assert_eq!(ticket.status, "in_progress");
        assert_eq!(ticket.assignee.as_deref(), Some("w-1"));
    }

    #[test]
    fn claim_and_assign_skips_busy_workers() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            g.register_agent("w-1", "worker", "backend-dev").unwrap();
            g.update_agent("w-1", "busy", Some("t-existing"), None).unwrap();
            g.create_ticket("Task A", "Do A", "general", 1).unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        let ticket = g.get_ticket("t-001").unwrap().unwrap();
        assert_eq!(ticket.status, "pending", "ticket should remain pending when no idle workers");
        assert!(ticket.assignee.is_none());
    }

    #[test]
    fn claim_and_assign_respects_priority_order() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            g.register_agent("w-1", "worker", "backend-dev").unwrap();
            // Lower priority number = higher priority; create low-priority first
            g.create_ticket("Low priority", "LP", "general", 10).unwrap();
            g.create_ticket("High priority", "HP", "general", 1).unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        // High priority (t-002, priority=1) should be assigned first
        let t2 = g.get_ticket("t-002").unwrap().unwrap();
        assert_eq!(t2.status, "in_progress");
        assert_eq!(t2.assignee.as_deref(), Some("w-1"));
    }

    #[test]
    fn claim_and_assign_multiple_idle_workers() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            g.register_agent("w-1", "worker", "backend-dev").unwrap();
            g.register_agent("w-2", "worker", "backend-dev").unwrap();
            g.create_ticket("Task A", "Do A", "general", 1).unwrap();
            g.create_ticket("Task B", "Do B", "general", 1).unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        let t1 = g.get_ticket("t-001").unwrap().unwrap();
        let t2 = g.get_ticket("t-002").unwrap().unwrap();
        assert_eq!(t1.status, "in_progress");
        assert_eq!(t2.status, "in_progress");
        // Each ticket should have a different assignee
        assert_ne!(t1.assignee, t2.assignee);
    }

    #[test]
    fn claim_and_assign_sends_inbox_message() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            g.register_agent("w-1", "worker", "backend-dev").unwrap();
            g.create_ticket("Task A", "Do A", "general", 1).unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        let msg = g.pop_inbox("w-1").unwrap();
        assert!(msg.is_some(), "worker should receive an inbox message");
        let msg = msg.unwrap();
        assert_eq!(msg.msg_type, "ticket_assignment");
        assert!(msg.payload.contains("t-001"));
    }

    // -----------------------------------------------------------------------
    // process_completions: marks tickets completed from inbox messages
    // -----------------------------------------------------------------------

    #[test]
    fn process_completions_marks_ticket_completed() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            let tid = g.create_ticket("Task A", "Do A", "general", 1).unwrap();
            g.update_ticket(&tid, "in_progress", None, None, None).unwrap();
            // Worker sends completion to manager inbox
            g.push_inbox("mgr", "ticket_completed", r#"{"ticket_id":"t-001"}"#, "w-1").unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        let ticket = g.get_ticket("t-001").unwrap().unwrap();
        assert_eq!(ticket.status, "completed");
    }

    #[test]
    fn process_completions_handles_legacy_completion_type() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            let tid = g.create_ticket("Task A", "Do A", "general", 1).unwrap();
            g.update_ticket(&tid, "in_progress", None, None, None).unwrap();
            g.push_inbox("mgr", "completion", r#"{"ticket_id":"t-001"}"#, "w-1").unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        let ticket = g.get_ticket("t-001").unwrap().unwrap();
        assert_eq!(ticket.status, "completed");
    }

    #[test]
    fn process_completions_ignores_unknown_msg_types() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            let tid = g.create_ticket("Task A", "Do A", "general", 1).unwrap();
            g.update_ticket(&tid, "in_progress", None, None, None).unwrap();
            g.push_inbox("mgr", "random_noise", r#"{"ticket_id":"t-001"}"#, "w-1").unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        let ticket = g.get_ticket("t-001").unwrap().unwrap();
        assert_eq!(ticket.status, "in_progress", "unknown msg_type should not change ticket status");
    }

    #[test]
    fn process_completions_multiple_messages() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            g.create_ticket("Task A", "Do A", "general", 1).unwrap();
            g.create_ticket("Task B", "Do B", "general", 1).unwrap();
            g.update_ticket("t-001", "in_progress", None, None, None).unwrap();
            g.update_ticket("t-002", "in_progress", None, None, None).unwrap();
            g.push_inbox("mgr", "ticket_completed", r#"{"ticket_id":"t-001"}"#, "w-1").unwrap();
            g.push_inbox("mgr", "ticket_completed", r#"{"ticket_id":"t-002"}"#, "w-2").unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        assert_eq!(g.get_ticket("t-001").unwrap().unwrap().status, "completed");
        assert_eq!(g.get_ticket("t-002").unwrap().unwrap().status, "completed");
    }

    // -----------------------------------------------------------------------
    // unblock_tickets: resets blocked tickets when blocker completes
    // -----------------------------------------------------------------------

    #[test]
    fn unblock_tickets_resets_when_blocker_done() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            g.create_ticket("Blocker", "Blocking ticket", "general", 1).unwrap();
            g.create_ticket("Blocked", "Depends on blocker", "general", 1).unwrap();
            g.update_ticket("t-001", "completed", None, None, None).unwrap();
            g.update_ticket("t-002", "blocked", None, Some("t-001"), None).unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        let ticket = g.get_ticket("t-002").unwrap().unwrap();
        assert_eq!(ticket.status, "pending", "blocked ticket should be reset to pending");
        // blocked_by should be cleared
        assert!(
            ticket.blocked_by.is_none() || ticket.blocked_by.as_deref() == Some(""),
            "blocked_by should be cleared"
        );
    }

    #[test]
    fn unblock_tickets_stays_blocked_if_blocker_not_done() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            g.create_ticket("Blocker", "Still in progress", "general", 1).unwrap();
            g.create_ticket("Blocked", "Depends on blocker", "general", 1).unwrap();
            g.update_ticket("t-001", "in_progress", None, None, None).unwrap();
            g.update_ticket("t-002", "blocked", None, Some("t-001"), None).unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        let ticket = g.get_ticket("t-002").unwrap().unwrap();
        assert_eq!(ticket.status, "blocked");
    }

    #[test]
    fn unblock_tickets_missing_blocker_stays_blocked() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            g.create_ticket("Blocked", "Depends on nonexistent", "general", 1).unwrap();
            g.update_ticket("t-001", "blocked", None, Some("t-999"), None).unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        let ticket = g.get_ticket("t-001").unwrap().unwrap();
        assert_eq!(ticket.status, "blocked", "should stay blocked if blocker doesn't exist");
    }

    // -----------------------------------------------------------------------
    // auto_review: promotes review_pending → completed
    // -----------------------------------------------------------------------

    #[test]
    fn auto_review_promotes_review_pending_to_completed() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            g.create_ticket("Task A", "Do A", "general", 1).unwrap();
            g.update_ticket("t-001", "review_pending", None, None, None).unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        let ticket = g.get_ticket("t-001").unwrap().unwrap();
        assert_eq!(ticket.status, "completed");
        assert_eq!(ticket.notes, "Auto-reviewed by manager");
    }

    #[test]
    fn auto_review_does_not_touch_other_statuses() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            g.create_ticket("Pending", "still pending", "general", 1).unwrap();
            g.create_ticket("In prog", "in progress", "general", 1).unwrap();
            g.update_ticket("t-002", "in_progress", None, None, None).unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        // t-001 was pending and got assigned (if idle workers exist), but not auto-reviewed
        // t-002 should stay in_progress
        let t2 = g.get_ticket("t-002").unwrap().unwrap();
        assert_eq!(t2.status, "in_progress");
    }

    #[test]
    fn auto_review_multiple_tickets() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            g.create_ticket("Task A", "Do A", "general", 1).unwrap();
            g.create_ticket("Task B", "Do B", "general", 1).unwrap();
            g.update_ticket("t-001", "review_pending", None, None, None).unwrap();
            g.update_ticket("t-002", "review_pending", None, None, None).unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        assert_eq!(g.get_ticket("t-001").unwrap().unwrap().status, "completed");
        assert_eq!(g.get_ticket("t-002").unwrap().unwrap().status, "completed");
    }

    #[test]
    fn auto_review_logs_event() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            g.create_ticket("Task A", "Do A", "general", 1).unwrap();
            g.update_ticket("t-001", "review_pending", None, None, None).unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        let events = g.recent_events(10).unwrap();
        let review_event = events.iter().find(|e| e.event_type == "ticket_reviewed");
        assert!(review_event.is_some(), "should log a ticket_reviewed event");
        assert!(review_event.unwrap().detail.contains("t-001"));
    }

    // -----------------------------------------------------------------------
    // Integration: full cycle with mixed state
    // -----------------------------------------------------------------------

    #[test]
    fn full_cycle_handles_mixed_state() {
        let (db, config) = setup();
        {
            let g = db.lock().unwrap();
            // Worker
            g.register_agent("w-1", "worker", "backend-dev").unwrap();

            // review_pending ticket → should be auto-reviewed
            g.create_ticket("Review me", "needs review", "general", 1).unwrap();
            g.update_ticket("t-001", "review_pending", None, None, None).unwrap();

            // Pending ticket → should be assigned to idle w-1
            g.create_ticket("Assign me", "needs assignment", "general", 1).unwrap();

            // Blocked ticket with completed blocker → should be unblocked
            g.create_ticket("Blocker", "I block things", "general", 1).unwrap();
            g.update_ticket("t-003", "completed", None, None, None).unwrap();
            g.create_ticket("Blocked", "waiting on t-003", "general", 1).unwrap();
            g.update_ticket("t-004", "blocked", None, Some("t-003"), None).unwrap();

            // Completion message in mgr inbox
            g.create_ticket("Almost done", "completing", "general", 1).unwrap();
            g.update_ticket("t-005", "in_progress", None, None, None).unwrap();
            g.push_inbox("mgr", "ticket_completed", r#"{"ticket_id":"t-005"}"#, "w-2").unwrap();
        }

        run_cycle(&db, &config, std::path::Path::new("/tmp/test")).unwrap();

        let g = db.lock().unwrap();
        // t-001: review_pending → completed (auto-review)
        assert_eq!(g.get_ticket("t-001").unwrap().unwrap().status, "completed");
        // t-002: pending → in_progress (assigned to w-1)
        assert_eq!(g.get_ticket("t-002").unwrap().unwrap().status, "in_progress");
        // t-004: blocked → pending (blocker t-003 completed)
        assert_eq!(g.get_ticket("t-004").unwrap().unwrap().status, "pending");
        // t-005: in_progress → completed (completion message)
        assert_eq!(g.get_ticket("t-005").unwrap().unwrap().status, "completed");
    }
}
