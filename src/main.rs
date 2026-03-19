use clap::Parser;
use acs::cli;

fn main() -> anyhow::Result<()> {
    let args = cli::Cli::parse();
    match args.command {
        cli::Commands::Ticket(cmd) => cli::ticket::execute(cmd),
        cli::Commands::Kb(cmd) => cli::kb::execute(cmd),
        cli::Commands::Inbox(cmd) => cli::inbox::execute(cmd),
        cli::Commands::Init { spec, auto } => cli::init::execute(spec, auto),
        cli::Commands::Run { workers } => cli::run::execute(workers),
        cli::Commands::Status => cli::status::execute(),
        cli::Commands::Log { follow, limit } => cli::log::execute(follow, limit),
    }
}
