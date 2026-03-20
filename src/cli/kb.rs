use clap::Subcommand;
use anyhow::{anyhow, Result};
use crate::db::Db;

#[derive(Subcommand)]
pub enum KbCommands {
    /// Read a knowledge base entry
    Read {
        #[arg(long)]
        domain: String,
        #[arg(long)]
        key: String,
    },
    /// Write a knowledge base entry
    Write {
        #[arg(long)]
        domain: String,
        #[arg(long)]
        key: String,
        #[arg(long)]
        value: String,
    },
    /// List knowledge base entries, optionally filtered by type (domain)
    List {
        /// Filter by type/domain (e.g. --type learning)
        #[arg(long = "type")]
        kb_type: Option<String>,
    },
}

pub fn execute(cmd: KbCommands) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;

    match cmd {
        KbCommands::Read { domain, key } => {
            match db.read_knowledge(&domain, &key)? {
                Some(entry) => println!("{}", serde_json::to_string_pretty(&entry)?),
                None => return Err(anyhow!("knowledge entry '{}/{}' not found", domain, key)),
            }
        }
        KbCommands::Write { domain, key, value } => {
            db.write_knowledge(&domain, &key, &value)?;
            let out = serde_json::json!({ "status": "written", "domain": domain, "key": key });
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        KbCommands::List { kb_type } => {
            let entries = match kb_type.as_deref() {
                Some(domain) => db.list_knowledge_by_domain(domain)?,
                None => db.list_all_knowledge()?,
            };
            println!("{}", serde_json::to_string_pretty(&entries)?);
        }
    }

    Ok(())
}
