use anyhow::Result;
use crate::db::Db;

/// `acs check` — show milestones pending CEO approval.
pub fn execute() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;

    if !db.has_milestones()? {
        println!("No milestones configured. Run `acs plan` to create milestones.");
        return Ok(());
    }

    let milestones = db.list_milestones()?;

    let awaiting: Vec<_> = milestones.iter().filter(|m| m.status == "awaiting_approval").collect();
    let active: Vec<_> = milestones.iter().filter(|m| m.status == "active").collect();
    let pending: Vec<_> = milestones.iter().filter(|m| m.status == "pending").collect();

    if awaiting.is_empty() {
        println!("No milestones awaiting approval.");

        if let Some(ms) = active.first() {
            let ticket_ids = db.get_milestone_ticket_ids(ms.id)?;
            let all_tickets = db.list_tickets(None)?;
            let ms_tickets: Vec<_> = all_tickets.iter()
                .filter(|t| ticket_ids.contains(&t.id))
                .collect();
            let done = ms_tickets.iter().filter(|t| t.status == "completed").count();
            println!(
                "\nActive milestone: [{}] {} — {} ({}/{})",
                ms.id, ms.name, ms.goal, done, ms_tickets.len()
            );
        }
    } else {
        for ms in &awaiting {
            println!("=== MILESTONE AWAITING APPROVAL ===");
            println!("  ID:   {}", ms.id);
            println!("  Name: {}", ms.name);
            println!("  Goal: {}", ms.goal);

            // Show ticket summary
            let ticket_ids = db.get_milestone_ticket_ids(ms.id)?;
            if !ticket_ids.is_empty() {
                let all_tickets = db.list_tickets(None)?;
                let ms_tickets: Vec<_> = all_tickets.iter()
                    .filter(|t| ticket_ids.contains(&t.id))
                    .collect();
                println!("  Tickets ({} total):", ms_tickets.len());
                for t in &ms_tickets {
                    println!("    - [{}] {} — {}", t.id, t.title, t.status);
                }
            }

            // Check for milestone report
            let report_path = acs_dir.join("reports").join(format!("milestone-{}.md", ms.id));
            if report_path.exists() {
                println!("  Report: {}", report_path.display());
            }

            println!();
            println!("Run `acs approve` to advance to the next milestone.");
            println!("Run `acs reject --reason \"...\"` to send feedback to the architect.");
        }
    }

    // Show upcoming milestones
    if !pending.is_empty() {
        println!("\nUpcoming milestones ({}):", pending.len());
        for ms in &pending {
            println!("  [{}] {} — {}", ms.id, ms.name, ms.goal);
        }
    }

    Ok(())
}
