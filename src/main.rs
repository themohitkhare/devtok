use clap::Parser;
use acs::cli;

fn main() -> anyhow::Result<()> {
    let args = cli::Cli::parse();
    match args.command {
        cli::Commands::Ticket(cmd) => cli::ticket::execute(cmd),
        cli::Commands::Kb(cmd) => cli::kb::execute(cmd),
        cli::Commands::Inbox(cmd) => cli::inbox::execute(cmd),
        cli::Commands::Status => { println!("TODO: status"); Ok(()) },
        cli::Commands::Log { .. } => { println!("TODO: log"); Ok(()) },
        cli::Commands::Init { spec, auto } => cli::init::execute(spec, auto),
        cli::Commands::Run { .. } => { println!("TODO: run"); Ok(()) },
    }
}
