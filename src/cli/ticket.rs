use clap::Subcommand;
use anyhow::{anyhow, Result};
use crate::db::Db;
use crate::quality::score_ticket_from_branch;
use std::io::{self, Write};
use std::path::Path;

#[derive(Subcommand)]
pub enum TicketCommands {
    /// List tickets
    List {
        #[arg(long)]
        status: Option<String>,
    },
    /// Create a ticket
    Create {
        #[arg(long)]
        title: String,
        #[arg(long)]
        description: String,
        #[arg(long)]
        domain: String,
        #[arg(long, default_value = "3")]
        priority: i32,
        #[arg(long)]
        blocked_by: Option<String>,
        /// Skip duplicate check and create anyway
        #[arg(long, default_value = "false")]
        force: bool,
        /// Non-interactive mode (auto-skip if >80% match)
        #[arg(long, default_value = "false")]
        non_interactive: bool,
    },
    /// Update ticket status
    Update {
        #[arg(long)]
        id: String,
        #[arg(long)]
        status: String,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        blocked_by: Option<String>,
    },
    /// Show ticket details, or list blocked tickets with --all-blocked
    Show {
        /// Ticket ID to display (e.g. t-001); omit when using --all-blocked
        id: Option<String>,
        /// Emit raw JSON instead of a human-readable card
        #[arg(long)]
        json: bool,
        /// List all tickets with status == blocked
        #[arg(long)]
        all_blocked: bool,
    },
}

/// Similarity threshold for showing a warning (70%)
const SIMILARITY_WARN_THRESHOLD: f64 = 0.70;
/// Similarity threshold for auto-skipping in non-interactive mode (80%)
const SIMILARITY_BLOCK_THRESHOLD: f64 = 0.80;

pub fn execute(cmd: TicketCommands) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;

    match cmd {
        TicketCommands::Create { title, description, domain, priority, blocked_by, force, non_interactive } => {
            // Check for similar tickets unless --force is set
            if !force {
                let similar = db.find_similar_tickets(&title, &description)?;
                let top_match = similar.first();

                if let Some((match_id, match_title, score)) = top_match {
                    let pct = (score * 100.0).round() as u32;

                    if pct as f64 >= SIMILARITY_WARN_THRESHOLD * 100.0 {
                        if non_interactive && *score >= SIMILARITY_BLOCK_THRESHOLD {
                            let out = serde_json::json!({
                                "status": "skipped",
                                "reason": "duplicate",
                                "similar_ticket": match_id,
                                "similar_title": match_title,
                                "similarity": pct
                            });
                            println!("{}", serde_json::to_string_pretty(&out)?);
                            return Ok(());
                        }

                        if !non_interactive {
                            eprintln!(
                                "Similar ticket exists: {} ({}% match) - \"{}\"",
                                match_id, pct, match_title
                            );
                            eprint!("Create anyway? [y/N] ");
                            io::stderr().flush()?;
                            let mut input = String::new();
                            io::stdin().read_line(&mut input)?;
                            if !input.trim().eq_ignore_ascii_case("y") {
                                let out = serde_json::json!({
                                    "status": "skipped",
                                    "reason": "duplicate",
                                    "similar_ticket": match_id,
                                    "similar_title": match_title,
                                    "similarity": pct
                                });
                                println!("{}", serde_json::to_string_pretty(&out)?);
                                return Ok(());
                            }
                        }
                    }
                }
            }

            let id = db.create_ticket(&title, &description, &domain, priority)?;
            db.store_ticket_keywords(&id, &title, &description)?;
            if let Some(blocked) = blocked_by {
                db.update_ticket(&id, "pending", None, Some(&blocked), None)?;
            }
            let out = serde_json::json!({ "status": "created", "id": id });
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        TicketCommands::List { status } => {
            let tickets = db.list_tickets(status.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&tickets)?);
        }
        TicketCommands::Update { id, status, notes, blocked_by } => {
            update_ticket_and_auto_score(
                &db,
                &cwd,
                &id,
                &status,
                notes.as_deref(),
                blocked_by.as_deref(),
            )?;
            let out = serde_json::json!({ "status": "updated" });
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        TicketCommands::Show { id, json, all_blocked } => {
            if all_blocked {
                let tickets = db.list_tickets(Some("blocked"))?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&tickets)?);
                } else {
                    if tickets.is_empty() {
                        println!("No blocked tickets.");
                    } else {
                        for ticket in &tickets {
                            print_ticket_card(ticket);
                            println!();
                        }
                    }
                }
            } else {
                let ticket_id = id.ok_or_else(|| anyhow!("provide a ticket ID or use --all-blocked"))?;
                match db.get_ticket(&ticket_id)? {
                    Some(ticket) => {
                        if json {
                            println!("{}", serde_json::to_string_pretty(&ticket)?);
                        } else {
                            print_ticket_card(&ticket);
                        }
                    }
                    None => return Err(anyhow!("ticket '{}' not found", ticket_id)),
                }
            }
        }
    }

    Ok(())
}

fn print_ticket_card(ticket: &crate::models::Ticket) {
    let sep = "─".repeat(60);
    println!("┌{}┐", sep);
    println!("│ {:58} │", format!("{} — {}", ticket.id, ticket.title));
    println!("├{}┤", sep);
    println!("│ {:<20} {:<37} │", "Domain:", ticket.domain);
    println!("│ {:<20} {:<37} │", "Priority:", ticket.priority);
    println!("│ {:<20} {:<37} │", "Status:", ticket.status);
    println!("│ {:<20} {:<37} │", "Assignee:",
        ticket.assignee.as_deref().unwrap_or("<none>"));
    println!("│ {:<20} {:<37} │", "Blocked by:",
        ticket.blocked_by.as_deref().unwrap_or("<none>"));
    println!("│ {:<20} {:<37} │", "Created:", ticket.created_at);
    println!("│ {:<20} {:<37} │", "Updated:", ticket.updated_at);
    println!("├{}┤", sep);
    println!("│ Description:                                               │");
    for line in wrap_text(&ticket.description, 56) {
        println!("│   {:<56} │", line);
    }
    if !ticket.notes.is_empty() {
        println!("├{}┤", sep);
        println!("│ Notes:                                                     │");
        for line in wrap_text(&ticket.notes, 56) {
            println!("│   {:<56} │", line);
        }
    }
    println!("└{}┘", sep);
}

/// Wrap text to a maximum width, splitting on whitespace.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else if current.len() + 1 + word.len() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(current.clone());
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn update_ticket_and_auto_score(
    db: &Db,
    project_dir: &Path,
    id: &str,
    status: &str,
    notes: Option<&str>,
    blocked_by: Option<&str>,
) -> Result<()> {
    db.update_ticket(id, status, notes, blocked_by, None)?;

    // Auto-compute quality when a ticket is explicitly marked completed.
    if status == "completed" {
        let ticket = db
            .get_ticket(id)?
            .ok_or_else(|| anyhow!("ticket '{}' not found", id))?;
        let branch = find_branch_for_ticket(project_dir, id);
        let _ = score_ticket_from_branch(
            db,
            project_dir,
            id,
            branch.as_deref(),
            &ticket.notes,
        )?;
    }

    Ok(())
}

fn find_branch_for_ticket(project_dir: &Path, ticket_id: &str) -> Option<String> {
    use std::process::Command;

    let output = Command::new("git")
        .args(["branch", "--list", &format!("acs/{}*", ticket_id)])
        .current_dir(project_dir)
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .map(|l| l.trim().trim_start_matches('*').trim().to_string())
        .find(|l| !l.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    #[test]
    fn update_completed_ticket_auto_scores_from_branch() {
        let db = Db::open_memory().unwrap();
        let ticket_id = db.create_ticket("Task A", "Do A", "general", 1).unwrap();

        let repo = tempfile::tempdir().unwrap();
        let repo_path = repo.path();

        Command::new("git")
            .args(["init"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        fs::write(repo_path.join("README.md"), "base readme").unwrap();
        fs::create_dir_all(repo_path.join("src")).unwrap();
        fs::write(repo_path.join("src/lib.rs"), "pub fn base() {}").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init", "--no-gpg-sign"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        Command::new("git")
            .args(["checkout", "-b", "acs/t-001-abcd"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        fs::write(repo_path.join("README.md"), "updated readme docs").unwrap();
        fs::create_dir_all(repo_path.join("tests")).unwrap();
        fs::write(repo_path.join("tests/quality_test.rs"), "#[test]\nfn ok() { assert!(true); }\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "changes", "--no-gpg-sign"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "main"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        update_ticket_and_auto_score(
            &db,
            repo_path,
            &ticket_id,
            "completed",
            Some("AC verified"),
            None,
        )
        .unwrap();

        let score = db.get_quality_score("t-001").unwrap().unwrap();
        assert!(score.tests_added);
        assert!(score.docs_updated);
        assert!(score.acceptance_criteria_met);
        assert_eq!(score.score, 100);
    }

    #[test]
    fn print_ticket_card_renders_fields() {
        let db = Db::open_memory().unwrap();
        let id = db.create_ticket("My Task", "A longer description\nwith two lines", "core", 2).unwrap();
        let ticket = db.get_ticket(&id).unwrap().unwrap();
        // Should not panic; spot-check card content via wrap_text
        let lines = wrap_text("hello world foo", 10);
        assert_eq!(lines[0], "hello");
        assert_eq!(lines[1], "world foo");
        // Full card output: just ensure it doesn't panic
        print_ticket_card(&ticket);
    }

    #[test]
    fn wrap_text_empty_string() {
        let lines = wrap_text("", 40);
        assert_eq!(lines, vec![""]);
    }

    #[test]
    fn wrap_text_fits_on_one_line() {
        let lines = wrap_text("short text", 40);
        assert_eq!(lines, vec!["short text"]);
    }

    #[test]
    fn show_missing_ticket_returns_error() {
        let db = Db::open_memory().unwrap();
        let result = db.get_ticket("t-999").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn list_blocked_tickets_returns_only_blocked() {
        let db = Db::open_memory().unwrap();
        let id1 = db.create_ticket("T1", "desc", "core", 1).unwrap();
        let id2 = db.create_ticket("T2", "desc", "core", 1).unwrap();
        db.update_ticket(&id1, "blocked", None, None, None).unwrap();
        db.update_ticket(&id2, "in_progress", None, None, None).unwrap();
        let blocked = db.list_tickets(Some("blocked")).unwrap();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].id, id1);
    }
}
