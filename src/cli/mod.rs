pub mod ticket;
pub mod kb;
pub mod inbox;
pub mod init;
pub mod run;
pub mod status;
pub mod log;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "acs", version, about = "ACS — Auto Consulting Service")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Bootstrap a project: analyze repo and create tickets
    Init {
        /// Path to spec/requirements file
        #[arg(long)]
        spec: Option<String>,
        /// Auto-analyze existing repo
        #[arg(long)]
        auto: bool,
    },
    /// Start manager + workers to execute tickets
    Run {
        /// Number of worker agents
        #[arg(long, default_value = "2")]
        workers: usize,
    },
    /// Show project status
    Status,
    /// Show event log
    Log {
        /// Follow mode (like tail -f)
        #[arg(long)]
        follow: bool,
        /// Max entries to show
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Manage tickets
    #[command(subcommand)]
    Ticket(ticket::TicketCommands),
    /// Knowledge base operations
    #[command(subcommand)]
    Kb(kb::KbCommands),
    /// Agent inbox operations
    #[command(subcommand)]
    Inbox(inbox::InboxCommands),
}
