use spacetimedb::{reducer, table, ReducerContext, Table};
use spacetimedb::log;

#[table(accessor = project, public)]
pub struct Project {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub name: String,
    pub status: String,
    pub repository_path: String,
    pub created_at: u64,
}

#[table(accessor = agent, public)]
pub struct Agent {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub name: String,
    pub specialty: String,
    pub avatar_seed: String,
    pub last_seen: u64,
}

#[table(accessor = action_card, public)]
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

#[table(accessor = feedback, public)]
pub struct Feedback {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub card_id: u64,
    pub action_type: String,
    pub payload: String,
    pub created_at: u64,
}

#[table(accessor = concurrent_task, public)]
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

fn now_micros(ctx: &ReducerContext) -> u64 {
    ctx.timestamp
        .to_duration_since_unix_epoch()
        .unwrap_or_default()
        .as_micros() as u64
}

fn color_for_task_type(task_type: &str) -> String {
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
    .to_string()
}

#[reducer]
pub fn create_agent(
    ctx: &ReducerContext,
    name: String,
    specialty: String,
    avatar_seed: String,
) -> Result<(), String> {
    let timestamp = now_micros(ctx);

    ctx.db.agent().insert(Agent {
        id: 0,
        name,
        specialty,
        avatar_seed,
        last_seen: timestamp,
    });

    Ok(())
}

#[reducer]
pub fn insert_action_card(
    ctx: &ReducerContext,
    agent_id: u64,
    project_id: u64,
    visual_type: String,
    content: String,
    task_summary: String,
    priority: u32,
) -> Result<(), String> {
    if ctx.db.agent().id().find(&agent_id).is_none() {
        return Err("Agent not found".to_string());
    }

    if ctx.db.project().id().find(&project_id).is_none() {
        return Err("Project not found".to_string());
    }

    let timestamp = now_micros(ctx);

    ctx.db.action_card().insert(ActionCard {
        id: 0,
        agent_id,
        project_id,
        status: "queued".to_string(),
        visual_type,
        content,
        task_summary,
        priority,
        created_at: timestamp,
        updated_at: timestamp,
    });

    Ok(())
}

#[reducer]
pub fn approve_action(ctx: &ReducerContext, card_id: u64) -> Result<(), String> {
    let card = ctx
        .db
        .action_card()
        .id()
        .find(&card_id)
        .ok_or_else(|| "Action card not found".to_string())?;

    let timestamp = now_micros(ctx);

    ctx.db.action_card().id().update(ActionCard {
        status: "success".to_string(),
        updated_at: timestamp,
        ..card
    });

    ctx.db.feedback().insert(Feedback {
        id: 0,
        card_id,
        action_type: "approve".to_string(),
        payload: String::new(),
        created_at: timestamp,
    });

    Ok(())
}

#[reducer]
pub fn reject_action(
    ctx: &ReducerContext,
    card_id: u64,
    reason: String,
) -> Result<(), String> {
    let card = ctx
        .db
        .action_card()
        .id()
        .find(&card_id)
        .ok_or_else(|| "Action card not found".to_string())?;

    let timestamp = now_micros(ctx);

    ctx.db.action_card().id().update(ActionCard {
        status: "failed".to_string(),
        updated_at: timestamp,
        ..card
    });

    ctx.db.feedback().insert(Feedback {
        id: 0,
        card_id,
        action_type: "reject".to_string(),
        payload: reason,
        created_at: timestamp,
    });

    Ok(())
}

#[reducer]
pub fn add_comment(
    ctx: &ReducerContext,
    card_id: u64,
    comment: String,
) -> Result<(), String> {
    if ctx
        .db
        .action_card()
        .id()
        .find(&card_id)
        .is_none()
    {
        return Err("Action card not found".to_string());
    }

    let timestamp = now_micros(ctx);

    ctx.db.feedback().insert(Feedback {
        id: 0,
        card_id,
        action_type: "comment".to_string(),
        payload: comment,
        created_at: timestamp,
    });

    Ok(())
}

#[reducer]
pub fn escalate_action(
    ctx: &ReducerContext,
    card_id: u64,
    reason: String,
) -> Result<(), String> {
    let card = ctx
        .db
        .action_card()
        .id()
        .find(&card_id)
        .ok_or_else(|| "Action card not found".to_string())?;

    let timestamp = now_micros(ctx);

    ctx.db.action_card().id().update(ActionCard {
        status: "blocked".to_string(),
        updated_at: timestamp,
        ..card
    });

    ctx.db.feedback().insert(Feedback {
        id: 0,
        card_id,
        action_type: "escalate".to_string(),
        payload: reason,
        created_at: timestamp,
    });

    Ok(())
}

#[reducer]
pub fn update_agent_status(
    ctx: &ReducerContext,
    agent_id: u64,
    specialty: String,
    avatar_seed: String,
) -> Result<(), String> {
    let agent = ctx
        .db
        .agent()
        .id()
        .find(&agent_id)
        .ok_or_else(|| "Agent not found".to_string())?;

    let timestamp = now_micros(ctx);

    ctx.db.agent().id().update(Agent {
        specialty,
        avatar_seed,
        last_seen: timestamp,
        ..agent
    });

    Ok(())
}

#[reducer]
pub fn insert_concurrent_task(
    ctx: &ReducerContext,
    agent_id: u64,
    task_type: String,
    status: String,
) -> Result<(), String> {
    if ctx.db.agent().id().find(&agent_id).is_none() {
        return Err("Agent not found".to_string());
    }

    let timestamp = now_micros(ctx);
    let color = color_for_task_type(&task_type);

    ctx.db.concurrent_task().insert(ConcurrentTask {
        id: 0,
        agent_id,
        task_type,
        status,
        color,
        created_at: timestamp,
    });

    Ok(())
}

#[reducer]
pub fn complete_task(ctx: &ReducerContext, task_id: u64) -> Result<(), String> {
    let task = ctx
        .db
        .concurrent_task()
        .id()
        .find(&task_id)
        .ok_or_else(|| "Concurrent task not found".to_string())?;

    ctx.db.concurrent_task().id().update(ConcurrentTask {
        status: "completed".to_string(),
        ..task
    });

    Ok(())
}

#[reducer]
pub fn seed_demo_data(ctx: &ReducerContext) -> Result<(), String> {
    // Avoid reseeding if data already exists.
    if ctx.db.agent().iter().next().is_some() {
        log::info!("seed_demo_data: agents already exist, skipping seed.");
        return Ok(());
    }

    let timestamp = now_micros(ctx);

    // Create a demo project for Synapse.
    let project = ctx.db.project().insert(Project {
        id: 0,
        name: "Synapse".to_string(),
        status: "active".to_string(),
        repository_path: "/Users/mkhare/Development/devtok".to_string(),
        created_at: timestamp,
    });

    // Create demo agents.
    let agent_alice = ctx.db.agent().insert(Agent {
        id: 0,
        name: "Alice".to_string(),
        specialty: "TypeScript & React".to_string(),
        avatar_seed: "alice-dev".to_string(),
        last_seen: timestamp,
    });

    let agent_bob = ctx.db.agent().insert(Agent {
        id: 0,
        name: "Bob".to_string(),
        specialty: "Rust backend".to_string(),
        avatar_seed: "bob-rs".to_string(),
        last_seen: timestamp,
    });

    let agent_cara = ctx.db.agent().insert(Agent {
        id: 0,
        name: "Cara".to_string(),
        specialty: "CI/CD & testing".to_string(),
        avatar_seed: "cara-ci".to_string(),
        last_seen: timestamp,
    });

    // Create demo action cards.
    let _card1 = ctx.db.action_card().insert(ActionCard {
        id: 0,
        agent_id: agent_alice.id,
        project_id: project.id,
        status: "queued".to_string(),
        visual_type: "CodeDiff".to_string(),
        content: "Refactor `ActionCard` component props to use a discriminated union.".to_string(),
        task_summary: "Refactor Synapse card props".to_string(),
        priority: 1,
        created_at: timestamp,
        updated_at: timestamp,
    });

    let _card2 = ctx.db.action_card().insert(ActionCard {
        id: 0,
        agent_id: agent_bob.id,
        project_id: project.id,
        status: "running".to_string(),
        visual_type: "TerminalOutput".to_string(),
        content: "Running `spacetime build` for synapse-backend...".to_string(),
        task_summary: "Build SpacetimeDB module".to_string(),
        priority: 2,
        created_at: timestamp,
        updated_at: timestamp,
    });

    let _card3 = ctx.db.action_card().insert(ActionCard {
        id: 0,
        agent_id: agent_cara.id,
        project_id: project.id,
        status: "queued".to_string(),
        visual_type: "StatusUpdate".to_string(),
        content: "Set up GitHub Actions workflow for Synapse frontend.".to_string(),
        task_summary: "Add CI pipeline for frontend".to_string(),
        priority: 3,
        created_at: timestamp,
        updated_at: timestamp,
    });

    let _card4 = ctx.db.action_card().insert(ActionCard {
        id: 0,
        agent_id: agent_alice.id,
        project_id: project.id,
        status: "blocked".to_string(),
        visual_type: "StatusUpdate".to_string(),
        content: "Waiting for design approval on swipe interactions.".to_string(),
        task_summary: "Finalize swipe UX".to_string(),
        priority: 2,
        created_at: timestamp,
        updated_at: timestamp,
    });

    let _card5 = ctx.db.action_card().insert(ActionCard {
        id: 0,
        agent_id: agent_bob.id,
        project_id: project.id,
        status: "queued".to_string(),
        visual_type: "CodeDiff".to_string(),
        content: "Introduce `ConcurrentTask` pills in the card header.".to_string(),
        task_summary: "Display concurrent agent tasks".to_string(),
        priority: 1,
        created_at: timestamp,
        updated_at: timestamp,
    });

    Ok(())
}

