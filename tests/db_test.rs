use acs::db::Db;

#[test]
fn test_create_and_get_ticket() {
    let db = Db::open_memory().unwrap();
    let id = db.create_ticket("Build auth", "Add login", "backend", 1).unwrap();
    assert_eq!(id, "t-001");
    let ticket = db.get_ticket(&id).unwrap().unwrap();
    assert_eq!(ticket.title, "Build auth");
    assert_eq!(ticket.status, "pending");
}

#[test]
fn test_sequential_ticket_ids() {
    let db = Db::open_memory().unwrap();
    let id1 = db.create_ticket("First", "Desc", "backend", 1).unwrap();
    let id2 = db.create_ticket("Second", "Desc", "frontend", 2).unwrap();
    assert_eq!(id1, "t-001");
    assert_eq!(id2, "t-002");
}

#[test]
fn test_claim_next_ticket() {
    let db = Db::open_memory().unwrap();
    db.create_ticket("High priority", "Desc", "backend", 1).unwrap();
    db.create_ticket("Low priority", "Desc", "backend", 5).unwrap();
    let claimed = db.claim_next_ticket("w-0").unwrap().unwrap();
    assert_eq!(claimed.title, "High priority");
    assert_eq!(claimed.status, "in_progress");
    assert_eq!(claimed.assignee.as_deref(), Some("w-0"));
}

#[test]
fn test_claim_empty_returns_none() {
    let db = Db::open_memory().unwrap();
    let claimed = db.claim_next_ticket("w-0").unwrap();
    assert!(claimed.is_none());
}

#[test]
fn test_inbox_push_pop() {
    let db = Db::open_memory().unwrap();
    db.push_inbox("w-0", "ticket_assignment", r#"{"ticket_id":"t-001"}"#, "mgr").unwrap();
    let msg = db.pop_inbox("w-0").unwrap().unwrap();
    assert_eq!(msg.msg_type, "ticket_assignment");
    assert_eq!(msg.sender, "mgr");
    // Second pop returns None (already read)
    let msg2 = db.pop_inbox("w-0").unwrap();
    assert!(msg2.is_none());
}

#[test]
fn test_knowledge_write_read() {
    let db = Db::open_memory().unwrap();
    db.write_knowledge("backend", "stack", "Rust, Axum").unwrap();
    let entry = db.read_knowledge("backend", "stack").unwrap().unwrap();
    assert_eq!(entry.value, "Rust, Axum");
    assert_eq!(entry.version, 1);
    // Overwrite bumps version
    db.write_knowledge("backend", "stack", "Rust, Actix").unwrap();
    let entry2 = db.read_knowledge("backend", "stack").unwrap().unwrap();
    assert_eq!(entry2.value, "Rust, Actix");
    assert_eq!(entry2.version, 2);
}

#[test]
fn test_agent_lifecycle() {
    let db = Db::open_memory().unwrap();
    db.register_agent("w-0", "worker", "backend-dev").unwrap();
    let agents = db.list_agents().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].status, "idle");

    db.update_agent("w-0", "working", Some("t-001"), Some(12345)).unwrap();
    let agents = db.list_agents().unwrap();
    assert_eq!(agents[0].status, "working");

    db.deregister_agent("w-0").unwrap();
    assert!(db.list_agents().unwrap().is_empty());
}

#[test]
fn test_events() {
    let db = Db::open_memory().unwrap();
    db.log_event(Some("w-0"), "ticket_completed", "Completed t-001", Some(1500)).unwrap();
    let events = db.recent_events(10).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].tokens_used, Some(1500));
}
