use clap::Subcommand;
use anyhow::{anyhow, Result};
use crate::db::Db;
use std::path::Path;

#[derive(Subcommand)]
pub enum TicketCommands {
    /// List tickets
    List {
        #[arg(long)]
        status: Option<String>,
    },
    /// Create a ticket
    Create {
        #[arg(long)]
        title: String,
        #[arg(long)]
        description: String,
        #[arg(long)]
        domain: String,
        #[arg(long, default_value = "3")]
        priority: i32,
        #[arg(long)]
        blocked_by: Option<String>,
    },
    /// Update ticket status
    Update {
        #[arg(long)]
        id: String,
        #[arg(long)]
        status: String,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        blocked_by: Option<String>,
    },
    /// Show ticket details
    Show {
        id: String,
    },
}

pub fn execute(cmd: TicketCommands) -> Result<()> {
    let db = Db::open(Path::new(".acs/project.db"))?;

    match cmd {
        TicketCommands::Create { title, description, domain, priority, blocked_by } => {
            let id = db.create_ticket(&title, &description, &domain, priority)?;
            if let Some(blocked) = blocked_by {
                db.update_ticket(&id, "pending", None, Some(&blocked), None)?;
            }
            let out = serde_json::json!({ "status": "created", "id": id });
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        TicketCommands::List { status } => {
            let tickets = db.list_tickets(status.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&tickets)?);
        }
        TicketCommands::Update { id, status, notes, blocked_by } => {
            db.update_ticket(&id, &status, notes.as_deref(), blocked_by.as_deref(), None)?;
            let out = serde_json::json!({ "status": "updated" });
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        TicketCommands::Show { id } => {
            match db.get_ticket(&id)? {
                Some(ticket) => println!("{}", serde_json::to_string_pretty(&ticket)?),
                None => return Err(anyhow!("ticket '{}' not found", id)),
            }
        }
    }

    Ok(())
}
