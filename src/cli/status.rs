use crate::db::Db;
use anyhow::Result;
use chrono::Utc;

/// Formats an RFC-3339 timestamp as a human-readable "N minutes ago" string.
fn format_time_ago(ts: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(ts) {
        Ok(dt) => {
            let secs = Utc::now()
                .signed_duration_since(dt.with_timezone(&Utc))
                .num_seconds()
                .max(0);
            if secs < 60 {
                format!("{} seconds ago", secs)
            } else if secs < 3600 {
                format!("{} minutes ago", secs / 60)
            } else if secs < 86400 {
                format!("{} hours ago", secs / 3600)
            } else {
                format!("{} days ago", secs / 86400)
            }
        }
        Err(_) => ts.to_string(),
    }
}

pub fn execute(live: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;
    execute_with_db(&db, live)
}

fn execute_with_db(db: &Db, live: bool) -> Result<()> {
    if live {
        let cwd = std::env::current_dir()?;
        let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
        return crate::cli::status_live::run(db, &acs_dir);
    }

    // Ticket summary
    let counts = db.count_by_status()?;
    let total: i64 = counts.iter().map(|(_, c)| c).sum();
    println!("Tickets: {}", total);
    for (status, count) in &counts {
        println!("  {}: {}", status, count);
    }

    // Last rebuilt
    match db.last_rebuilt_at()? {
        Some(ts) => {
            let ago = format_time_ago(&ts);
            println!("\nLast rebuilt: {}", ago);
        }
        None => {
            println!("\nLast rebuilt: never");
        }
    }

    // Agent status
    let agents = db.list_agents()?;
    if !agents.is_empty() {
        println!("\nAgents:");
        for a in &agents {
            let ticket_info = a.current_ticket.as_deref().unwrap_or("-");
            println!(
                "  {} ({}) [{}] — {} [{}]",
                a.id, a.role, a.backend, a.status, ticket_info
            );
        }
    }

    // Token usage summary with cost estimate
    let (input_tokens, output_tokens) = db.total_token_details()?;
    let total_tokens = input_tokens + output_tokens;
    if total_tokens > 0 {
        use crate::models::pricing;
        let sonnet_cost = pricing::estimate_cost(
            input_tokens,
            output_tokens,
            pricing::SONNET_INPUT_PER_M,
            pricing::SONNET_OUTPUT_PER_M,
        );
        let opus_cost = pricing::estimate_cost(
            input_tokens,
            output_tokens,
            pricing::OPUS_INPUT_PER_M,
            pricing::OPUS_OUTPUT_PER_M,
        );
        println!(
            "\nTokens: {} ({} in / {} out)",
            crate::cli::cost::fmt_tokens(total_tokens),
            crate::cli::cost::fmt_tokens(input_tokens),
            crate::cli::cost::fmt_tokens(output_tokens),
        );
        println!(
            "  Est. cost: ${:.4} Sonnet / ${:.4} Opus  (run 'acs cost' for per-ticket breakdown)",
            sonnet_cost, opus_cost
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_non_live_with_empty_db() {
        let db = Db::open_memory().unwrap();
        execute_with_db(&db, false).unwrap();
    }

    #[test]
    fn status_non_live_with_agents_and_tokens() {
        let db = Db::open_memory().unwrap();
        let t = db.create_ticket("T1", "D", "core", 1).unwrap();
        db.update_ticket(&t, "completed", Some("w-1"), None, None)
            .unwrap();
        db.register_agent("w-1", "worker", "general").unwrap();
        db.update_agent("w-1", "working", Some(&t), None).unwrap();
        db.log_token_event(
            Some("w-1"),
            "token_usage",
            "tracked",
            70,
            30,
            Some(&t),
            Some("claude"),
        )
        .unwrap();
        execute_with_db(&db, false).unwrap();
    }

    #[test]
    fn status_shows_last_rebuilt_never_when_no_events() {
        let db = Db::open_memory().unwrap();
        // Just verify it doesn't panic and runs without error.
        execute_with_db(&db, false).unwrap();
    }

    #[test]
    fn status_shows_last_rebuilt_after_event() {
        let db = Db::open_memory().unwrap();
        db.log_event(Some("mgr"), "binary_rebuilt", "built ok", None).unwrap();
        execute_with_db(&db, false).unwrap();
    }

    #[test]
    fn format_time_ago_returns_seconds_for_recent() {
        let ts = Utc::now().to_rfc3339();
        let result = format_time_ago(&ts);
        assert!(result.ends_with("seconds ago"), "unexpected: {}", result);
    }

    #[test]
    fn format_time_ago_falls_back_on_invalid_ts() {
        let result = format_time_ago("not-a-timestamp");
        assert_eq!(result, "not-a-timestamp");
    }
}
