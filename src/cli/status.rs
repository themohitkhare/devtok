use crate::db::Db;
use anyhow::Result;

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

    // Agent status
    let agents = db.list_agents()?;
    if !agents.is_empty() {
        println!("\nAgents:");
        for a in &agents {
            let ticket_info = a.current_ticket.as_deref().unwrap_or("-");
            println!(
                "  {} ({}/{}) — {} [{}]",
                a.id, a.role, a.persona, a.status, ticket_info
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
}
