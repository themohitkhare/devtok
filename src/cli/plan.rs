// src/cli/plan.rs
use anyhow::{bail, Result};

use crate::config::Config;
use crate::db::Db;
use crate::prompts;
use crate::spawner::Spawner;

pub fn execute() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;

    let config = Config::load(&acs_dir.join("config.toml"))?;
    let db = Db::open(&acs_dir.join("project.db"))?;

    // Verify there are tickets to plan around
    let tickets = db.list_tickets(None)?;
    if tickets.is_empty() {
        bail!("No tickets found. Run `acs init --auto` or create tickets first.");
    }

    println!("Starting Solution Architect agent...");
    println!("Found {} tickets to plan around.", tickets.len());

    let tool_path = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "acs".to_string());

    let system_prompt = prompts::architect_prompt(
        &cwd.to_string_lossy(),
        &tool_path,
    );

    let task_prompt = format!(
        "Analyze the tickets and knowledge base for the project at {}, then create a comprehensive \
         architecture plan. Group tickets into milestones, write ADRs for key decisions, and define \
         API contracts between domains. \
         Use the Bash tool to run `{}` commands as described in your system prompt. \
         IMPORTANT: Always use the Bash tool to call acs commands. Do not try MCP tools.",
        cwd.display(),
        tool_path,
    );

    let spawner = Spawner::new(&cwd, &config.agents.claude_path, &tool_path);
    let mut child = spawner.spawn_claude("architect", &cwd, &task_prompt, &system_prompt)?;

    let status = child.wait()?;

    // Check what was produced
    let has_plan = db.read_knowledge("architecture", "milestone-plan")?.is_some();
    let has_contracts = db.read_knowledge("architecture", "api-contracts")?.is_some();

    if status.success() {
        println!("Architecture planning complete!");
        if has_plan {
            println!("  - Milestone plan stored in KB (architecture/milestone-plan)");
        }
        if has_contracts {
            println!("  - API contracts stored in KB (architecture/api-contracts)");
        }
    } else {
        println!(
            "Architect agent exited with code {:?}.",
            status.code(),
        );
    }

    db.log_event(
        Some("architect"),
        "planning_complete",
        &format!(
            "milestone_plan={}, api_contracts={}",
            has_plan, has_contracts
        ),
        None,
    )?;

    println!("Run `acs run` to start the AI team.");
    Ok(())
}
