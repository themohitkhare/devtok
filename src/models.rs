use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticket {
    pub id: String,
    pub title: String,
    pub description: String,
    pub domain: String,
    pub priority: i32,
    pub status: String,
    pub assignee: Option<String>,
    pub blocked_by: Option<String>,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub role: String,
    pub persona: String,
    pub status: String,
    pub current_ticket: Option<String>,
    pub pid: Option<u32>,
    pub last_heartbeat: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxMessage {
    pub id: i64,
    pub recipient: String,
    pub msg_type: String,
    pub payload: String,
    pub sender: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub domain: String,
    pub key: String,
    pub value: String,
    pub version: i32,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub timestamp: String,
    pub agent: Option<String>,
    pub event_type: String,
    pub detail: String,
    pub tokens_used: Option<i64>,
}
