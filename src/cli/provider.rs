// src/cli/provider.rs — Provider registry CLI commands (t-090)
//
// Exposes runtime provider state management:
//   acs provider list               — show all providers and ACTIVE/BLACKLISTED state
//   acs provider enable <name>      — remove blacklist (workers can use this provider again)
//   acs provider blacklist <name>   — manually blacklist a provider

use anyhow::Result;
use clap::Subcommand;

use crate::provider_registry::ProviderRegistry;

#[derive(Subcommand)]
pub enum ProviderCommands {
    /// List all providers and their ACTIVE/BLACKLISTED state
    List,
    /// Re-enable a blacklisted provider (removes it from the blacklist)
    Enable {
        /// Provider name (e.g. claude, cursor, codex)
        name: String,
    },
    /// Manually blacklist a provider (workers will skip it)
    Blacklist {
        /// Provider name (e.g. claude, cursor, codex)
        name: String,
    },
}

pub fn execute(cmd: ProviderCommands) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;

    // Load config to get the failover_order for display ordering.
    let config_path = acs_dir.join("config.toml");
    let failover_order: Vec<String> = if config_path.exists() {
        crate::config::Config::load(&config_path)
            .map(|c| c.backends.failover_order)
            .unwrap_or_default()
    } else {
        vec![]
    };

    match cmd {
        ProviderCommands::List => {
            let registry = ProviderRegistry::load(&acs_dir);
            let states = registry.all_states(&failover_order);

            if states.is_empty() {
                // No state file and no failover_order — show a helpful hint.
                println!("{}", serde_json::json!({
                    "providers": [],
                    "note": "No provider state found. Configure [backends] failover_order in config.toml or run acs to generate state."
                }));
            } else {
                let entries: Vec<_> = states
                    .into_iter()
                    .map(|(name, state)| {
                        serde_json::json!({
                            "provider": name,
                            "state": state.to_string(),
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&entries)?);
            }
        }

        ProviderCommands::Enable { name } => {
            let mut registry = ProviderRegistry::load(&acs_dir);
            registry.enable(&name);
            registry.save(&acs_dir)?;
            println!(
                "{}",
                serde_json::json!({
                    "status": "enabled",
                    "provider": name,
                    "state": "ACTIVE",
                })
            );
        }

        ProviderCommands::Blacklist { name } => {
            let mut registry = ProviderRegistry::load(&acs_dir);
            registry.blacklist(&name);
            registry.save(&acs_dir)?;
            println!(
                "{}",
                serde_json::json!({
                    "status": "blacklisted",
                    "provider": name,
                    "state": "BLACKLISTED",
                })
            );
        }
    }

    Ok(())
}
