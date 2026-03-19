use acs::cli;
use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = match cli::Cli::try_parse() {
        Ok(args) => args,
        Err(e) => {
            // Clap errors (--help, --version, missing args) go to stderr as JSON
            let msg = e.to_string();
            // --help and --version are "success" exits — print normally
            if e.use_stderr() {
                eprintln!("{}", serde_json::json!({ "error": msg }));
                return ExitCode::from(2);
            } else {
                print!("{}", e);
                return ExitCode::SUCCESS;
            }
        }
    };

    let result = match args.command {
        cli::Commands::Ticket(cmd) => cli::ticket::execute(cmd),
        cli::Commands::Kb(cmd) => cli::kb::execute(cmd),
        cli::Commands::Inbox(cmd) => cli::inbox::execute(cmd),
        cli::Commands::Init { spec, auto } => cli::init::execute(spec, auto),
        cli::Commands::Plan => cli::plan::execute(),
        cli::Commands::Run { workers, backend } => cli::run::execute(workers, backend),
        cli::Commands::Health => cli::health::execute(),
        cli::Commands::Evolve {
            workers,
            max_iterations,
            plan_each_iteration,
            bootstrap_after_run,
            stop_when_no_new_tickets,
            max_run_seconds,
            preserve_agents,
            dry_run,
            backend,
        } => cli::evolve::execute(
            workers,
            max_iterations,
            plan_each_iteration,
            bootstrap_after_run,
            stop_when_no_new_tickets,
            max_run_seconds,
            preserve_agents,
            dry_run,
            backend,
        ),
        cli::Commands::Cleanup => cli::cleanup::execute(),
        cli::Commands::Restart {
            workers,
            backend,
            wait_seconds,
        } => cli::restart::execute(workers, backend, wait_seconds),
        cli::Commands::Status { live } => cli::status::execute(live),
        cli::Commands::Log {
            follow,
            limit,
            worker,
            filters,
        } => cli::log::execute(follow, limit, worker, filters),
        cli::Commands::Report => cli::report::execute(),
        cli::Commands::Cost => cli::cost::execute(),
        cli::Commands::Export { format, out } => cli::export::execute(format, out),
        cli::Commands::Milestone(cmd) => cli::milestone::execute(cmd),
        cli::Commands::Check => cli::check::execute(),
        cli::Commands::Approve => cli::approve::execute(),
        cli::Commands::Reject { reason } => cli::reject::execute(reason),
        cli::Commands::Quality(cmd) => cli::quality::execute(cmd),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Format the full error chain for debugging context
            let msg = format!("{:#}", e);
            eprintln!("{}", serde_json::json!({ "error": msg }));
            ExitCode::FAILURE
        }
    }
}
