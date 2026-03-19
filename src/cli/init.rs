// src/cli/init.rs
use anyhow::{Context, Result};
use std::fs;

use crate::config::Config;
use crate::db::Db;
use crate::prompts;
use crate::spawner::Spawner;

pub fn execute(spec: Option<String>, auto: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = cwd.join(".acs");

    if acs_dir.exists() {
        anyhow::bail!(".acs/ already exists. Use `acs run` to start agents.");
    }

    // Create directories
    fs::create_dir_all(acs_dir.join("logs"))?;

    // Detect project name from directory
    let project_name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    // Write config
    let config = Config::default_for(project_name);
    fs::write(acs_dir.join("config.toml"), config.to_toml())?;

    // Create database
    let db = Db::open(&acs_dir.join("project.db"))?;

    println!("Initialized ACS in .acs/");

    // Read spec if provided
    let spec_text = if let Some(ref spec_path) = spec {
        Some(fs::read_to_string(spec_path).context("Failed to read spec file")?)
    } else {
        None
    };

    if auto || spec.is_some() {
        println!("Bootstrapping project...");

        let tool_path = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "acs".to_string());

        let system_prompt = prompts::bootstrap_prompt(
            &cwd.to_string_lossy(),
            spec_text.as_deref(),
            &tool_path,
        );

        let task_prompt = format!(
            "Analyze the repository at {} and create tickets for all work needed. \
             Use the Bash tool to run `{}` commands as described in your system prompt. \
             IMPORTANT: Always use the Bash tool to call acs commands. Do not try MCP tools.",
            cwd.display(),
            tool_path,
        );

        let spawner = Spawner::new(&cwd, &config.agents.claude_path, &tool_path);
        let mut child = spawner.spawn_claude("bootstrap", &cwd, &task_prompt, &system_prompt)?;

        let status = child.wait()?;

        // Count tickets
        let tickets = db.list_tickets(None)?;
        let count = tickets.len();

        if status.success() {
            println!("Bootstrap complete! Created {} tickets.", count);
        } else {
            println!(
                "Bootstrap exited with code {:?}. Created {} tickets.",
                status.code(),
                count
            );
        }

        db.log_event(
            Some("bootstrap"),
            "bootstrap_complete",
            &format!("Created {} tickets", count),
            None,
        )?;

        // Generate bootstrap summary report.
        if let Err(e) = crate::cli::report::generate_bootstrap_report(&acs_dir, &db) {
            eprintln!("[report] warning: failed to generate bootstrap report: {:#}", e);
        }
    } else {
        println!("Run `acs init --auto` to auto-analyze, or `acs init --spec <file>` to bootstrap from a spec.");
    }

    println!("Run `acs run` to start the AI team.");
    Ok(())
}
