// src/cli/quality.rs
//
// `acs quality` subcommand — quality scoring and North Star checks.

use anyhow::Result;
use clap::Subcommand;

use crate::db::Db;
use crate::quality::{
    check_north_star, compute_score, format_north_star_report, format_quality_scores_table,
    notes_contain_ac_verification, score_ticket_from_branch,
};

#[derive(Subcommand)]
pub enum QualityCommands {
    /// Compute and display the quality score for a ticket (or all tickets).
    Score {
        /// Ticket ID to score. If omitted, score all completed tickets.
        #[arg(long)]
        ticket: Option<String>,
        /// Score all completed tickets.
        #[arg(long, default_value = "false")]
        all: bool,
    },
    /// Check North Star completion metrics for the project.
    Check,
    /// Show all stored quality scores.
    List,
}

pub fn execute(cmd: QualityCommands) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;

    match cmd {
        QualityCommands::Score { ticket, all } => {
            if let Some(id) = ticket {
                score_one(&db, &id)?;
            } else if all {
                score_all(&db)?;
            } else {
                anyhow::bail!("Specify --ticket <id> or --all");
            }
        }
        QualityCommands::Check => {
            let status = check_north_star(&db, &cwd)?;
            let report = format_north_star_report(&status);
            println!("{}", report);
            let out = serde_json::to_string_pretty(&status)?;
            println!("{}", out);
        }
        QualityCommands::List => {
            let scores = db.list_quality_scores()?;
            let table = format_quality_scores_table(&scores);
            println!("{}", table);
        }
    }

    Ok(())
}

fn score_one(db: &Db, ticket_id: &str) -> Result<()> {
    let ticket = db
        .get_ticket(ticket_id)?
        .ok_or_else(|| anyhow::anyhow!("ticket '{}' not found", ticket_id))?;

    // Derive branch name from ticket — ACS convention: acs/<ticket-id>-<hash>
    // We look for an exact branch match or just pass None.
    let branch = find_branch_for_ticket(ticket_id);
    let notes = &ticket.notes;
    let score = score_ticket_from_branch(db, ticket_id, branch.as_deref(), notes)?;

    let out = serde_json::to_string_pretty(&score)?;
    println!("{}", out);
    Ok(())
}

fn score_all(db: &Db) -> Result<()> {
    let tickets = db.list_tickets(Some("completed"))?;

    if tickets.is_empty() {
        println!("No completed tickets to score.");
        return Ok(());
    }

    for ticket in &tickets {
        let branch = find_branch_for_ticket(&ticket.id);
        let _score =
            score_ticket_from_branch(db, &ticket.id, branch.as_deref(), &ticket.notes)?;
    }

    let scores = db.list_quality_scores()?;
    let table = format_quality_scores_table(&scores);
    println!("{}", table);
    Ok(())
}

/// Try to find the git branch name for a given ticket ID.
/// ACS branches follow the pattern: `acs/<ticket-id>-<hash>`.
fn find_branch_for_ticket(ticket_id: &str) -> Option<String> {
    use std::process::Command;

    let output = Command::new("git")
        .args(["branch", "--list", &format!("acs/{}*", ticket_id)])
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .map(|l| l.trim().trim_start_matches('*').trim().to_string())
        .find(|l| !l.is_empty())
}
