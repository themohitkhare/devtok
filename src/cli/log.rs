use anyhow::Result;
use crate::db::Db;

pub fn execute(follow: bool, limit: usize, worker: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;

    let events = match worker.as_deref() {
        Some(agent) => db.recent_events_for_agent(agent, limit)?,
        None => db.recent_events(limit)?,
    };
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
            let new_events = match worker.as_deref() {
                Some(agent) => db.recent_events_for_agent(agent, 10)?,
                None => db.recent_events(10)?,
            };
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
