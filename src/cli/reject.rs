// src/cli/reject.rs
use anyhow::Result;
use crate::db::Db;

/// `acs reject --reason "..."` — reject the current awaiting-approval milestone
/// and send feedback to the architect inbox so they can revise the plan.
pub fn execute(reason: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;

    match db.reject_milestone()? {
        None => {
            println!("No milestone is awaiting approval.");
            println!("Run `acs check` to see the current state.");
        }
        Some(rejected_id) => {
            let ms = db.get_milestone(rejected_id)?.unwrap();
            println!("Rejected milestone [{}]: {}", ms.id, ms.name);
            println!("Reason: {}", reason);

            // Notify the architect inbox
            let payload = serde_json::json!({
                "milestone_id": rejected_id,
                "milestone_name": ms.name,
                "reason": reason,
            })
            .to_string();

            db.push_inbox("architect", "milestone_rejected", &payload, "ceo")?;

            db.log_event(
                Some("ceo"),
                "milestone_rejected",
                &format!(
                    "milestone {} '{}' rejected: {}",
                    rejected_id, ms.name, reason
                ),
                None,
            )?;

            println!("\nFeedback sent to architect inbox.");
            println!("Run `acs plan` to have the architect revise the plan.");
        }
    }

    Ok(())
}
