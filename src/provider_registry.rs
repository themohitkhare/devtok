// src/provider_registry.rs — Provider failover registry (t-090)
//
// Maintains a runtime registry of backend providers with ACTIVE/BLACKLISTED states.
// State is persisted to `.acs/provider_state.json` so it survives restarts.
// Re-enable is always manual to prevent infinite retry loops on hard quota hits.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderRegistry {
    pub providers: HashMap<String, ProviderState>,
}

impl ProviderRegistry {
    /// Load from `<acs_dir>/provider_state.json`, or return an empty registry if absent.
    pub fn load(acs_dir: &Path) -> Self {
        let path = acs_dir.join("provider_state.json");
        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist to `<acs_dir>/provider_state.json`.
    pub fn save(&self, acs_dir: &Path) -> Result<()> {
        let path = acs_dir.join("provider_state.json");
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Mark a provider as BLACKLISTED (quota exceeded).
    pub fn blacklist(&mut self, provider: &str) {
        self.providers
            .insert(provider.to_string(), ProviderState::Blacklisted);
    }

    /// Mark a provider as ACTIVE (manual re-enable).
    pub fn enable(&mut self, provider: &str) {
        self.providers
            .insert(provider.to_string(), ProviderState::Active);
    }

    /// Check if a provider is ACTIVE (unknown providers default to ACTIVE).
    pub fn is_active(&self, provider: &str) -> bool {
        self.providers
            .get(provider)
            .map(|s| *s == ProviderState::Active)
            .unwrap_or(true) // unknown providers are ACTIVE by default
    }

    /// Get the next active provider in the failover order.
    /// Returns `None` if all providers are blacklisted or the order is empty.
    pub fn next_active<'a>(&self, failover_order: &'a [String]) -> Option<&'a str> {
        failover_order
            .iter()
            .find(|p| self.is_active(p))
            .map(|s| s.as_str())
    }

    /// Returns provider states for display, merging the failover order with
    /// any explicitly-tracked providers.
    /// Providers in failover_order that aren't in the registry are shown as ACTIVE.
    pub fn all_states(&self, failover_order: &[String]) -> Vec<(String, ProviderState)> {
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // First, emit providers in failover_order order.
        for provider in failover_order {
            let state = self
                .providers
                .get(provider)
                .cloned()
                .unwrap_or(ProviderState::Active);
            result.push((provider.clone(), state));
            seen.insert(provider.clone());
        }

        // Then, emit any providers tracked in registry but not in failover_order.
        let mut extras: Vec<_> = self
            .providers
            .iter()
            .filter(|(p, _)| !seen.contains(*p))
            .map(|(p, s)| (p.clone(), s.clone()))
            .collect();
        extras.sort_by(|a, b| a.0.cmp(&b.0));
        result.extend(extras);

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn unknown_provider_is_active_by_default() {
        let reg = ProviderRegistry::default();
        assert!(reg.is_active("claude"));
        assert!(reg.is_active("cursor"));
    }

    #[test]
    fn blacklisted_provider_is_not_active() {
        let mut reg = ProviderRegistry::default();
        reg.blacklist("claude");
        assert!(!reg.is_active("claude"));
    }

    #[test]
    fn enable_restores_active_state() {
        let mut reg = ProviderRegistry::default();
        reg.blacklist("claude");
        reg.enable("claude");
        assert!(reg.is_active("claude"));
    }

    #[test]
    fn next_active_skips_blacklisted() {
        let mut reg = ProviderRegistry::default();
        reg.blacklist("claude");
        let order = vec!["claude".to_string(), "cursor".to_string(), "codex".to_string()];
        assert_eq!(reg.next_active(&order), Some("cursor"));
    }

    #[test]
    fn next_active_returns_none_when_all_blacklisted() {
        let mut reg = ProviderRegistry::default();
        reg.blacklist("claude");
        reg.blacklist("cursor");
        let order = vec!["claude".to_string(), "cursor".to_string()];
        assert_eq!(reg.next_active(&order), None);
    }

    #[test]
    fn next_active_with_empty_order_returns_none() {
        let reg = ProviderRegistry::default();
        assert_eq!(reg.next_active(&[]), None);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tmp();
        let mut reg = ProviderRegistry::default();
        reg.blacklist("claude");
        reg.enable("cursor");
        reg.save(dir.path()).unwrap();

        let loaded = ProviderRegistry::load(dir.path());
        assert!(!loaded.is_active("claude"));
        assert!(loaded.is_active("cursor"));
    }

    #[test]
    fn load_returns_default_when_file_absent() {
        let dir = tmp();
        let reg = ProviderRegistry::load(dir.path());
        assert!(reg.providers.is_empty());
    }

    #[test]
    fn all_states_respects_failover_order() {
        let mut reg = ProviderRegistry::default();
        reg.blacklist("claude");
        let order = vec!["claude".to_string(), "cursor".to_string(), "codex".to_string()];
        let states = reg.all_states(&order);
        assert_eq!(states.len(), 3);
        assert_eq!(states[0].0, "claude");
        assert_eq!(states[0].1, ProviderState::Blacklisted);
        assert_eq!(states[1].0, "cursor");
        assert_eq!(states[1].1, ProviderState::Active);
    }

    #[test]
    fn all_states_includes_extra_tracked_providers() {
        let mut reg = ProviderRegistry::default();
        reg.blacklist("extra-provider");
        let order = vec!["claude".to_string()];
        let states = reg.all_states(&order);
        // claude from order + extra-provider from registry
        assert_eq!(states.len(), 2);
        let names: Vec<&str> = states.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"claude"));
        assert!(names.contains(&"extra-provider"));
    }

    #[test]
    fn display_formats() {
        assert_eq!(ProviderState::Active.to_string(), "ACTIVE");
        assert_eq!(ProviderState::Blacklisted.to_string(), "BLACKLISTED");
    }
}
