// src/cli/provider.rs — t-090 Provider registry CLI commands
//
// acs provider list             — show all providers and ACTIVE/BLACKLISTED state
// acs provider enable <name>    — remove blacklist, provider is active again
// acs provider blacklist <name> — manually blacklist a provider

use anyhow::Result;
use clap::Subcommand;

use crate::config::Config;
use crate::provider_registry::ProviderRegistry;

#[derive(Subcommand)]
pub enum ProviderCommands {
    /// List all providers and their ACTIVE/BLACKLISTED state
    List,
    /// Re-enable a blacklisted provider (removes blacklist)
    Enable {
        /// Provider name (e.g. claude, cursor, codex)
        name: String,
    },
    /// Manually blacklist a provider
    Blacklist {
        /// Provider name (e.g. claude, cursor, codex)
        name: String,
    },
}

pub fn execute(cmd: ProviderCommands) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;

    // Load config for failover_order; fall back to empty if config can't be read
    let failover_order = Config::load(&acs_dir.join("config.toml"))
        .map(|c| c.backends.failover_order)
        .unwrap_or_default();

    let mut registry = ProviderRegistry::load(&acs_dir);

    match cmd {
        ProviderCommands::List => {
            if failover_order.is_empty() {
                // No failover_order configured — show everything in registry
                if registry.providers.is_empty() {
                    println!(
                        "{}",
                        serde_json::json!({
                            "providers": [],
                            "note": "No failover_order configured and no providers blacklisted"
                        })
                    );
                } else {
                    let providers: Vec<serde_json::Value> = registry
                        .providers
                        .iter()
                        .map(|(name, state)| {
                            serde_json::json!({
                                "name": name,
                                "state": state.to_string(),
                            })
                        })
                        .collect();
                    println!("{}", serde_json::json!({ "providers": providers }));
                }
            } else {
                let states = registry.all_states(&failover_order);
                let providers: Vec<serde_json::Value> = states
                    .into_iter()
                    .map(|(name, state)| {
                        serde_json::json!({
                            "name": name,
                            "state": state.to_string(),
                        })
                    })
                    .collect();
                println!("{}", serde_json::json!({ "providers": providers }));
            }
        }

        ProviderCommands::Enable { name } => {
            registry.enable(&name);
            registry.save(&acs_dir)?;
            println!(
                "{}",
                serde_json::json!({
                    "ok": true,
                    "provider": name,
                    "state": "ACTIVE",
                    "message": format!("provider '{}' re-enabled", name),
                })
            );
        }

        ProviderCommands::Blacklist { name } => {
            registry.blacklist(&name);
            registry.save(&acs_dir)?;
            println!(
                "{}",
                serde_json::json!({
                    "ok": true,
                    "provider": name,
                    "state": "BLACKLISTED",
                    "message": format!("provider '{}' blacklisted", name),
                })
            );
        }
    }

    Ok(())
}
