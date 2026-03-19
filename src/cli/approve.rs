use anyhow::Result;
use crate::db::Db;

/// `acs approve` — approve the current awaiting-approval milestone and advance to the next.
pub fn execute() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;

    match db.approve_milestone()? {
        None => {
            println!("No milestone is awaiting approval.");
            println!("Run `acs check` to see the current state.");
        }
        Some((approved_id, next_id)) => {
            let approved = db.get_milestone(approved_id)?.unwrap();
            println!("Approved milestone [{}]: {}", approved.id, approved.name);

            db.log_event(
                Some("ceo"),
                "milestone_approved",
                &format!("milestone {} '{}' approved", approved.id, approved.name),
                None,
            )?;

            match next_id {
                Some(nid) => {
                    let next = db.get_milestone(nid)?.unwrap();
                    println!("Activated next milestone [{}]: {}", next.id, next.name);
                    println!("Goal: {}", next.goal);
                    println!("\nRun `acs run` to continue executing tickets in this milestone.");

                    db.log_event(
                        Some("ceo"),
                        "milestone_activated",
                        &format!("milestone {} '{}' activated", next.id, next.name),
                        None,
                    )?;
                }
                None => {
                    println!("\nAll milestones complete! No more pending milestones.");
                    println!("Run `acs report` to generate a final progress report.");
                }
            }
        }
    }

    Ok(())
}
