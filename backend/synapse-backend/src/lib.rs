//! Synapse — SpacetimeDB backend for TikTok-style AI agent monitoring.
//! Tables: project, agent, action_card, feedback, concurrent_task.

use spacetimedb::{ReducerContext, Table};

// ============== Tables ==============

/// Project table: id, name, status, repository_path, created_at
#[spacetimedb::table(accessor = project, public)]
pub struct Project {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub name: String,
    pub status: String,
    pub repository_path: String,
    pub created_at: u64,
}

/// Agent table: id, name, specialty, avatar_seed, last_seen
#[spacetimedb::table(accessor = agent, public)]
pub struct Agent {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub name: String,
    pub specialty: String,
    pub avatar_seed: String,
    pub last_seen: u64,
}

/// ActionCard: status in [running, thinking, success, blocked, failed, queued, cancelled]
/// visual_type in [CodeDiff, TerminalOutput, StatusUpdate]
#[spacetimedb::table(accessor = action_card, public)]
pub struct ActionCard {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub agent_id: u64,
    pub project_id: u64,
    pub status: String,
    pub visual_type: String,
    pub content: String,
    pub task_summary: String,
    pub priority: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Feedback: action_type in [approve, reject, comment, escalate]
#[spacetimedb::table(accessor = feedback, public)]
pub struct Feedback {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub card_id: u64,
    pub action_type: String,
    pub payload: String,
    pub created_at: u64,
}

/// ConcurrentTask: task_type in [code, test, deploy, review, scan, migrate, refactor]
/// colors: code=#3b82f6, test=#10b981, deploy=#8b5cf6, review=#f59e0b, scan=#06b6d4, migrate=#ec4899, refactor=#f97316
#[spacetimedb::table(accessor = concurrent_task, public)]
pub struct ConcurrentTask {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub agent_id: u64,
    pub task_type: String,
    pub status: String,
    pub color: String,
    pub created_at: u64,
}

// ============== Reducers ==============

#[spacetimedb::reducer(init)]
pub fn init(_ctx: &ReducerContext) {
    // Called when the module is initially published
}

#[spacetimedb::reducer]
pub fn create_agent(
    ctx: &ReducerContext,
    name: String,
    specialty: String,
    avatar_seed: String,
) {
    let now = ctx.timestamp.to_micros_since_unix_epoch() as u64;
    let _ = ctx.db.agent().insert(Agent {
        id: 0,
        name,
        specialty,
        avatar_seed,
        last_seen: now,
    });
}

#[spacetimedb::reducer]
pub fn insert_action_card(
    ctx: &ReducerContext,
    agent_id: u64,
    project_id: u64,
    visual_type: String,
    content: String,
    task_summary: String,
    priority: u32,
) {
    let now = ctx.timestamp.to_micros_since_unix_epoch() as u64;
    let _ = ctx.db.action_card().insert(ActionCard {
        id: 0,
        agent_id,
        project_id,
        status: "queued".to_string(),
        visual_type,
        content,
        task_summary,
        priority,
        created_at: now,
        updated_at: now,
    });
}

#[spacetimedb::reducer]
pub fn approve_action(ctx: &ReducerContext, card_id: u64) {
    let now = ctx.timestamp.to_micros_since_unix_epoch() as u64;
    if let Some(card) = ctx.db.action_card().id().find(card_id) {
        let mut updated = card;
        updated.status = "success".to_string();
        updated.updated_at = now;
        let _ = ctx.db.action_card().id().update(updated);
    }
}

#[spacetimedb::reducer]
pub fn reject_action(ctx: &ReducerContext, card_id: u64) {
    let now = ctx.timestamp.to_micros_since_unix_epoch() as u64;
    if let Some(card) = ctx.db.action_card().id().find(card_id) {
        let mut updated = card;
        updated.status = "failed".to_string();
        updated.updated_at = now;
        let _ = ctx.db.action_card().id().update(updated);
    }
}

#[spacetimedb::reducer]
pub fn add_comment(ctx: &ReducerContext, card_id: u64, comment: String) {
    let now = ctx.timestamp.to_micros_since_unix_epoch() as u64;
    let _ = ctx.db.feedback().insert(Feedback {
        id: 0,
        card_id,
        action_type: "comment".to_string(),
        payload: comment,
        created_at: now,
    });
}

#[spacetimedb::reducer]
pub fn escalate_action(ctx: &ReducerContext, card_id: u64) {
    let now = ctx.timestamp.to_micros_since_unix_epoch() as u64;
    let _ = ctx.db.feedback().insert(Feedback {
        id: 0,
        card_id,
        action_type: "escalate".to_string(),
        payload: String::new(),
        created_at: now,
    });
}

#[spacetimedb::reducer]
pub fn update_agent_status(ctx: &ReducerContext, agent_id: u64, _new_status: String) {
    if let Some(agent) = ctx.db.agent().id().find(agent_id) {
        let mut updated = agent;
        updated.last_seen = ctx.timestamp.to_micros_since_unix_epoch() as u64;
        // Store status in a way that fits the schema - Agent has no status field.
        // Per spec we only have last_seen; treat "update status" as last_seen refresh.
        let _ = ctx.db.agent().id().update(updated);
    }
}

fn task_type_to_color(task_type: &str) -> &'static str {
    match task_type {
        "code" => "#3b82f6",
        "test" => "#10b981",
        "deploy" => "#8b5cf6",
        "review" => "#f59e0b",
        "scan" => "#06b6d4",
        "migrate" => "#ec4899",
        "refactor" => "#f97316",
        _ => "#6b7280",
    }
}

#[spacetimedb::reducer]
pub fn insert_concurrent_task(ctx: &ReducerContext, agent_id: u64, task_type: String) {
    let now = ctx.timestamp.to_micros_since_unix_epoch() as u64;
    let color = task_type_to_color(&task_type).to_string();
    let _ = ctx.db.concurrent_task().insert(ConcurrentTask {
        id: 0,
        agent_id,
        task_type,
        status: "running".to_string(),
        color,
        created_at: now,
    });
}

#[spacetimedb::reducer]
pub fn complete_task(ctx: &ReducerContext, task_id: u64) {
    if let Some(task) = ctx.db.concurrent_task().id().find(task_id) {
        let mut updated = task;
        updated.status = "success".to_string();
        let _ = ctx.db.concurrent_task().id().update(updated);
    }
}

#[spacetimedb::reducer]
pub fn seed_demo_data(ctx: &ReducerContext) {
    let now = ctx.timestamp.to_micros_since_unix_epoch() as u64;

    // One demo project
    let _ = ctx.db.project().insert(Project {
        id: 0,
        name: "synapse-demo".to_string(),
        status: "active".to_string(),
        repository_path: "/Users/dev/synapse-demo".to_string(),
        created_at: now,
    });

    // 3 agents
    let a1 = ctx.db.agent().insert(Agent {
        id: 0,
        name: "Cody".to_string(),
        specialty: "Full-stack".to_string(),
        avatar_seed: "cody-42".to_string(),
        last_seen: now,
    });
    let a2 = ctx.db.agent().insert(Agent {
        id: 0,
        name: "Devon".to_string(),
        specialty: "Backend".to_string(),
        avatar_seed: "devon-7".to_string(),
        last_seen: now,
    });
    let a3 = ctx.db.agent().insert(Agent {
        id: 0,
        name: "Sam".to_string(),
        specialty: "DevOps".to_string(),
        avatar_seed: "sam-13".to_string(),
        last_seen: now,
    });

    let project_id = 1u64; // first project

    // 5 action cards
    let _ = ctx.db.action_card().insert(ActionCard {
        id: 0,
        agent_id: a1.id,
        project_id,
        status: "running".to_string(),
        visual_type: "CodeDiff".to_string(),
        content: "```diff\n- const x = 1;\n+ const x = 2;\n```".to_string(),
        task_summary: "Bump config constant".to_string(),
        priority: 2,
        created_at: now,
        updated_at: now,
    });
    let _ = ctx.db.action_card().insert(ActionCard {
        id: 0,
        agent_id: a2.id,
        project_id,
        status: "thinking".to_string(),
        visual_type: "TerminalOutput".to_string(),
        content: "$ cargo test\n   Compiling synapse-backend...".to_string(),
        task_summary: "Run test suite".to_string(),
        priority: 1,
        created_at: now,
        updated_at: now,
    });
    let _ = ctx.db.action_card().insert(ActionCard {
        id: 0,
        agent_id: a1.id,
        project_id,
        status: "queued".to_string(),
        visual_type: "StatusUpdate".to_string(),
        content: "Linting complete. 0 errors.".to_string(),
        task_summary: "Lint check".to_string(),
        priority: 0,
        created_at: now,
        updated_at: now,
    });
    let _ = ctx.db.action_card().insert(ActionCard {
        id: 0,
        agent_id: a3.id,
        project_id,
        status: "success".to_string(),
        visual_type: "CodeDiff".to_string(),
        content: "Dockerfile updated for multi-stage build.".to_string(),
        task_summary: "Optimize Docker build".to_string(),
        priority: 3,
        created_at: now,
        updated_at: now,
    });
    let _ = ctx.db.action_card().insert(ActionCard {
        id: 0,
        agent_id: a2.id,
        project_id,
        status: "blocked".to_string(),
        visual_type: "TerminalOutput".to_string(),
        content: "Waiting for database connection...".to_string(),
        task_summary: "Migration script".to_string(),
        priority: 1,
        created_at: now,
        updated_at: now,
    });
}
