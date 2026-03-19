use clap::Subcommand;
use anyhow::{anyhow, Result};
use crate::db::Db;
use std::path::Path;

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
}

pub fn execute(cmd: KbCommands) -> Result<()> {
    let db = Db::open(Path::new(".acs/project.db"))?;

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
    }

    Ok(())
}
