use acs::db::Db;

#[test]
fn test_update_ticket_status_and_notes() {
    let db = Db::open_memory().unwrap();
    let id = db.create_ticket("Task", "Desc", "backend", 1).unwrap();
    db.update_ticket(&id, "in_progress", Some("Started work"), None, None).unwrap();

    let ticket = db.get_ticket(&id).unwrap().unwrap();
    assert_eq!(ticket.status, "in_progress");
    assert_eq!(ticket.notes, "Started work");
}

#[test]
fn test_update_ticket_sets_blocked_by() {
    let db = Db::open_memory().unwrap();
    let id1 = db.create_ticket("Blocker", "Desc", "backend", 1).unwrap();
    let id2 = db.create_ticket("Blocked", "Desc", "backend", 2).unwrap();
    db.update_ticket(&id2, "blocked", Some("Needs blocker"), Some(&id1), None).unwrap();

    let ticket = db.get_ticket(&id2).unwrap().unwrap();
    assert_eq!(ticket.status, "blocked");
    assert_eq!(ticket.blocked_by.as_deref(), Some("t-001"));
}

#[test]
fn test_update_ticket_clears_blocked_by() {
    let db = Db::open_memory().unwrap();
    let id = db.create_ticket("Task", "Desc", "backend", 1).unwrap();
    db.update_ticket(&id, "blocked", None, Some("t-000"), None).unwrap();
    db.update_ticket(&id, "pending", None, None, None).unwrap();

    let ticket = db.get_ticket(&id).unwrap().unwrap();
    assert_eq!(ticket.status, "pending");
    // blocked_by should be cleared (set to NULL via None param)
    assert!(ticket.blocked_by.is_none());
}

#[test]
fn test_update_ticket_preserves_notes_when_none() {
    let db = Db::open_memory().unwrap();
    let id = db.create_ticket("Task", "Desc", "backend", 1).unwrap();
    db.update_ticket(&id, "in_progress", Some("Note1"), None, None).unwrap();
    // Update status but pass None for notes — should preserve existing notes
    db.update_ticket(&id, "review_pending", None, None, None).unwrap();

    let ticket = db.get_ticket(&id).unwrap().unwrap();
    assert_eq!(ticket.status, "review_pending");
    assert_eq!(ticket.notes, "Note1");
}

#[test]
fn test_get_ticket_nonexistent_returns_none() {
    let db = Db::open_memory().unwrap();
    let result = db.get_ticket("t-999").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_count_by_status_empty() {
    let db = Db::open_memory().unwrap();
    let counts = db.count_by_status().unwrap();
    assert!(counts.is_empty());
}

#[test]
fn test_count_by_status_multiple() {
    let db = Db::open_memory().unwrap();
    db.create_ticket("A", "d", "backend", 1).unwrap();
    db.create_ticket("B", "d", "backend", 1).unwrap();
    db.create_ticket("C", "d", "backend", 1).unwrap();
    let id3 = "t-003".to_string();
    db.update_ticket(&id3, "in_progress", None, None, None).unwrap();

    let counts = db.count_by_status().unwrap();
    let pending_count = counts.iter().find(|(s, _)| s == "pending").map(|(_, c)| *c).unwrap_or(0);
    let ip_count = counts.iter().find(|(s, _)| s == "in_progress").map(|(_, c)| *c).unwrap_or(0);
    assert_eq!(pending_count, 2);
    assert_eq!(ip_count, 1);
}

#[test]
fn test_list_tickets_with_status_filter() {
    let db = Db::open_memory().unwrap();
    db.create_ticket("Pending1", "d", "backend", 1).unwrap();
    db.create_ticket("Pending2", "d", "backend", 2).unwrap();
    db.update_ticket("t-002", "in_progress", None, None, None).unwrap();

    let pending = db.list_tickets(Some("pending")).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].title, "Pending1");

    let in_progress = db.list_tickets(Some("in_progress")).unwrap();
    assert_eq!(in_progress.len(), 1);
    assert_eq!(in_progress[0].title, "Pending2");
}

#[test]
fn test_list_tickets_without_filter_returns_all() {
    let db = Db::open_memory().unwrap();
    db.create_ticket("A", "d", "backend", 1).unwrap();
    db.create_ticket("B", "d", "frontend", 2).unwrap();

    let all = db.list_tickets(None).unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn test_list_tickets_ordered_by_priority() {
    let db = Db::open_memory().unwrap();
    db.create_ticket("Low", "d", "backend", 5).unwrap();
    db.create_ticket("High", "d", "backend", 1).unwrap();

    let all = db.list_tickets(None).unwrap();
    assert_eq!(all[0].title, "High");
    assert_eq!(all[1].title, "Low");
}

#[test]
fn test_claim_skips_non_pending() {
    let db = Db::open_memory().unwrap();
    let id = db.create_ticket("Task", "d", "backend", 1).unwrap();
    // Set to in_progress — should not be claimable
    db.update_ticket(&id, "in_progress", None, None, None).unwrap();

    let claimed = db.claim_next_ticket("w-1").unwrap();
    assert!(claimed.is_none());
}

#[test]
fn test_claim_skips_already_assigned() {
    let db = Db::open_memory().unwrap();
    db.create_ticket("Task", "d", "backend", 1).unwrap();
    // First claim
    let first = db.claim_next_ticket("w-0").unwrap();
    assert!(first.is_some());
    // Second claim on same queue — no more pending tickets
    let second = db.claim_next_ticket("w-1").unwrap();
    assert!(second.is_none());
}

#[test]
fn test_concurrent_claim_safety() {
    // Simulates two agents trying to claim from the same pool
    let db = Db::open_memory().unwrap();
    db.create_ticket("Only One", "d", "backend", 1).unwrap();

    let c1 = db.claim_next_ticket("w-0").unwrap();
    let c2 = db.claim_next_ticket("w-1").unwrap();

    // Exactly one should succeed, the other should get None
    assert!(c1.is_some());
    assert!(c2.is_none());
}

#[test]
fn test_inbox_pop_empty() {
    let db = Db::open_memory().unwrap();
    let msg = db.pop_inbox("w-0").unwrap();
    assert!(msg.is_none());
}

#[test]
fn test_inbox_pop_returns_oldest_first() {
    let db = Db::open_memory().unwrap();
    db.push_inbox("w-0", "type_a", "first", "mgr").unwrap();
    db.push_inbox("w-0", "type_b", "second", "mgr").unwrap();

    let msg1 = db.pop_inbox("w-0").unwrap().unwrap();
    assert_eq!(msg1.payload, "first");

    let msg2 = db.pop_inbox("w-0").unwrap().unwrap();
    assert_eq!(msg2.payload, "second");
}

#[test]
fn test_inbox_isolation_between_agents() {
    let db = Db::open_memory().unwrap();
    db.push_inbox("w-0", "msg", "for w-0", "mgr").unwrap();
    db.push_inbox("w-1", "msg", "for w-1", "mgr").unwrap();

    let msg = db.pop_inbox("w-0").unwrap().unwrap();
    assert_eq!(msg.payload, "for w-0");

    let msg = db.pop_inbox("w-1").unwrap().unwrap();
    assert_eq!(msg.payload, "for w-1");
}

#[test]
fn test_knowledge_read_nonexistent_returns_none() {
    let db = Db::open_memory().unwrap();
    let entry = db.read_knowledge("backend", "nonexistent").unwrap();
    assert!(entry.is_none());
}

#[test]
fn test_recent_events_respects_limit() {
    let db = Db::open_memory().unwrap();
    for i in 0..10 {
        db.log_event(None, "test", &format!("event {}", i), None).unwrap();
    }
    let events = db.recent_events(3).unwrap();
    assert_eq!(events.len(), 3);
}

#[test]
fn test_recent_events_empty_db() {
    let db = Db::open_memory().unwrap();
    let events = db.recent_events(10).unwrap();
    assert!(events.is_empty());
}

#[test]
fn test_log_event_without_tokens() {
    let db = Db::open_memory().unwrap();
    db.log_event(Some("w-0"), "test", "detail", None).unwrap();
    let events = db.recent_events(1).unwrap();
    assert_eq!(events[0].tokens_used, Some(0)); // defaults to 0 in SQL
}
