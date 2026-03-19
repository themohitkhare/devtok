use acs::db::Db;

/// Tests for worker-related logic that can be tested via the DB layer,
/// since the actual worker_loop requires spawning Claude processes.

#[test]
fn test_inbox_polling_returns_messages_in_order() {
    let db = Db::open_memory().unwrap();
    db.push_inbox("w-0", "ticket_assignment", r#"{"ticket_id":"t-001"}"#, "mgr").unwrap();
    db.push_inbox("w-0", "ticket_assignment", r#"{"ticket_id":"t-002"}"#, "mgr").unwrap();

    let msg1 = db.pop_inbox("w-0").unwrap().unwrap();
    assert!(msg1.payload.contains("t-001"));

    let msg2 = db.pop_inbox("w-0").unwrap().unwrap();
    assert!(msg2.payload.contains("t-002"));

    // No more messages
    assert!(db.pop_inbox("w-0").unwrap().is_none());
}

#[test]
fn test_ticket_assignment_payload_parsing() {
    let payload = r#"{"ticket_id":"t-001","title":"Build auth","description":"Add login","persona":"backend-dev"}"#;
    let val: serde_json::Value = serde_json::from_str(payload).unwrap();
    assert_eq!(val["ticket_id"].as_str().unwrap(), "t-001");
    assert_eq!(val["title"].as_str().unwrap(), "Build auth");
    assert_eq!(val["description"].as_str().unwrap(), "Add login");
    assert_eq!(val["persona"].as_str().unwrap(), "backend-dev");
}

#[test]
fn test_ticket_assignment_payload_with_missing_fields() {
    // Worker should handle missing optional fields gracefully
    let payload = r#"{"ticket_id":"t-001"}"#;
    let val: serde_json::Value = serde_json::from_str(payload).unwrap();
    assert_eq!(val["ticket_id"].as_str().unwrap(), "t-001");
    assert_eq!(val["title"].as_str().unwrap_or(""), "");
    assert_eq!(val["description"].as_str().unwrap_or(""), "");
    assert_eq!(val["domain"].as_str().unwrap_or("general"), "general");
}

#[test]
fn test_crash_recovery_resets_to_pending() {
    // Simulates what happens when a worker crashes: ticket goes back to pending
    let db = Db::open_memory().unwrap();
    db.create_ticket("Task", "Desc", "backend", 1).unwrap();
    db.register_agent("w-0", "worker", "general").unwrap();

    // Simulate assignment
    db.update_ticket("t-001", "in_progress", None, None, None).unwrap();
    db.update_agent("w-0", "working", Some("t-001"), Some(12345)).unwrap();

    // Simulate crash recovery
    db.update_ticket("t-001", "pending", None, None, None).unwrap();
    db.update_agent("w-0", "idle", None, None).unwrap();

    let ticket = db.get_ticket("t-001").unwrap().unwrap();
    assert_eq!(ticket.status, "pending");

    let agents = db.list_agents().unwrap();
    assert_eq!(agents[0].status, "idle");
    assert!(agents[0].current_ticket.is_none());
}

#[test]
fn test_timeout_sets_ticket_blocked() {
    // Simulates timeout behavior: ticket goes to blocked
    let db = Db::open_memory().unwrap();
    db.create_ticket("Task", "Desc", "backend", 1).unwrap();
    db.register_agent("w-0", "worker", "general").unwrap();

    db.update_ticket("t-001", "in_progress", None, None, None).unwrap();
    db.update_agent("w-0", "working", Some("t-001"), Some(12345)).unwrap();

    // Simulate timeout
    db.update_ticket("t-001", "blocked", Some("Worker timed out"), None, None).unwrap();
    db.update_agent("w-0", "idle", None, None).unwrap();

    let ticket = db.get_ticket("t-001").unwrap().unwrap();
    assert_eq!(ticket.status, "blocked");
    assert_eq!(ticket.notes, "Worker timed out");
}

#[test]
fn test_completion_notification_to_manager() {
    let db = Db::open_memory().unwrap();
    db.create_ticket("Task", "Desc", "backend", 1).unwrap();

    // Simulate worker completing and notifying manager
    db.push_inbox(
        "mgr",
        "ticket_completed",
        r#"{"ticket_id":"t-001","worker_id":"w-0","status":"review_pending"}"#,
        "w-0",
    ).unwrap();

    let msg = db.pop_inbox("mgr").unwrap().unwrap();
    assert_eq!(msg.msg_type, "ticket_completed");
    let val: serde_json::Value = serde_json::from_str(&msg.payload).unwrap();
    assert_eq!(val["ticket_id"].as_str().unwrap(), "t-001");
}

#[test]
fn test_parse_token_usage_from_log_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("w-0.log");

    // Write a log with JSON output containing usage
    std::fs::write(&log_path, r#"{"result":"ok","usage":{"input_tokens":1000,"output_tokens":500}}"#).unwrap();

    let contents = std::fs::read_to_string(&log_path).unwrap();
    let mut total: Option<i64> = None;
    for line in contents.lines() {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line.trim()) {
            if let Some(usage) = val.get("usage") {
                let input = usage.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                let output = usage.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                if input + output > 0 {
                    total = Some(input + output);
                }
            }
        }
    }
    assert_eq!(total, Some(1500));
}

#[test]
fn test_parse_token_usage_missing_file() {
    let contents = std::fs::read_to_string("/nonexistent/log.log");
    assert!(contents.is_err());
}
