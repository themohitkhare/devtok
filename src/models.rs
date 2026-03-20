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
    /// How many times this ticket was deferred due to file conflicts with in-progress tickets.
    #[serde(default)]
    pub defer_count: i32,
    /// Comma-separated list of file paths this ticket is expected to touch (for conflict detection).
    pub files_hint: Option<String>,
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
    /// Backend provider name (e.g. "claude", "cursor", "codex").
    /// Defaults to "claude" for agents registered without an explicit backend.
    #[serde(default = "default_backend")]
    pub backend: String,
}

fn default_backend() -> String { "claude".to_string() }

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

/// Persisted quality scoring for a ticket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityScore {
    pub ticket_id: String,
    pub tests_added: bool,
    pub docs_updated: bool,
    pub acceptance_criteria_met: bool,
    pub score: i32,
    pub computed_at: String,
}

/// Throughput and health metrics computed over a rolling time window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputMetrics {
    /// Completed tickets in the last 60 minutes.
    pub tickets_per_hour: f64,
    /// Average total tokens per completed ticket (all-time).
    pub avg_tokens_per_ticket: f64,
    /// Ratio of merge-conflict events to completions in the last hour.
    pub merge_conflict_rate: f64,
    /// Ratio of stale/timeout requeue events to completions in the last hour.
    pub timeout_rate: f64,
    /// Number of tickets currently pending (not yet started).
    pub pending_count: i64,
}

impl ThroughputMetrics {
    /// Estimated hours to clear the pending queue at current throughput.
    /// Returns None if tickets_per_hour is 0.
    pub fn eta_hours(&self) -> Option<f64> {
        if self.tickets_per_hour > 0.0 {
            Some(self.pending_count as f64 / self.tickets_per_hour)
        } else {
            None
        }
    }
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
