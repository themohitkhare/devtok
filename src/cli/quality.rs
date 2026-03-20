// src/cli/quality.rs
//
// `acs quality` subcommand — quality scoring and North Star checks.

use anyhow::Result;
use clap::Subcommand;

use crate::db::Db;
use crate::quality::{
    check_north_star, detect_changes_from_scoring_ref, format_north_star_report,
    format_quality_scores_table, resolve_scoring_ref, score_ticket_from_branch,
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
    /// Retroactively score all completed tickets (backfill). Equivalent to `score --all`.
    Backfill,
    /// Check North Star completion metrics for the project.
    Check,
    /// Show all stored quality scores.
    List,
}

pub fn execute(cmd: QualityCommands) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;
    execute_with_db(&db, &cwd, cmd)
}

fn execute_with_db(db: &Db, cwd: &std::path::Path, cmd: QualityCommands) -> Result<()> {
    match cmd {
        QualityCommands::Score { ticket, all } => {
            if let Some(id) = ticket {
                score_one(db, cwd, &id)?;
            } else if all {
                score_all(db, cwd)?;
            } else {
                anyhow::bail!("Specify --ticket <id> or --all");
            }
        }
        QualityCommands::Backfill => {
            score_all(db, cwd)?;
        }
        QualityCommands::Check => {
            let status = check_north_star(db, cwd)?;
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

fn score_one(db: &Db, cwd: &std::path::Path, ticket_id: &str) -> Result<()> {
    let ticket = db
        .get_ticket(ticket_id)?
        .ok_or_else(|| anyhow::anyhow!("ticket '{}' not found", ticket_id))?;
    let score = score_ticket_via_ref(db, cwd, ticket_id, &ticket.notes)?;
    let out = serde_json::to_string_pretty(&score)?;
    println!("{}", out);
    Ok(())
}

fn score_all(db: &Db, cwd: &std::path::Path) -> Result<()> {
    let tickets = db.list_tickets(Some("completed"))?;
    if tickets.is_empty() {
        println!("No completed tickets to score.");
        return Ok(());
    }
    for ticket in &tickets {
        if let Err(e) = score_ticket_via_ref(db, cwd, &ticket.id, &ticket.notes) {
            eprintln!("[quality] scoring failed for {}: {}", ticket.id, e);
        }
    }
    let scores = db.list_quality_scores()?;
    let table = format_quality_scores_table(&scores);
    println!("{}", table);
    Ok(())
}

/// Score a ticket using the full scoring ref resolution (local branch → remote → merge commit).
fn score_ticket_via_ref(
    db: &Db,
    cwd: &std::path::Path,
    ticket_id: &str,
    ticket_notes: &str,
) -> Result<crate::models::QualityScore> {
    use crate::quality::{compute_score, notes_contain_ac_verification};

    let scoring_ref = resolve_scoring_ref(cwd, ticket_id);
    let (tests_added, docs_updated) = match &scoring_ref {
        Some(r) => detect_changes_from_scoring_ref(cwd, r),
        None => (false, false),
    };
    let acceptance_criteria_met = notes_contain_ac_verification(ticket_notes);
    let score = compute_score(ticket_id, tests_added, docs_updated, acceptance_criteria_met);
    db.upsert_quality_score(&score)?;
    Ok(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn score_requires_ticket_or_all_flag() {
        let db = Db::open_memory().unwrap();
        let cwd = repo_root();
        let err = execute_with_db(
            &db,
            &cwd,
            QualityCommands::Score {
                ticket: None,
                all: false,
            },
        )
        .unwrap_err();
        assert!(format!("{:#}", err).contains("Specify --ticket <id> or --all"));
    }

    #[test]
    fn score_one_errors_for_missing_ticket() {
        let db = Db::open_memory().unwrap();
        let cwd = repo_root();
        let err = score_one(&db, &cwd, "t-404").unwrap_err();
        assert!(format!("{:#}", err).contains("ticket 't-404' not found"));
    }

    #[test]
    fn score_all_handles_no_completed_tickets() {
        let db = Db::open_memory().unwrap();
        let cwd = repo_root();
        score_all(&db, &cwd).unwrap();
    }

    #[test]
    fn find_branch_for_ticket_accepts_no_match() {
        let branch = find_branch_for_ticket("definitely-not-a-real-ticket");
        assert!(branch.is_none());
    }

    #[test]
    fn quality_check_and_list_execute_without_error() {
        let db = Db::open_memory().unwrap();
        let cwd = repo_root();
        execute_with_db(&db, &cwd, QualityCommands::Check).unwrap();
        execute_with_db(&db, &cwd, QualityCommands::List).unwrap();
    }

    #[test]
    fn backfill_handles_no_completed_tickets() {
        let db = Db::open_memory().unwrap();
        let cwd = repo_root();
        execute_with_db(&db, &cwd, QualityCommands::Backfill).unwrap();
    }

    #[test]
    fn backfill_scores_all_completed_tickets() {
        let db = Db::open_memory().unwrap();
        let cwd = repo_root();
        let id = db.create_ticket("Test", "desc", "core", 1).unwrap();
        db.update_ticket(&id, "completed", None, None, None).unwrap();
        execute_with_db(&db, &cwd, QualityCommands::Backfill).unwrap();
        // score should be persisted even if branch doesn't exist (score=0)
        let score = db.get_quality_score(&id).unwrap();
        assert!(score.is_some(), "backfill should persist quality score for completed ticket");
    }
}
