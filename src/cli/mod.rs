pub mod ticket;
pub mod kb;
pub mod inbox;
pub mod init;
pub mod plan;
pub mod run;
pub mod cleanup;
pub mod status;
pub mod log;
pub mod acs_dir;
pub mod evolve;

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
    /// Run the Solution Architect agent to plan milestones and write ADRs
    Plan,
    /// Start manager + workers to execute tickets
    Run {
        /// Number of worker agents
        #[arg(long, default_value = "2")]
        workers: usize,
    },
    /// Iteratively run manager/workers + incremental bootstrap (self-development loop)
    Evolve {
        /// Number of worker agents
        #[arg(long, default_value = "2")]
        workers: usize,
        /// Maximum evolution iterations
        #[arg(long, default_value = "1")]
        max_iterations: usize,
        /// Run solution architect each iteration
        #[arg(long)]
        plan_each_iteration: bool,
        /// Run incremental bootstrap after each bounded execution
        #[arg(long, default_value = "true")]
        bootstrap_after_run: bool,
        /// Stop when bootstrap produces no new tickets
        #[arg(long, default_value = "true")]
        stop_when_no_new_tickets: bool,
        /// Max seconds for a single bounded manager/worker run (optional)
        #[arg(long)]
        max_run_seconds: Option<u64>,
        /// Dry-run: don't spawn Claude/agents; just print current ticket count
        #[arg(long, default_value = "false")]
        dry_run: bool,
    },
    /// Show project status
    Status,
    /// Remove stale acs/* branches and orphaned worktrees
    Cleanup,
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
