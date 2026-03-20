use acs::models::*;

#[test]
fn test_ticket_serialize_deserialize_roundtrip() {
    let ticket = Ticket {
        id: "t-001".into(),
        title: "Build auth".into(),
        description: "Add login".into(),
        domain: "backend".into(),
        priority: 1,
        status: "pending".into(),
        assignee: Some("w-0".into()),
        blocked_by: None,
        notes: "".into(),
        created_at: "2025-01-01T00:00:00Z".into(),
        updated_at: "2025-01-01T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&ticket).unwrap();
    let deserialized: Ticket = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, "t-001");
    assert_eq!(deserialized.title, "Build auth");
    assert_eq!(deserialized.assignee, Some("w-0".into()));
    assert_eq!(deserialized.blocked_by, None);
}

#[test]
fn test_ticket_with_null_optional_fields() {
    let json = r#"{
        "id": "t-002",
        "title": "Test",
        "description": "Desc",
        "domain": "qa",
        "priority": 3,
        "status": "pending",
        "assignee": null,
        "blocked_by": null,
        "notes": "",
        "created_at": "2025-01-01T00:00:00Z",
        "updated_at": "2025-01-01T00:00:00Z"
    }"#;
    let ticket: Ticket = serde_json::from_str(json).unwrap();
    assert!(ticket.assignee.is_none());
    assert!(ticket.blocked_by.is_none());
}

#[test]
fn test_agent_serialize_deserialize_roundtrip() {
    let agent = Agent {
        id: "w-0".into(),
        role: "worker".into(),
        persona: "backend-dev".into(),
        status: "idle".into(),
        current_ticket: None,
        pid: Some(12345),
        last_heartbeat: Some("2025-01-01T00:00:00Z".into()),
        backend: "claude".into(),
    };
    let json = serde_json::to_string(&agent).unwrap();
    let deserialized: Agent = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, "w-0");
    assert_eq!(deserialized.pid, Some(12345));
    assert!(deserialized.current_ticket.is_none());
}

#[test]
fn test_inbox_message_serialize_deserialize_roundtrip() {
    let msg = InboxMessage {
        id: 1,
        recipient: "w-0".into(),
        msg_type: "ticket_assignment".into(),
        payload: r#"{"ticket_id":"t-001"}"#.into(),
        sender: "mgr".into(),
        created_at: "2025-01-01T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let deserialized: InboxMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, 1);
    assert_eq!(deserialized.msg_type, "ticket_assignment");
    assert_eq!(deserialized.recipient, "w-0");
}

#[test]
fn test_knowledge_entry_serialize_deserialize_roundtrip() {
    let entry = KnowledgeEntry {
        domain: "backend".into(),
        key: "stack".into(),
        value: "Rust, Axum".into(),
        version: 2,
        updated_at: "2025-01-01T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let deserialized: KnowledgeEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.domain, "backend");
    assert_eq!(deserialized.version, 2);
}

#[test]
fn test_event_serialize_deserialize_roundtrip() {
    let event = Event {
        id: 42,
        timestamp: "2025-01-01T00:00:00Z".into(),
        agent: Some("w-0".into()),
        event_type: "ticket_completed".into(),
        detail: "Completed t-001".into(),
        tokens_used: Some(1500),
        input_tokens: Some(1000),
        output_tokens: Some(500),
        ticket_id: Some("t-001".into()),
        model: Some("claude-sonnet-4-6".into()),
    };
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, 42);
    assert_eq!(deserialized.tokens_used, Some(1500));
    assert_eq!(deserialized.agent, Some("w-0".into()));
    assert_eq!(deserialized.input_tokens, Some(1000));
    assert_eq!(deserialized.output_tokens, Some(500));
    assert_eq!(deserialized.ticket_id, Some("t-001".into()));
}

#[test]
fn test_event_with_null_agent_and_tokens() {
    let event = Event {
        id: 1,
        timestamp: "2025-01-01T00:00:00Z".into(),
        agent: None,
        event_type: "system_start".into(),
        detail: "Started".into(),
        tokens_used: None,
        input_tokens: None,
        output_tokens: None,
        ticket_id: None,
        model: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: Event = serde_json::from_str(&json).unwrap();
    assert!(deserialized.agent.is_none());
    assert!(deserialized.tokens_used.is_none());
    assert!(deserialized.input_tokens.is_none());
    assert!(deserialized.ticket_id.is_none());
}
