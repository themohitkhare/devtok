use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::models::{pricing, Agent, Ticket};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Full standup report — used for both text output and --json serialization.
#[derive(Debug, Serialize, Deserialize)]
pub struct StandupReport {
    pub date: String,
    pub completed: Vec<TicketSummary>,
    pub in_progress: Vec<InProgressSummary>,
    pub blocked: Vec<BlockedSummary>,
    pub metrics: StandupMetrics,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TicketSummary {
    pub id: String,
    pub title: String,
    pub domain: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InProgressSummary {
    pub id: String,
    pub title: String,
    pub worker: Option<String>,
    pub elapsed_minutes: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockedSummary {
    pub id: String,
    pub title: String,
    pub blocked_by: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StandupMetrics {
    /// Rolling 7-day velocity in tickets/day.
    pub velocity_7day: f64,
    /// Estimated cost (Sonnet pricing) for today's token usage.
    pub cost_today_usd: f64,
    /// Total estimated cost across all events.
    pub cost_total_usd: f64,
    /// Number of active workers.
    pub active_workers: usize,
    /// Total registered workers.
    pub total_workers: usize,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn execute(json: bool, post_github: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;

    let report = build_report(&db)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    let text = format_report(&report);

    if post_github {
        post_to_github(&text)?;
    } else {
        print!("{}", text);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Report construction
// ---------------------------------------------------------------------------

fn build_report(db: &Db) -> Result<StandupReport> {
    let now = Utc::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let since_24h = (now - Duration::hours(24)).to_rfc3339();
    let day_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
        .unwrap_or(now - Duration::hours(24))
        .to_rfc3339();

    // Completed in last 24h
    let completed_tickets = db.tickets_completed_since(&since_24h)?;
    let completed: Vec<TicketSummary> = completed_tickets
        .into_iter()
        .map(|t| TicketSummary {
            id: t.id,
            title: t.title,
            domain: t.domain,
        })
        .collect();

    // In progress
    let in_progress_tickets = db.list_tickets(Some("in_progress"))?;
    let agents = db.list_agents()?;
    let in_progress: Vec<InProgressSummary> = in_progress_tickets
        .into_iter()
        .map(|t| {
            let worker = find_worker_for_ticket(&agents, &t);
            let elapsed = elapsed_minutes(db, &t);
            InProgressSummary {
                id: t.id,
                title: t.title,
                worker,
                elapsed_minutes: elapsed,
            }
        })
        .collect();

    // Blocked
    let blocked_tickets = db.list_tickets(Some("blocked"))?;
    let blocked: Vec<BlockedSummary> = blocked_tickets
        .into_iter()
        .map(|t| BlockedSummary {
            id: t.id.clone(),
            title: t.title,
            blocked_by: t.blocked_by,
        })
        .collect();

    // Metrics
    let velocity = db.velocity_7day()?;

    let (today_in, today_out) = db.token_details_since(&day_start)?;
    let cost_today = pricing::estimate_cost(
        today_in,
        today_out,
        pricing::SONNET_INPUT_PER_M,
        pricing::SONNET_OUTPUT_PER_M,
    );

    let (total_in, total_out) = db.total_token_details()?;
    let cost_total = pricing::estimate_cost(
        total_in,
        total_out,
        pricing::SONNET_INPUT_PER_M,
        pricing::SONNET_OUTPUT_PER_M,
    );

    let active_workers = agents.iter().filter(|a| a.status == "active").count();
    let total_workers = agents.len();

    let metrics = StandupMetrics {
        velocity_7day: velocity,
        cost_today_usd: cost_today,
        cost_total_usd: cost_total,
        active_workers,
        total_workers,
    };

    Ok(StandupReport {
        date: date_str,
        completed,
        in_progress,
        blocked,
        metrics,
    })
}

fn find_worker_for_ticket(agents: &[Agent], ticket: &Ticket) -> Option<String> {
    ticket.assignee.as_ref().and_then(|assignee| {
        agents
            .iter()
            .find(|a| &a.id == assignee)
            .map(|a| a.id.clone())
    })
}

fn elapsed_minutes(db: &Db, ticket: &Ticket) -> Option<i64> {
    // Try to find when this ticket was last assigned via events.
    // Fall back to ticket.updated_at if no assignment event found.
    let assigned_at = db.ticket_assigned_at(&ticket.id).ok().flatten()
        .or_else(|| Some(ticket.updated_at.clone()))?;

    let assigned_dt = DateTime::parse_from_rfc3339(&assigned_at)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()?;

    let elapsed = Utc::now() - assigned_dt;
    Some(elapsed.num_minutes())
}

// ---------------------------------------------------------------------------
// Text formatting
// ---------------------------------------------------------------------------

fn format_report(r: &StandupReport) -> String {
    let mut out = String::new();

    out.push_str(&format!("--- ACS Daily Standup ({}) ---\n", r.date));

    // Completed
    out.push_str(&format!(
        "\n✅ Completed yesterday ({}):\n",
        r.completed.len()
    ));
    if r.completed.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for t in &r.completed {
            out.push_str(&format!("  - {}: {} ({})\n", t.id, t.title, t.domain));
        }
    }

    // In progress
    out.push_str(&format!(
        "\n🔄 In progress today ({}):\n",
        r.in_progress.len()
    ));
    if r.in_progress.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for t in &r.in_progress {
            let worker_str = t
                .worker
                .as_deref()
                .map(|w| format!("{}, ", w))
                .unwrap_or_default();
            let elapsed_str = t
                .elapsed_minutes
                .map(|m| format_elapsed(m))
                .unwrap_or_else(|| "?".to_string());
            out.push_str(&format!(
                "  - {}: {} [{}{}]\n",
                t.id, t.title, worker_str, elapsed_str
            ));
        }
    }

    // Blocked
    out.push_str(&format!("\n🚧 Blocked ({}):\n", r.blocked.len()));
    if r.blocked.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for t in &r.blocked {
            let blocked_by_str = t
                .blocked_by
                .as_deref()
                .map(|b| format!(" blocked by {}", b))
                .unwrap_or_default();
            out.push_str(&format!("  - {}:{}\n", t.id, blocked_by_str));
        }
    }

    // Metrics
    out.push_str("\n📊 Metrics:\n");
    out.push_str(&format!(
        "  Velocity: {:.1} tickets/day (7-day avg)\n",
        r.metrics.velocity_7day
    ));
    out.push_str(&format!(
        "  Cost today: ${:.2} | Total: ${:.2}\n",
        r.metrics.cost_today_usd, r.metrics.cost_total_usd
    ));
    out.push_str(&format!(
        "  Active workers: {}/{}\n",
        r.metrics.active_workers, r.metrics.total_workers
    ));

    out
}

fn format_elapsed(minutes: i64) -> String {
    if minutes < 0 {
        return "?".to_string();
    }
    if minutes < 60 {
        format!("{}m elapsed", minutes)
    } else {
        let hours = minutes / 60;
        let mins = minutes % 60;
        if mins == 0 {
            format!("{}h elapsed", hours)
        } else {
            format!("{}h{}m elapsed", hours, mins)
        }
    }
}

// ---------------------------------------------------------------------------
// GitHub posting
// ---------------------------------------------------------------------------

fn post_to_github(text: &str) -> Result<()> {
    use std::process::Command;

    // Check if gh CLI is available
    let check = Command::new("gh").arg("--version").output();
    if check.is_err() {
        anyhow::bail!("gh CLI not found; cannot post to GitHub. Install from https://cli.github.com/");
    }

    // Get the current repo's default issue (or create a standup issue)
    // We post as a comment to an existing open issue tagged 'standup',
    // or create a new issue if none exists.
    let issue_list = Command::new("gh")
        .args(["issue", "list", "--label", "standup", "--state", "open", "--json", "number", "--limit", "1"])
        .output()?;

    let issue_num: Option<u64> = if issue_list.status.success() {
        let json_str = String::from_utf8_lossy(&issue_list.stdout);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap_or(serde_json::Value::Array(vec![]));
        parsed.as_array()
            .and_then(|arr| arr.first())
            .and_then(|obj| obj.get("number"))
            .and_then(|n| n.as_u64())
    } else {
        None
    };

    if let Some(num) = issue_num {
        // Post as comment on existing standup issue
        let output = Command::new("gh")
            .args(["issue", "comment", &num.to_string(), "--body", text])
            .output()?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("gh issue comment failed: {}", err);
        }
        eprintln!("[standup] posted comment to issue #{}", num);
    } else {
        // Create a new standup issue
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let title = format!("ACS Daily Standup — {}", today);
        let output = Command::new("gh")
            .args(["issue", "create", "--title", &title, "--body", text, "--label", "standup"])
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Label might not exist; retry without label
            let output2 = Command::new("gh")
                .args(["issue", "create", "--title", &title, "--body", text])
                .output()?;
            if !output2.status.success() {
                anyhow::bail!("gh issue create failed: {}", stderr);
            }
            let url = String::from_utf8_lossy(&output2.stdout).trim().to_string();
            eprintln!("[standup] created issue: {}", url);
        } else {
            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            eprintln!("[standup] created issue: {}", url);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_elapsed_minutes() {
        assert_eq!(format_elapsed(0), "0m elapsed");
        assert_eq!(format_elapsed(45), "45m elapsed");
        assert_eq!(format_elapsed(60), "1h elapsed");
        assert_eq!(format_elapsed(90), "1h30m elapsed");
        assert_eq!(format_elapsed(120), "2h elapsed");
        assert_eq!(format_elapsed(-1), "?");
    }

    #[test]
    fn format_report_structure() {
        let report = StandupReport {
            date: "2026-03-20".to_string(),
            completed: vec![TicketSummary {
                id: "t-001".to_string(),
                title: "Test ticket".to_string(),
                domain: "core".to_string(),
            }],
            in_progress: vec![InProgressSummary {
                id: "t-002".to_string(),
                title: "WIP ticket".to_string(),
                worker: Some("w-1".to_string()),
                elapsed_minutes: Some(45),
            }],
            blocked: vec![BlockedSummary {
                id: "t-003".to_string(),
                title: "Blocked ticket".to_string(),
                blocked_by: Some("t-001".to_string()),
            }],
            metrics: StandupMetrics {
                velocity_7day: 2.5,
                cost_today_usd: 1.23,
                cost_total_usd: 45.67,
                active_workers: 3,
                total_workers: 5,
            },
        };

        let text = format_report(&report);
        assert!(text.contains("ACS Daily Standup (2026-03-20)"));
        assert!(text.contains("✅ Completed yesterday (1)"));
        assert!(text.contains("t-001: Test ticket (core)"));
        assert!(text.contains("🔄 In progress today (1)"));
        assert!(text.contains("t-002: WIP ticket [w-1, 45m elapsed]"));
        assert!(text.contains("🚧 Blocked (1)"));
        assert!(text.contains("t-003: blocked by t-001"));
        assert!(text.contains("📊 Metrics:"));
        assert!(text.contains("Velocity: 2.5 tickets/day (7-day avg)"));
        assert!(text.contains("Cost today: $1.23 | Total: $45.67"));
        assert!(text.contains("Active workers: 3/5"));
    }

    #[test]
    fn json_serialization_roundtrip() {
        let report = StandupReport {
            date: "2026-03-20".to_string(),
            completed: vec![],
            in_progress: vec![],
            blocked: vec![],
            metrics: StandupMetrics {
                velocity_7day: 1.0,
                cost_today_usd: 0.0,
                cost_total_usd: 0.0,
                active_workers: 0,
                total_workers: 0,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: StandupReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.date, "2026-03-20");
        assert_eq!(parsed.metrics.velocity_7day, 1.0);
    }

    #[test]
    fn db_velocity_7day_empty() {
        let db = Db::open_memory().unwrap();
        let v = db.velocity_7day().unwrap();
        assert_eq!(v, 0.0);
    }

    #[test]
    fn db_tickets_completed_since_empty() {
        let db = Db::open_memory().unwrap();
        let since = (Utc::now() - Duration::hours(24)).to_rfc3339();
        let tickets = db.tickets_completed_since(&since).unwrap();
        assert!(tickets.is_empty());
    }

    #[test]
    fn db_token_details_since_empty() {
        let db = Db::open_memory().unwrap();
        let since = (Utc::now() - Duration::hours(24)).to_rfc3339();
        let (input, output) = db.token_details_since(&since).unwrap();
        assert_eq!(input, 0);
        assert_eq!(output, 0);
    }
}
