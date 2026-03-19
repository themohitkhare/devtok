use clap::Subcommand;
use anyhow::{anyhow, Result};
use crate::db::Db;

#[derive(Subcommand)]
pub enum MilestoneCommands {
    /// Create a new milestone
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        goal: String,
    },
    /// Assign a ticket to a milestone
    Assign {
        #[arg(long)]
        milestone_id: i64,
        #[arg(long)]
        ticket: String,
    },
    /// List all milestones
    List,
    /// Show a specific milestone
    Show {
        id: i64,
    },
    /// Activate a specific milestone (set it to active status)
    Activate {
        #[arg(long)]
        id: i64,
    },
}

pub fn execute(cmd: MilestoneCommands) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;

    match cmd {
        MilestoneCommands::Create { name, goal } => {
            let id = db.create_milestone(&name, &goal)?;
            let out = serde_json::json!({ "status": "created", "id": id, "name": name });
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        MilestoneCommands::Assign { milestone_id, ticket } => {
            // Validate ticket exists
            if db.get_ticket(&ticket)?.is_none() {
                return Err(anyhow!("ticket '{}' not found", ticket));
            }
            // Validate milestone exists
            if db.get_milestone(milestone_id)?.is_none() {
                return Err(anyhow!("milestone {} not found", milestone_id));
            }
            db.assign_ticket_to_milestone(milestone_id, &ticket)?;
            let out = serde_json::json!({ "status": "assigned", "milestone_id": milestone_id, "ticket_id": ticket });
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        MilestoneCommands::List => {
            let milestones = db.list_milestones()?;
            println!("{}", serde_json::to_string_pretty(&milestones)?);
        }
        MilestoneCommands::Show { id } => {
            match db.get_milestone(id)? {
                Some(ms) => println!("{}", serde_json::to_string_pretty(&ms)?),
                None => return Err(anyhow!("milestone {} not found", id)),
            }
        }
        MilestoneCommands::Activate { id } => {
            match db.get_milestone(id)? {
                None => return Err(anyhow!("milestone {} not found", id)),
                Some(_) => {
                    db.update_milestone_status(id, "active")?;
                    let out = serde_json::json!({ "status": "activated", "id": id });
                    println!("{}", serde_json::to_string_pretty(&out)?);
                }
            }
        }
    }

    Ok(())
}
