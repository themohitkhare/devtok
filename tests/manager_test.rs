use std::sync::{Arc, Mutex};
use acs::config::Config;
use acs::db::Db;

/// Helper: creates an in-memory DB wrapped in Arc<Mutex<>>
fn test_db() -> Arc<Mutex<Db>> {
    Arc::new(Mutex::new(Db::open_memory().unwrap()))
}

/// Helper: runs one manager cycle synchronously via the run_cycle function.
/// Since run_cycle is private, we test via the public run_loop with immediate shutdown.
fn run_one_cycle(db: &Arc<Mutex<Db>>, config: &Config) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let (tx, rx) = tokio::sync::watch::channel(false);
        // Send shutdown immediately so the loop runs one cycle then exits
        tx.send(true).unwrap();
        acs::manager::run_loop(db.clone(), config, rx).await;
    });
}

#[test]
fn test_manager_assigns_ticket_to_idle_worker() {
    let db = test_db();
    let config = Config::default_for("test");

    {
        let guard = db.lock().unwrap();
        guard.register_agent("w-0", "worker", "general").unwrap();
        guard.create_ticket("Task A", "Do A", "backend", 1).unwrap();
    }

    run_one_cycle(&db, &config);

    let guard = db.lock().unwrap();
    // Ticket should be claimed (in_progress) and assigned to w-0
    let ticket = guard.get_ticket("t-001").unwrap().unwrap();
    assert_eq!(ticket.status, "in_progress");
    assert_eq!(ticket.assignee.as_deref(), Some("w-0"));

    // Worker should have an inbox message
    let msg = guard.pop_inbox("w-0").unwrap();
    assert!(msg.is_some());
    assert_eq!(msg.unwrap().msg_type, "ticket_assignment");
}

#[test]
fn test_manager_skips_busy_workers() {
    let db = test_db();
    let config = Config::default_for("test");

    {
        let guard = db.lock().unwrap();
        guard.register_agent("w-0", "worker", "general").unwrap();
        guard.update_agent("w-0", "working", Some("t-existing"), None).unwrap();
        guard.create_ticket("Task A", "Do A", "backend", 1).unwrap();
    }

    run_one_cycle(&db, &config);

    let guard = db.lock().unwrap();
    // Ticket should still be pending since no idle workers
    let ticket = guard.get_ticket("t-001").unwrap().unwrap();
    assert_eq!(ticket.status, "pending");
}

#[test]
fn test_manager_processes_completion_message() {
    let db = test_db();
    let config = Config::default_for("test");

    {
        let guard = db.lock().unwrap();
        guard.create_ticket("Task A", "Do A", "backend", 1).unwrap();
        guard.update_ticket("t-001", "in_progress", None, None).unwrap();
        // Simulate worker sending completion to manager inbox
        guard.push_inbox("mgr", "ticket_completed", r#"{"ticket_id":"t-001"}"#, "w-0").unwrap();
    }

    run_one_cycle(&db, &config);

    let guard = db.lock().unwrap();
    let ticket = guard.get_ticket("t-001").unwrap().unwrap();
    assert_eq!(ticket.status, "completed");
}

#[test]
fn test_manager_processes_legacy_completion_msg_type() {
    let db = test_db();
    let config = Config::default_for("test");

    {
        let guard = db.lock().unwrap();
        guard.create_ticket("Task A", "Do A", "backend", 1).unwrap();
        guard.update_ticket("t-001", "in_progress", None, None).unwrap();
        // Use legacy "completion" msg_type
        guard.push_inbox("mgr", "completion", r#"{"ticket_id":"t-001"}"#, "w-0").unwrap();
    }

    run_one_cycle(&db, &config);

    let guard = db.lock().unwrap();
    let ticket = guard.get_ticket("t-001").unwrap().unwrap();
    assert_eq!(ticket.status, "completed");
}

#[test]
fn test_manager_auto_reviews_review_pending() {
    let db = test_db();
    let config = Config::default_for("test");

    {
        let guard = db.lock().unwrap();
        guard.create_ticket("Task A", "Do A", "backend", 1).unwrap();
        guard.update_ticket("t-001", "review_pending", None, None).unwrap();
    }

    run_one_cycle(&db, &config);

    let guard = db.lock().unwrap();
    let ticket = guard.get_ticket("t-001").unwrap().unwrap();
    assert_eq!(ticket.status, "completed");
    assert_eq!(ticket.notes, "Auto-reviewed by manager");
}

#[test]
fn test_manager_unblocks_when_blocker_completes() {
    let db = test_db();
    let config = Config::default_for("test");

    {
        let guard = db.lock().unwrap();
        guard.create_ticket("Blocker", "Do first", "backend", 1).unwrap();
        guard.create_ticket("Blocked", "Do second", "backend", 2).unwrap();
        guard.update_ticket("t-001", "completed", None, None).unwrap();
        guard.update_ticket("t-002", "blocked", None, Some("t-001")).unwrap();
    }

    run_one_cycle(&db, &config);

    let guard = db.lock().unwrap();
    let ticket = guard.get_ticket("t-002").unwrap().unwrap();
    assert_eq!(ticket.status, "pending");
}

#[test]
fn test_manager_does_not_unblock_if_blocker_incomplete() {
    let db = test_db();
    let config = Config::default_for("test");

    {
        let guard = db.lock().unwrap();
        guard.create_ticket("Blocker", "Do first", "backend", 1).unwrap();
        guard.create_ticket("Blocked", "Do second", "backend", 2).unwrap();
        guard.update_ticket("t-001", "in_progress", None, None).unwrap();
        guard.update_ticket("t-002", "blocked", None, Some("t-001")).unwrap();
    }

    run_one_cycle(&db, &config);

    let guard = db.lock().unwrap();
    let ticket = guard.get_ticket("t-002").unwrap().unwrap();
    assert_eq!(ticket.status, "blocked");
}

#[test]
fn test_manager_empty_queue_cycle() {
    let db = test_db();
    let config = Config::default_for("test");

    // No tickets, no agents — should just run without error
    run_one_cycle(&db, &config);

    // Verify nothing was created
    let guard = db.lock().unwrap();
    let tickets = guard.list_tickets(None).unwrap();
    assert!(tickets.is_empty());
}

#[test]
fn test_manager_ignores_unknown_message_types() {
    let db = test_db();
    let config = Config::default_for("test");

    {
        let guard = db.lock().unwrap();
        guard.push_inbox("mgr", "unknown_type", "some payload", "w-0").unwrap();
    }

    // Should not panic or error
    run_one_cycle(&db, &config);
}
