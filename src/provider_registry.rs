// src/provider_registry.rs — t-090 Provider failover registry
//
// Tracks ACTIVE/BLACKLISTED state for each provider backend.
// State is persisted to .acs/provider_state.json so blacklists survive
// acs restarts. Re-enable is always manual to prevent infinite retry loops.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const STATE_FILE: &str = "provider_state.json";

/// Runtime state of a provider backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ProviderState {
    Active,
    Blacklisted,
}

impl std::fmt::Display for ProviderState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderState::Active => write!(f, "ACTIVE"),
            ProviderState::Blacklisted => write!(f, "BLACKLISTED"),
        }
    }
}

/// Registry of provider states. Persisted to `.acs/provider_state.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderRegistry {
    pub providers: HashMap<String, ProviderState>,
}

impl ProviderRegistry {
    /// Load registry from `.acs/provider_state.json`.
    /// Returns an empty registry if the file doesn't exist or can't be parsed.
    pub fn load(acs_dir: &Path) -> Self {
        let path = acs_dir.join(STATE_FILE);
        let Ok(contents) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&contents).unwrap_or_default()
    }

    /// Persist registry to `.acs/provider_state.json`.
    pub fn save(&self, acs_dir: &Path) -> Result<()> {
        let path = acs_dir.join(STATE_FILE);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Blacklist a provider (e.g. after quota exhaustion).
    pub fn blacklist(&mut self, provider: &str) {
        self.providers
            .insert(provider.to_string(), ProviderState::Blacklisted);
    }

    /// Re-enable a previously blacklisted provider.
    pub fn enable(&mut self, provider: &str) {
        self.providers
            .insert(provider.to_string(), ProviderState::Active);
    }

    /// Returns `true` if the provider is not blacklisted.
    /// Unknown providers default to ACTIVE (fail-open).
    pub fn is_active(&self, provider: &str) -> bool {
        self.providers
            .get(provider)
            .map(|s| s == &ProviderState::Active)
            .unwrap_or(true)
    }

    /// Find the first ACTIVE provider in failover_order.
    /// Returns `None` when all providers in the list are blacklisted.
    pub fn next_active<'a>(&self, failover_order: &'a [String]) -> Option<&'a str> {
        failover_order
            .iter()
            .find(|p| self.is_active(p))
            .map(|s| s.as_str())
    }

    /// Return (provider_name, state) for all providers in failover_order.
    /// Providers not in the registry are shown as ACTIVE (fail-open).
    pub fn all_states(&self, failover_order: &[String]) -> Vec<(String, ProviderState)> {
        failover_order
            .iter()
            .map(|p| {
                let state = self
                    .providers
                    .get(p)
                    .cloned()
                    .unwrap_or(ProviderState::Active);
                (p.clone(), state)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp_acs_dir() -> TempDir {
        TempDir::new().unwrap()
    }

    #[test]
    fn default_registry_is_empty() {
        let r = ProviderRegistry::default();
        assert!(r.providers.is_empty());
    }

    #[test]
    fn unknown_provider_is_active() {
        let r = ProviderRegistry::default();
        assert!(r.is_active("claude"));
        assert!(r.is_active("codex"));
    }

    #[test]
    fn blacklist_makes_provider_inactive() {
        let mut r = ProviderRegistry::default();
        r.blacklist("claude");
        assert!(!r.is_active("claude"));
    }

    #[test]
    fn enable_restores_active_state() {
        let mut r = ProviderRegistry::default();
        r.blacklist("claude");
        r.enable("claude");
        assert!(r.is_active("claude"));
    }

    #[test]
    fn next_active_skips_blacklisted() {
        let mut r = ProviderRegistry::default();
        r.blacklist("claude");
        let order = vec![
            "claude".to_string(),
            "cursor".to_string(),
            "codex".to_string(),
        ];
        assert_eq!(r.next_active(&order), Some("cursor"));
    }

    #[test]
    fn next_active_returns_none_when_all_blacklisted() {
        let mut r = ProviderRegistry::default();
        let order = vec![
            "claude".to_string(),
            "cursor".to_string(),
        ];
        r.blacklist("claude");
        r.blacklist("cursor");
        assert_eq!(r.next_active(&order), None);
    }

    #[test]
    fn next_active_returns_first_when_none_blacklisted() {
        let r = ProviderRegistry::default();
        let order = vec!["claude".to_string(), "cursor".to_string()];
        assert_eq!(r.next_active(&order), Some("claude"));
    }

    #[test]
    fn all_states_shows_active_for_unknown_providers() {
        let r = ProviderRegistry::default();
        let order = vec!["claude".to_string(), "codex".to_string()];
        let states = r.all_states(&order);
        assert_eq!(states.len(), 2);
        assert_eq!(states[0], ("claude".to_string(), ProviderState::Active));
        assert_eq!(states[1], ("codex".to_string(), ProviderState::Active));
    }

    #[test]
    fn all_states_reflects_blacklist() {
        let mut r = ProviderRegistry::default();
        r.blacklist("claude");
        let order = vec!["claude".to_string(), "cursor".to_string()];
        let states = r.all_states(&order);
        assert_eq!(states[0], ("claude".to_string(), ProviderState::Blacklisted));
        assert_eq!(states[1], ("cursor".to_string(), ProviderState::Active));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tmp_acs_dir();
        let mut r = ProviderRegistry::default();
        r.blacklist("claude");
        r.enable("cursor");
        r.save(dir.path()).unwrap();

        let loaded = ProviderRegistry::load(dir.path());
        assert!(!loaded.is_active("claude"));
        assert!(loaded.is_active("cursor"));
    }

    #[test]
    fn load_returns_default_when_file_missing() {
        let dir = tmp_acs_dir();
        let r = ProviderRegistry::load(dir.path());
        assert!(r.providers.is_empty());
    }

    #[test]
    fn load_returns_default_on_corrupt_json() {
        let dir = tmp_acs_dir();
        std::fs::write(dir.path().join(STATE_FILE), "not-json!!!").unwrap();
        let r = ProviderRegistry::load(dir.path());
        assert!(r.providers.is_empty());
    }

    #[test]
    fn blacklist_persists_across_restart() {
        let dir = tmp_acs_dir();
        {
            let mut r = ProviderRegistry::load(dir.path());
            r.blacklist("claude");
            r.save(dir.path()).unwrap();
        }
        {
            let r = ProviderRegistry::load(dir.path());
            assert!(!r.is_active("claude"));
        }
    }

    #[test]
    fn provider_state_display() {
        assert_eq!(ProviderState::Active.to_string(), "ACTIVE");
        assert_eq!(ProviderState::Blacklisted.to_string(), "BLACKLISTED");
    }
}
