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
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub ticket_id: Option<String>,
    pub model: Option<String>,
}

/// A milestone groups tickets into a logical phase gated by CEO approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub id: i64,
    pub name: String,
    pub goal: String,
    /// Status: pending | active | awaiting_approval | approved | rejected
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    /// Ticket IDs belonging to this milestone (populated on demand)
    pub tickets: Vec<String>,
}

/// Per-ticket token usage summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketTokenUsage {
    pub ticket_id: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

/// Pricing constants per million tokens.
pub mod pricing {
    pub const SONNET_INPUT_PER_M: f64 = 3.0;
    pub const SONNET_OUTPUT_PER_M: f64 = 15.0;
    pub const OPUS_INPUT_PER_M: f64 = 15.0;
    pub const OPUS_OUTPUT_PER_M: f64 = 75.0;

    pub fn estimate_cost(input_tokens: i64, output_tokens: i64, input_per_m: f64, output_per_m: f64) -> f64 {
        (input_tokens as f64 / 1_000_000.0) * input_per_m
            + (output_tokens as f64 / 1_000_000.0) * output_per_m
    }
}
