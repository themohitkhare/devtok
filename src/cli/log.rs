use anyhow::Result;
use crate::db::Db;

pub fn execute(follow: bool, limit: usize) -> Result<()> {
    let acs_dir = std::env::current_dir()?.join(".acs");
    if !acs_dir.exists() {
        anyhow::bail!(".acs/ not found. Run `acs init` first.");
    }

    let db = Db::open(&acs_dir.join("project.db"))?;

    let events = db.recent_events(limit)?;
    for event in events.iter().rev() {
        let agent = event.agent.as_deref().unwrap_or("-");
        let tokens = event.tokens_used.map(|t| format!(" ({}tok)", t)).unwrap_or_default();
        println!("[{}] {} {}: {}{}", event.timestamp, agent, event.event_type, event.detail, tokens);
    }

    if follow {
        // Simple follow mode: poll every second for new events
        let mut last_id = events.first().map(|e| e.id).unwrap_or(0);
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let new_events = db.recent_events(10)?;
            for event in new_events.iter().rev() {
                if event.id > last_id {
                    let agent = event.agent.as_deref().unwrap_or("-");
                    let tokens = event.tokens_used.map(|t| format!(" ({}tok)", t)).unwrap_or_default();
                    println!("[{}] {} {}: {}{}", event.timestamp, agent, event.event_type, event.detail, tokens);
                    last_id = event.id;
                }
            }
        }
    }

    Ok(())
}
