pub mod acs_dir;
pub mod approve;
pub mod check;
pub mod doctor;
pub mod cleanup;
pub mod cost;
pub mod evolve;
pub mod export;
pub mod health;
pub mod inbox;
pub mod init;
pub mod kb;
pub mod log;
pub mod milestone;
pub mod plan;
pub mod provider;
pub mod quality;
pub mod reject;
pub mod report;
pub mod restart;
pub mod run;
pub mod standup;
pub mod status;
pub mod status_live;
pub mod stop;
pub mod ticket;
pub mod update;

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
        /// Number of worker agents (overrides profile/config default)
        #[arg(long)]
        workers: Option<usize>,
        /// Backend provider: claude, cursor, codex, or mixed (first half claude, second half cursor)
        #[arg(long)]
        backend: Option<String>,
        /// Enable autoscaling workers based on queue depth
        #[arg(long, default_value = "false")]
        autoscale: bool,
        /// Minimum workers to keep when autoscaling
        #[arg(long, default_value = "1")]
        min_workers: usize,
        /// Named config profile (dev/ci/prod). Also set via ACS_PROFILE env var.
        #[arg(long)]
        profile: Option<String>,
    },
    /// Iteratively run manager/workers + incremental bootstrap (self-development loop)
    Evolve {
        /// Number of worker agents (overrides profile/config default)
        #[arg(long)]
        workers: Option<usize>,
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
        /// Do not overwrite existing agents in the DB (useful to resume after
        /// an interrupted run). This helps avoid clobbering in-flight work.
        #[arg(long, default_value = "false")]
        preserve_agents: bool,
        /// Dry-run: don't spawn Claude/agents; just print current ticket count
        #[arg(long, default_value = "false")]
        dry_run: bool,
        /// Backend provider: claude, cursor, codex, or mixed (first half claude, second half cursor)
        #[arg(long)]
        backend: Option<String>,
        /// Auto-merge approved tickets to main and rebuild the acs binary (opt-in)
        #[arg(long, default_value = "false")]
        auto_merge: bool,
    },
    /// Show project status
    Status {
        /// Live TUI dashboard (refreshes every 2s, Ctrl+C to exit)
        #[arg(long)]
        live: bool,
    },
    /// Remove stale acs/* branches and orphaned worktrees
    Cleanup,
    /// Gracefully restart a running ACS instance
    Restart {
        /// Number of worker agents (defaults to config project.default_workers)
        #[arg(long)]
        workers: Option<usize>,
        /// Backend provider: claude, cursor, codex, or mixed
        #[arg(long)]
        backend: Option<String>,
        /// Seconds to wait for graceful shutdown before SIGKILL
        #[arg(long, default_value = "20")]
        wait_seconds: u64,
    },
    /// Gracefully stop a running ACS instance
    Stop {
        /// Seconds to wait for graceful shutdown before SIGKILL
        #[arg(long, default_value = "20")]
        wait_seconds: u64,
    },
    /// Run system diagnostics health checks (DB/workers/worktrees/git/blocked-tickets)
    Health,
    /// Show event log
    Log {
        /// Follow mode (like tail -f)
        #[arg(long)]
        follow: bool,
        /// Max entries to show
        #[arg(long, default_value = "20")]
        limit: usize,
        /// Filter events by worker/agent id (e.g. w-8, mgr)
        #[arg(long)]
        worker: Option<String>,
        /// Additional filter key=value (supported: worker, ticket). Repeatable.
        #[arg(long = "filter")]
        filters: Vec<String>,
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
    /// Generate a current progress report in .acs/reports/progress.md
    Report,
    /// Show token usage and estimated cost breakdown
    Cost,
    /// Export project summary as markdown or json
    Export {
        /// Output format
        #[arg(long, value_enum, default_value_t = export::ExportFormat::Markdown)]
        format: export::ExportFormat,
        /// Optional output file path; defaults to stdout
        #[arg(long)]
        out: Option<String>,
    },
    /// Manage milestones
    #[command(subcommand)]
    Milestone(milestone::MilestoneCommands),
    /// Show milestones awaiting CEO approval
    Check,
    /// Approve the current awaiting-approval milestone
    Approve,
    /// Reject the current awaiting-approval milestone with a reason
    Reject {
        #[arg(long)]
        reason: String,
    },
    /// Quality scoring and North Star metrics
    #[command(subcommand)]
    Quality(quality::QualityCommands),
    /// Manage provider registry (list, enable, blacklist)
    #[command(subcommand)]
    Provider(provider::ProviderCommands),
    /// Generate a daily standup summary (completed, in-progress, blocked, metrics)
    Standup {
        /// Output machine-readable JSON instead of human text
        #[arg(long)]
        json: bool,
        /// Post the standup as a GitHub issue comment (requires gh CLI)
        #[arg(long)]
        post_github: bool,
    },
    /// Validate the environment and auto-fix common issues
    Doctor {
        /// Apply automatic fixes (read-only by default)
        #[arg(long)]
        fix: bool,
    },
}
