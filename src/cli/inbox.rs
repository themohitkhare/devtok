use clap::Subcommand;
use anyhow::Result;
use crate::db::Db;
use std::path::Path;
use std::thread;
use std::time::Duration;

#[derive(Subcommand)]
pub enum InboxCommands {
    /// Push a message to an agent's inbox
    Push {
        /// Recipient agent id (alias: --recipient)
        #[arg(long, alias = "recipient")]
        to: String,
        /// Message type (alias: --msg-type)
        #[arg(long = "type", alias = "msg-type")]
        msg_type: String,
        #[arg(long)]
        payload: String,
        /// Sender id (defaults to "system")
        #[arg(long, default_value = "system")]
        sender: String,
    },
    /// Pop the next unread message from an agent's inbox
    Pop {
        #[arg(long)]
        agent: String,
        /// Seconds to wait for a message (0 = no wait)
        #[arg(long, default_value = "0")]
        timeout: u64,
    },
}

pub fn execute(cmd: InboxCommands) -> Result<()> {
    let db = Db::open(Path::new(".acs/project.db"))?;

    match cmd {
        InboxCommands::Push { to, msg_type, payload, sender } => {
            db.push_inbox(&to, &msg_type, &payload, &sender)?;
            let out = serde_json::json!({ "status": "pushed" });
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        InboxCommands::Pop { agent, timeout } => {
            let deadline = if timeout == 0 { 0u64 } else { timeout };
            let mut elapsed = 0u64;

            loop {
                if let Some(msg) = db.pop_inbox(&agent)? {
                    println!("{}", serde_json::to_string_pretty(&msg)?);
                    return Ok(());
                }

                if elapsed >= deadline {
                    // No message available within timeout (or no-wait mode)
                    let out = serde_json::json!({ "status": "empty" });
                    println!("{}", serde_json::to_string_pretty(&out)?);
                    return Ok(());
                }

                thread::sleep(Duration::from_secs(1));
                elapsed += 1;
            }
        }
    }

    Ok(())
}
