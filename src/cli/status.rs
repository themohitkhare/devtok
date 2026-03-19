use anyhow::Result;
use crate::db::Db;

pub fn execute() -> Result<()> {
    let acs_dir = std::env::current_dir()?.join(".acs");
    if !acs_dir.exists() {
        anyhow::bail!(".acs/ not found. Run `acs init` first.");
    }

    let db = Db::open(&acs_dir.join("project.db"))?;

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
            println!("  {} ({}/{}) — {} [{}]", a.id, a.role, a.persona, a.status, ticket_info);
        }
    }

    // Token usage
    let events = db.recent_events(100)?;
    let total_tokens: i64 = events.iter().filter_map(|e| e.tokens_used).sum();
    if total_tokens > 0 {
        println!("\nTokens used: {}", total_tokens);
    }

    Ok(())
}
