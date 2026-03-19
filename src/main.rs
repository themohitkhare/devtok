use clap::Parser;
use acs::cli;
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
        cli::Commands::Run { workers } => cli::run::execute(workers),
        cli::Commands::Evolve {
            workers,
            max_iterations,
            plan_each_iteration,
            bootstrap_after_run,
            stop_when_no_new_tickets,
            max_run_seconds,
            dry_run,
        } => cli::evolve::execute(
            workers,
            max_iterations,
            plan_each_iteration,
            bootstrap_after_run,
            stop_when_no_new_tickets,
            max_run_seconds,
            dry_run,
        ),
        cli::Commands::Cleanup => cli::cleanup::execute(),
        cli::Commands::Status => cli::status::execute(),
        cli::Commands::Log { follow, limit } => cli::log::execute(follow, limit),
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
