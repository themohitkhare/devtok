// src/config.rs — MARKER_T017
use anyhow::{bail, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// A named backend definition — either a simple whitespace-split command
/// string (legacy) or a structured `command` + `args` list (new-style).
///
/// **New-style** (preferred — handles prompts with spaces correctly):
/// ```toml
/// [backends.claude]
/// command = "claude"
/// args = ["-p", "{prompt}", "--append-system-prompt", "{system_prompt}",
///         "--dangerously-skip-permissions", "--output-format", "json"]
/// cwd_in_worktree = true
/// output_format = "json"
/// ```
///
/// **Legacy** (whitespace-split, kept for backward compat):
/// ```toml
/// [backends.my-tool]
/// command = "mytool --task {prompt} --sys {system_prompt}"
/// ```
///
/// Placeholders recognised: `{prompt}`, `{system_prompt}`, `{model}`.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct BackendTemplate {
    /// Executable name or absolute path. In legacy mode this is the full
    /// whitespace-split command; in new-style mode only the binary name.
    pub command: String,
    /// Argument list (new-style). Each element may contain `{prompt}`,
    /// `{system_prompt}`, or `{model}` placeholders.
    #[serde(default)]
    pub args: Vec<String>,
    /// Set the child process working directory to the ticket worktree.
    /// Defaults to `true`.
    #[serde(default = "default_cwd_in_worktree")]
    pub cwd_in_worktree: bool,
    /// Output format hint: `"json"` for Claude-style JSONL, `"text"` otherwise.
    #[serde(default = "default_output_format")]
    pub output_format: String,
}

impl BackendTemplate {
    /// Return `(program, expanded_args)` ready for `std::process::Command`.
    ///
    /// * If `args` is non-empty, uses `command` as the program and expands
    ///   each arg (new-style, handles spaces in prompts correctly).
    /// * If `args` is empty, whitespace-splits `command` and expands tokens
    ///   (legacy mode).
    ///
    /// Returns `None` when `command` is empty/blank.
    pub fn expand(&self, prompt: &str, system_prompt: &str) -> Option<(String, Vec<String>)> {
        self.expand_with_model(prompt, system_prompt, None)
    }

    /// Like [`expand`] but also substitutes `{model}`.
    pub fn expand_with_model(
        &self,
        prompt: &str,
        system_prompt: &str,
        model: Option<&str>,
    ) -> Option<(String, Vec<String>)> {
        let command = self.command.trim().to_string();
        if command.is_empty() {
            return None;
        }

        if self.args.is_empty() {
            // Legacy: whitespace-split the command string
            let model_s = model.unwrap_or("");
            let tokens: Vec<String> = command
                .split_whitespace()
                .map(|tok| match tok {
                    "{prompt}" => prompt.to_string(),
                    "{system_prompt}" => system_prompt.to_string(),
                    "{model}" => model_s.to_string(),
                    other => other.to_string(),
                })
                .collect();
            let mut it = tokens.into_iter();
            let program = it.next()?;
            Some((program, it.collect()))
        } else {
            // New-style: command is the binary, args are expanded separately
            let model_s = model.unwrap_or("");
            let expanded = self
                .args
                .iter()
                .map(|a| {
                    a.replace("{prompt}", prompt)
                        .replace("{system_prompt}", system_prompt)
                        .replace("{model}", model_s)
                })
                .collect();
            Some((command, expanded))
        }
    }
}

fn default_cwd_in_worktree() -> bool { true }
fn default_output_format() -> String { "text".into() }

/// Quality / code-review settings loaded from `[quality]` in `acs.toml`.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct QualityConfig {
    /// When `true` the manager spawns a Tech Lead agent to review each
    /// `review_pending` ticket instead of auto-approving.
    #[serde(default)]
    pub code_review: bool,
}

/// Top-level `[backends]` configuration section.
///
/// ```toml
/// [backends]
/// default = "claude"
///
/// [backends.claude]
/// command = "claude"
/// args = ["-p", "{prompt}", "--append-system-prompt", "{system_prompt}",
///         "--dangerously-skip-permissions", "--output-format", "json"]
/// cwd_in_worktree = true
///
/// [backends.cursor]
/// command = "agent"
/// args = ["{prompt}"]
/// cwd_in_worktree = true
///
/// [backends.custom]
/// command = "/path/to/my-agent"
/// args = ["--task", "{prompt}", "--context", "{system_prompt}"]
/// cwd_in_worktree = true
///
/// # Domain routing: prefer specific backends for specific ticket domains
/// [backends.routing]
/// qa = "cursor"
/// core = "claude"
/// default = "claude"
/// ```
#[derive(Debug, Clone)]
pub struct BackendsConfig {
    /// Default backend name (used when `--backend` is not specified and no
    /// domain mapping applies). Defaults to `"claude"`.
    pub default: String,
    /// Named backend definitions. Keys match the `--backend` flag value.
    pub definitions: HashMap<String, BackendTemplate>,
    /// Domain → preferred backend name mapping.
    /// Used by the manager to route tickets to workers with the matching backend.
    /// A special `"default"` key within the routing table sets the fallback backend.
    /// Example: `{ "qa" → "cursor", "core" → "claude" }`
    pub routing: HashMap<String, String>,
    /// Provider failover order (t-090). Workers try providers in this order,
    /// skipping BLACKLISTED ones. Empty means no failover order is configured.
    /// Example: `['claude', 'cursor', 'codex']`
    pub failover_order: Vec<String>,
    /// Per-provider quota-error strings (t-090). When a worker log contains
    /// any of these strings for the active provider, it is treated as a quota
    /// exhaustion (not a generic rate-limit).
    /// Example: `{ claude = ['usage limit for Opus', 'Spend Limit'] }`
    pub quota_errors: HashMap<String, Vec<String>>,
}

impl BackendsConfig {
    /// Return the preferred backend name for a given ticket domain.
    /// Falls back to `routing["default"]`, then to `self.default`.
    pub fn backend_for_domain(&self, domain: &str) -> &str {
        if let Some(b) = self.routing.get(domain) {
            return b.as_str();
        }
        if let Some(b) = self.routing.get("default") {
            return b.as_str();
        }
        self.default.as_str()
    }
}

impl Default for BackendsConfig {
    fn default() -> Self {
        Self {
            default: "claude".into(),
            definitions: HashMap::new(),
            routing: HashMap::new(),
            failover_order: vec![],
            quota_errors: HashMap::new(),
        }
    }
}

impl<'de> serde::Deserialize<'de> for BackendsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as DeError;

        // Deserialise as a raw TOML value map so we can handle the mixed
        // structure: a scalar "default" key, a "routing" sub-table, and
        // named backend definition sub-tables.
        let raw: HashMap<String, toml::Value> = HashMap::deserialize(deserializer)?;

        let default = raw
            .get("default")
            .and_then(|v| v.as_str())
            .unwrap_or("claude")
            .to_string();

        // Extract domain → backend routing table if present.
        let routing: HashMap<String, String> = raw
            .get("routing")
            .and_then(|v| v.as_table())
            .map(|t| {
                t.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        // Extract provider failover order (t-090).
        let failover_order: Vec<String> = raw
            .get("failover_order")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        // Extract per-provider quota-error strings (t-090).
        let quota_errors: HashMap<String, Vec<String>> = raw
            .get("quota_errors")
            .and_then(|v| v.as_table())
            .map(|t| {
                t.iter()
                    .map(|(k, v)| {
                        let errors = v
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|s| s.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        (k.clone(), errors)
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut definitions = HashMap::new();
        for (k, v) in &raw {
            if k == "default" || k == "routing" || k == "failover_order" || k == "quota_errors" {
                continue;
            }
            // Each backend value must be a TOML table
            let bt: BackendTemplate = v
                .clone()
                .try_into()
                .map_err(|e| DeError::custom(format!("backend '{}': {}", k, e)))?;
            definitions.insert(k.clone(), bt);
        }

        Ok(BackendsConfig { default, definitions, routing, failover_order, quota_errors })
    }
}

/// Named run profile that overrides worker count, models, and timeouts.
///
/// Example `config.toml`:
/// ```toml
/// [profile.dev]
/// default_workers = 2
/// claude_models = ['claude-haiku-4-5', 'claude-sonnet-4-6', 'claude-sonnet-4-6']
/// worker_timeout_seconds = 3600
///
/// [profile.ci]
/// default_workers = 4
/// claude_models = ['claude-sonnet-4-6', 'claude-sonnet-4-6', 'claude-opus-4-6']
/// worker_timeout_seconds = 1800
/// auto_approve_milestones = true
///
/// [profile.prod]
/// default_workers = 8
/// claude_models = ['claude-sonnet-4-6', 'claude-opus-4-6', 'claude-opus-4-6']
/// worker_timeout_seconds = 7200
/// ```
#[derive(Debug, Deserialize, Clone, Default)]
pub struct ProfileConfig {
    pub default_workers: Option<usize>,
    pub claude_models: Option<Vec<String>>,
    pub worker_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub auto_approve_milestones: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub project: ProjectConfig,
    #[serde(default)]
    pub manager: ManagerConfig,
    #[serde(default)]
    pub personas: PersonaConfig,
    #[serde(default)]
    pub agents: AgentConfig,
    /// Structured backend configuration (new-style `[backends]` section).
    /// When present, overrides `[agents]` per-path fields for provider selection.
    #[serde(default)]
    pub backends: BackendsConfig,
    /// Code-quality / review settings (`[quality]` section).
    #[serde(default)]
    pub quality: QualityConfig,
    /// Named run profiles (`[profile.dev]`, `[profile.ci]`, `[profile.prod]`, …).
    #[serde(default)]
    pub profile: HashMap<String, ProfileConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProjectConfig {
    pub name: String,
    #[serde(default = "default_workers")]
    pub default_workers: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ManagerConfig {
    #[serde(default = "default_cycle")]
    pub cycle_seconds: u64,
    #[serde(default = "default_timeout")]
    pub worker_timeout_seconds: u64,
    #[serde(default = "default_poll")]
    pub worker_poll_seconds: u64,
    /// When `true`, milestones are automatically approved without waiting for
    /// CEO review (useful for CI runs).
    #[serde(default)]
    pub auto_approve_milestones: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PersonaConfig {
    #[serde(default = "default_persona_mapping")]
    pub mapping: HashMap<String, String>,
    /// Optional domain → backend mapping.
    /// Example: `{ frontend = "cursor", backend = "claude" }`
    /// Workers handling a ticket in a mapped domain will use the specified
    /// backend instead of the global default.
    #[serde(default)]
    pub domain_backends: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AgentConfig {
    #[serde(default = "default_tool_path")]
    pub tool_path: String,
    #[serde(default = "default_claude_path")]
    pub claude_path: String,
    /// Optional CodeX provider binary path (if empty, CodeX is disabled).
    #[serde(default = "default_empty_provider_path")]
    pub codex_path: String,
    /// Optional "agent" provider binary path (if empty, agent provider is disabled).
    #[serde(default = "default_empty_provider_path")]
    pub agent_path: String,
    /// Provider selection order (e.g. ["claude","codex","agent"]).
    /// If empty, workers infer providers from configured paths.
    #[serde(default)]
    pub providers: Vec<String>,

    /// Claude model offers (best -> cheaper), used for manager tiering.
    #[serde(default)]
    pub claude_models: Vec<String>,
    /// Codex model offers (best -> cheaper), used for manager tiering.
    #[serde(default)]
    pub codex_models: Vec<String>,
    /// Cursor Agent model offers (best -> cheaper), used for manager tiering.
    #[serde(default)]
    pub agent_models: Vec<String>,
}

fn default_workers() -> usize { 2 }
fn default_cycle() -> u64 { 15 }
fn default_timeout() -> u64 { 300 }
fn default_poll() -> u64 { 3 }
fn default_tool_path() -> String { "acs".into() }
fn default_claude_path() -> String { "claude".into() }
fn default_empty_provider_path() -> String { "".into() }

fn default_persona_mapping() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("frontend".into(), "frontend-dev".into());
    m.insert("backend".into(), "backend-dev".into());
    m.insert("devops".into(), "devops".into());
    m.insert("qa".into(), "qa".into());
    m.insert("qa-lead".into(), "qa-lead".into());
    m.insert("infra".into(), "devops".into());
    m.insert("core".into(), "tech-lead".into());
    m.insert("general".into(), "backend-dev".into());
    // New specialist personas
    m.insert("pm".into(), "pm".into());
    m.insert("management".into(), "senior-manager".into());
    m
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            cycle_seconds: default_cycle(),
            worker_timeout_seconds: default_timeout(),
            worker_poll_seconds: default_poll(),
            auto_approve_milestones: false,
        }
    }
}

impl Default for PersonaConfig {
    fn default() -> Self {
        Self { mapping: default_persona_mapping(), domain_backends: HashMap::new() }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            tool_path: default_tool_path(),
            claude_path: default_claude_path(),
            codex_path: default_empty_provider_path(),
            agent_path: default_empty_provider_path(),
            providers: vec![],
            claude_models: vec![],
            codex_models: vec![],
            agent_models: vec![],
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn default_for(project_name: &str) -> Self {
        Config {
            project: ProjectConfig { name: project_name.into(), default_workers: 2 },
            manager: ManagerConfig::default(),
            personas: PersonaConfig::default(),
            agents: AgentConfig::default(),
            backends: BackendsConfig::default(),
            quality: QualityConfig::default(),
            profile: HashMap::new(),
        }
    }

    /// Apply a named profile, overriding worker count, models, and timeout.
    ///
    /// Returns an error listing available profiles when the name is unknown.
    pub fn apply_profile(&mut self, profile_name: &str) -> Result<()> {
        let p = match self.profile.get(profile_name).cloned() {
            Some(p) => p,
            None => {
                let mut available: Vec<&str> =
                    self.profile.keys().map(|s| s.as_str()).collect();
                available.sort();
                if available.is_empty() {
                    bail!(
                        "Unknown profile '{}'. No profiles are defined in config.toml. \
                         Add [profile.<name>] sections to define profiles.",
                        profile_name
                    );
                } else {
                    bail!(
                        "Unknown profile '{}'. Available profiles: {}",
                        profile_name,
                        available.join(", ")
                    );
                }
            }
        };

        if let Some(w) = p.default_workers {
            self.project.default_workers = w;
        }
        if let Some(models) = p.claude_models {
            self.agents.claude_models = models;
        }
        if let Some(timeout) = p.worker_timeout_seconds {
            self.manager.worker_timeout_seconds = timeout;
        }
        if p.auto_approve_milestones {
            self.manager.auto_approve_milestones = true;
        }

        Ok(())
    }

    /// If `ANTHROPIC_MODEL` env var is set, override all claude model tiers
    /// with the single specified model.
    pub fn apply_anthropic_model_env(&mut self) {
        if let Ok(model) = std::env::var("ANTHROPIC_MODEL") {
            if !model.is_empty() {
                let n = self.agents.claude_models.len().max(3);
                self.agents.claude_models = vec![model; n];
            }
        }
    }

    pub fn persona_for_domain(&self, domain: &str) -> &str {
        self.personas.mapping.get(domain).map(|s| s.as_str()).unwrap_or("backend-dev")
    }

    pub fn to_toml(&self) -> String {
        fn escape_toml_string(s: &str) -> String {
            // Minimal TOML-string escaping for our usage (we emit "..." strings).
            s.replace('\\', "\\\\").replace('"', "\\\"")
        }

        let mut out = format!(
            r#"[project]
name = "{}"
default_workers = {}

[manager]
cycle_seconds = {}
worker_timeout_seconds = {}
worker_poll_seconds = {}

[agents]
tool_path = "{}"
claude_path = "{}"
"#,
            self.project.name,
            self.project.default_workers,
            self.manager.cycle_seconds,
            self.manager.worker_timeout_seconds,
            self.manager.worker_poll_seconds,
            self.agents.tool_path,
            self.agents.claude_path,
        );

        // Serialize [personas] (domain -> persona mapping, and optional
        // per-domain backend overrides).
        //
        // This is required so `acs init` produces a config.toml that users can
        // edit to customize which persona gets assigned to each ticket domain.
        let mut persona_keys: Vec<&String> = self.personas.mapping.keys().collect();
        persona_keys.sort();
        if !persona_keys.is_empty() {
            let parts = persona_keys
                .iter()
                .map(|k| {
                    let v = self.personas.mapping.get(*k).expect("key should exist");
                    format!(
                        "\"{}\" = \"{}\"",
                        escape_toml_string(k),
                        escape_toml_string(v)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("\n[personas]\nmapping = {{{}}}", parts));
        }

        let mut backend_keys: Vec<&String> = self.personas.domain_backends.keys().collect();
        backend_keys.sort();
        if !backend_keys.is_empty() {
            let parts = backend_keys
                .iter()
                .map(|k| {
                    let v = self.personas.domain_backends.get(*k).expect("key should exist");
                    format!(
                        "\"{}\" = \"{}\"",
                        escape_toml_string(k),
                        escape_toml_string(v)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");

            // If we already emitted a [personas] table for mapping above,
            // extend it with domain_backends. Otherwise create the table first.
            if out.contains("\n[personas]\n") || out.contains("\n[personas]") {
                out.push_str(&format!("\ndomain_backends = {{{}}}", parts));
            } else {
                out.push_str(&format!("\n[personas]\ndomain_backends = {{{}}}", parts));
            }
        }

        if !self.agents.codex_path.is_empty() {
            out.push_str(&format!("\ncodex_path = \"{}\"", self.agents.codex_path));
        }
        if !self.agents.agent_path.is_empty() {
            out.push_str(&format!("\nagent_path = \"{}\"", self.agents.agent_path));
        }
        if !self.agents.providers.is_empty() {
            let providers = self
                .agents
                .providers
                .iter()
                .map(|p| format!("\"{}\"", p))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("\nproviders = [{}]", providers));
        }

        if !self.agents.claude_models.is_empty() {
            let models = self
                .agents
                .claude_models
                .iter()
                .map(|m| format!("\"{}\"", m))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("\nclaude_models = [{}]", models));
        }
        if !self.agents.codex_models.is_empty() {
            let models = self
                .agents
                .codex_models
                .iter()
                .map(|m| format!("\"{}\"", m))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("\ncodex_models = [{}]", models));
        }
        if !self.agents.agent_models.is_empty() {
            let models = self
                .agents
                .agent_models
                .iter()
                .map(|m| format!("\"{}\"", m))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("\nagent_models = [{}]", models));
        }

        // Serialize [backends.*] sections
        let mut backend_names: Vec<&String> = self.backends.definitions.keys().collect();
        backend_names.sort(); // deterministic output
        for name in backend_names {
            let tpl = &self.backends.definitions[name];
            if !tpl.command.is_empty() {
                out.push_str(&format!(
                    "\n\n[backends.{}]\ncommand = \"{}\"",
                    name,
                    tpl.command.replace('\\', "\\\\").replace('"', "\\\""),
                ));
            }
        }

        // Serialize [quality] section (only when non-default)
        if self.quality.code_review {
            out.push_str("\n\n[quality]\ncode_review = true");
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn default_for_creates_valid_config() {
        let cfg = Config::default_for("my-project");
        assert_eq!(cfg.project.name, "my-project");
        assert_eq!(cfg.project.default_workers, 2);
        assert_eq!(cfg.manager.cycle_seconds, 15);
        assert_eq!(cfg.manager.worker_timeout_seconds, 300);
        assert_eq!(cfg.manager.worker_poll_seconds, 3);
        assert_eq!(cfg.agents.tool_path, "acs");
        assert_eq!(cfg.agents.claude_path, "claude");
        assert!(!cfg.personas.mapping.is_empty());
    }

    #[test]
    fn persona_for_domain_returns_correct_mappings() {
        let cfg = Config::default_for("test");
        assert_eq!(cfg.persona_for_domain("frontend"), "frontend-dev");
        assert_eq!(cfg.persona_for_domain("backend"), "backend-dev");
        assert_eq!(cfg.persona_for_domain("devops"), "devops");
        assert_eq!(cfg.persona_for_domain("qa"), "qa");
        assert_eq!(cfg.persona_for_domain("infra"), "devops");
        assert_eq!(cfg.persona_for_domain("core"), "tech-lead");
        assert_eq!(cfg.persona_for_domain("general"), "backend-dev");
    }

    #[test]
    fn persona_for_domain_unknown_falls_back_to_backend_dev() {
        let cfg = Config::default_for("test");
        assert_eq!(cfg.persona_for_domain("nonexistent"), "backend-dev");
        assert_eq!(cfg.persona_for_domain(""), "backend-dev");
    }

    #[test]
    fn persona_for_domain_returns_new_specialist_mappings() {
        let cfg = Config::default_for("test");
        assert_eq!(cfg.persona_for_domain("pm"), "pm");
        assert_eq!(cfg.persona_for_domain("management"), "senior-manager");
        assert_eq!(cfg.persona_for_domain("qa-lead"), "qa-lead");
    }

    #[test]
    fn to_toml_roundtrips() {
        let original = Config::default_for("roundtrip-proj");
        let toml_str = original.to_toml();
        let parsed: Config = toml::from_str(&toml_str).expect("should parse generated TOML");
        assert_eq!(parsed.project.name, original.project.name);
        assert_eq!(parsed.project.default_workers, original.project.default_workers);
        assert_eq!(parsed.manager.cycle_seconds, original.manager.cycle_seconds);
        assert_eq!(parsed.manager.worker_timeout_seconds, original.manager.worker_timeout_seconds);
        assert_eq!(parsed.manager.worker_poll_seconds, original.manager.worker_poll_seconds);
        assert_eq!(parsed.agents.tool_path, original.agents.tool_path);
        assert_eq!(parsed.agents.claude_path, original.agents.claude_path);
        assert_eq!(parsed.agents.claude_models, original.agents.claude_models);
        assert_eq!(parsed.agents.codex_models, original.agents.codex_models);
        assert_eq!(parsed.agents.agent_models, original.agents.agent_models);

        assert_eq!(parsed.personas.mapping, original.personas.mapping);
        assert_eq!(parsed.personas.domain_backends, original.personas.domain_backends);
    }

    #[test]
    fn config_load_parses_personas_mapping_and_domain_backends() {
        let toml_content = r#"
[project]
name = "file-test"
default_workers = 4

[manager]
cycle_seconds = 30
worker_timeout_seconds = 600
worker_poll_seconds = 5

[agents]
tool_path = "/usr/bin/acs"
claude_path = "/usr/bin/claude"

[personas]
mapping = { core = "pm", "qa-lead" = "qa-lead", management = "senior-manager" }
domain_backends = { core = "cursor" }
"#;

        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", toml_content).unwrap();

        let cfg = Config::load(tmp.path()).expect("should load config from file");
        assert_eq!(cfg.persona_for_domain("core"), "pm");
        assert_eq!(cfg.persona_for_domain("qa-lead"), "qa-lead");
        assert_eq!(cfg.persona_for_domain("management"), "senior-manager");

        assert_eq!(
            cfg.personas.domain_backends.get("core").map(|s| s.as_str()),
            Some("cursor")
        );
    }

    #[test]
    fn config_load_parses_toml_file() {
        let toml_content = r#"
[project]
name = "file-test"
default_workers = 4

[manager]
cycle_seconds = 30
worker_timeout_seconds = 600
worker_poll_seconds = 5

[agents]
tool_path = "/usr/bin/acs"
claude_path = "/usr/bin/claude"
"#;
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", toml_content).unwrap();

        let cfg = Config::load(tmp.path()).expect("should load config from file");
        assert_eq!(cfg.project.name, "file-test");
        assert_eq!(cfg.project.default_workers, 4);
        assert_eq!(cfg.manager.cycle_seconds, 30);
        assert_eq!(cfg.manager.worker_timeout_seconds, 600);
        assert_eq!(cfg.manager.worker_poll_seconds, 5);
        assert_eq!(cfg.agents.tool_path, "/usr/bin/acs");
        assert_eq!(cfg.agents.claude_path, "/usr/bin/claude");
    }

    #[test]
    fn config_load_with_minimal_toml_uses_defaults() {
        let toml_content = r#"
[project]
name = "minimal"
"#;
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", toml_content).unwrap();

        let cfg = Config::load(tmp.path()).expect("should load minimal config");
        assert_eq!(cfg.project.name, "minimal");
        assert_eq!(cfg.project.default_workers, 2);
        assert_eq!(cfg.manager.cycle_seconds, 15);
        assert_eq!(cfg.agents.tool_path, "acs");
        assert_eq!(cfg.persona_for_domain("frontend"), "frontend-dev");
    }

    #[test]
    fn config_load_fails_on_missing_file() {
        let result = Config::load(Path::new("/nonexistent/path/config.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn config_load_fails_on_invalid_toml() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "this is not valid toml {{{{").unwrap();

        let result = Config::load(tmp.path());
        assert!(result.is_err());
    }

    // ── BackendTemplate::expand ──────────────────────────────────────

    #[test]
    fn backend_template_expand_basic() {
        let t = BackendTemplate {
            command: "mytool --task {prompt} --ctx {system_prompt}".to_string(),
            ..Default::default()
        };
        let (prog, args) = t.expand("do the thing", "you are an AI").unwrap();
        assert_eq!(prog, "mytool");
        assert_eq!(args, vec!["--task", "do the thing", "--ctx", "you are an AI"]);
    }

    #[test]
    fn backend_template_expand_prompt_with_spaces() {
        let t = BackendTemplate {
            command: "echo {prompt}".to_string(),
            ..Default::default()
        };
        // The full prompt string (including spaces) replaces the {prompt} token as one arg.
        let (prog, args) = t.expand("hello world foo", "sys").unwrap();
        assert_eq!(prog, "echo");
        assert_eq!(args, vec!["hello world foo"]);
    }

    #[test]
    fn backend_template_expand_no_placeholders() {
        let t = BackendTemplate {
            command: "echo static".to_string(),
            ..Default::default()
        };
        let (prog, args) = t.expand("unused", "unused").unwrap();
        assert_eq!(prog, "echo");
        assert_eq!(args, vec!["static"]);
    }

    #[test]
    fn backend_template_expand_empty_returns_none() {
        let t = BackendTemplate { command: "".to_string(), ..Default::default() };
        assert!(t.expand("p", "s").is_none());
    }

    // ── [backends] config round-trip ────────────────────────────────

    #[test]
    fn backends_round_trip_via_toml() {
        let toml_content = r#"
[project]
name = "backend-test"

[backends.my-claude]
command = "claude -p {prompt} --append-system-prompt {system_prompt}"

[backends.custom]
command = "/usr/local/bin/myai --task {prompt}"
"#;
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", toml_content).unwrap();
        let cfg = Config::load(tmp.path()).expect("should parse");

        assert_eq!(cfg.backends.definitions.len(), 2);
        assert!(cfg.backends.definitions.contains_key("my-claude"));
        assert!(cfg.backends.definitions.contains_key("custom"));

        let mc = &cfg.backends.definitions["my-claude"];
        assert!(mc.command.contains("{prompt}"));
        assert!(mc.command.contains("{system_prompt}"));

        let cu = &cfg.backends.definitions["custom"];
        assert!(cu.command.contains("{prompt}"));
    }

    #[test]
    fn to_toml_serializes_backends() {
        let mut cfg = Config::default_for("proj");
        cfg.backends.definitions.insert("my-backend".to_string(), BackendTemplate {
            command: "mytool {prompt} {system_prompt}".to_string(),
            ..Default::default()
        });
        let toml_str = cfg.to_toml();
        assert!(toml_str.contains("[backends.my-backend]"), "missing backends section");
        assert!(toml_str.contains("command = \"mytool {prompt} {system_prompt}\""), "missing command");

        // Should round-trip
        let reparsed: Config = toml::from_str(&toml_str).expect("should parse generated TOML");
        assert_eq!(reparsed.backends.definitions["my-backend"].command, "mytool {prompt} {system_prompt}");
    }

    #[test]
    fn config_without_backends_section_defaults_to_empty_map() {
        let toml_content = "[project]\nname = \"minimal\"\n";
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", toml_content).unwrap();
        let cfg = Config::load(tmp.path()).expect("should load");
        assert!(cfg.backends.definitions.is_empty());
    }

    // -----------------------------------------------------------------------
    // QualityConfig tests
    // -----------------------------------------------------------------------

    #[test]
    fn quality_config_defaults_to_no_code_review() {
        let cfg = Config::default_for("test-project");
        assert!(!cfg.quality.code_review, "code_review should default to false");
    }

    #[test]
    fn quality_config_code_review_can_be_enabled_via_toml() {
        let toml_content = r#"
[project]
name = "review-enabled"

[quality]
code_review = true
"#;
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", toml_content).unwrap();

        let cfg = Config::load(tmp.path()).expect("should parse quality config");
        assert!(cfg.quality.code_review, "code_review should be enabled");
    }

    #[test]
    fn quality_config_false_when_section_absent() {
        let toml_content = "[project]\nname = \"no-quality\"\n";
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", toml_content).unwrap();

        let cfg = Config::load(tmp.path()).expect("should load without quality section");
        assert!(!cfg.quality.code_review);
    }

    // ── Profile tests ────────────────────────────────────────────────

    #[test]
    fn config_load_parses_profile_sections() {
        let toml_content = r#"
[project]
name = "profile-test"

[profile.dev]
default_workers = 2
claude_models = ["claude-haiku-4-5", "claude-sonnet-4-6", "claude-sonnet-4-6"]
worker_timeout_seconds = 3600

[profile.ci]
default_workers = 4
claude_models = ["claude-sonnet-4-6", "claude-sonnet-4-6", "claude-opus-4-6"]
worker_timeout_seconds = 1800
auto_approve_milestones = true

[profile.prod]
default_workers = 8
claude_models = ["claude-sonnet-4-6", "claude-opus-4-6", "claude-opus-4-6"]
worker_timeout_seconds = 7200
"#;
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", toml_content).unwrap();
        let cfg = Config::load(tmp.path()).expect("should parse profiles");

        assert_eq!(cfg.profile.len(), 3);

        let dev = &cfg.profile["dev"];
        assert_eq!(dev.default_workers, Some(2));
        assert_eq!(dev.worker_timeout_seconds, Some(3600));
        assert!(!dev.auto_approve_milestones);

        let ci = &cfg.profile["ci"];
        assert_eq!(ci.default_workers, Some(4));
        assert_eq!(ci.worker_timeout_seconds, Some(1800));
        assert!(ci.auto_approve_milestones);

        let prod = &cfg.profile["prod"];
        assert_eq!(prod.default_workers, Some(8));
        assert_eq!(prod.worker_timeout_seconds, Some(7200));
    }

    #[test]
    fn apply_profile_overrides_config_fields() {
        let toml_content = r#"
[project]
name = "apply-test"
default_workers = 1

[manager]
worker_timeout_seconds = 300

[profile.ci]
default_workers = 4
claude_models = ["claude-sonnet-4-6", "claude-sonnet-4-6", "claude-opus-4-6"]
worker_timeout_seconds = 1800
auto_approve_milestones = true
"#;
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", toml_content).unwrap();
        let mut cfg = Config::load(tmp.path()).expect("should load");

        cfg.apply_profile("ci").expect("ci profile should apply");

        assert_eq!(cfg.project.default_workers, 4);
        assert_eq!(cfg.manager.worker_timeout_seconds, 1800);
        assert!(cfg.manager.auto_approve_milestones);
        assert_eq!(
            cfg.agents.claude_models,
            vec!["claude-sonnet-4-6", "claude-sonnet-4-6", "claude-opus-4-6"]
        );
    }

    #[test]
    fn apply_profile_unknown_returns_error_with_available_list() {
        let mut cfg = Config::default_for("test");
        cfg.profile
            .insert("dev".to_string(), ProfileConfig { default_workers: Some(2), ..Default::default() });
        cfg.profile
            .insert("prod".to_string(), ProfileConfig { default_workers: Some(8), ..Default::default() });

        let err = cfg.apply_profile("nonexistent").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("nonexistent"), "error should mention the unknown profile name");
        assert!(
            msg.contains("dev") || msg.contains("prod"),
            "error should list available profiles: {}",
            msg
        );
    }

    #[test]
    fn apply_profile_no_profiles_defined_gives_helpful_error() {
        let mut cfg = Config::default_for("test");
        let err = cfg.apply_profile("ci").unwrap_err();
        assert!(
            err.to_string().contains("No profiles are defined"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn apply_profile_dev_default_applies_workers_and_timeout() {
        let mut cfg = Config::default_for("test");
        cfg.profile.insert(
            "dev".to_string(),
            ProfileConfig {
                default_workers: Some(3),
                worker_timeout_seconds: Some(999),
                ..Default::default()
            },
        );
        cfg.apply_profile("dev").expect("dev profile should apply");
        assert_eq!(cfg.project.default_workers, 3);
        assert_eq!(cfg.manager.worker_timeout_seconds, 999);
    }

    #[test]
    fn apply_anthropic_model_env_overrides_all_tiers() {
        let mut cfg = Config::default_for("test");
        cfg.agents.claude_models = vec![
            "claude-haiku-4-5".to_string(),
            "claude-sonnet-4-6".to_string(),
            "claude-opus-4-6".to_string(),
        ];
        std::env::set_var("ANTHROPIC_MODEL", "claude-opus-4-6");
        cfg.apply_anthropic_model_env();
        std::env::remove_var("ANTHROPIC_MODEL");

        assert_eq!(cfg.agents.claude_models.len(), 3);
        assert!(cfg.agents.claude_models.iter().all(|m| m == "claude-opus-4-6"));
    }

    #[test]
    fn to_toml_includes_quality_section_when_code_review_enabled() {
        let mut cfg = Config::default_for("proj");
        cfg.quality.code_review = true;
        let toml_str = cfg.to_toml();
        assert!(
            toml_str.contains("[quality]"),
            "serialized TOML should include [quality] section"
        );
        assert!(
            toml_str.contains("code_review = true"),
            "serialized TOML should include code_review = true"
        );

        // And it should round-trip
        let reparsed: Config = toml::from_str(&toml_str).expect("should re-parse");
        assert!(reparsed.quality.code_review);
    }

    // -----------------------------------------------------------------------
    // BackendsConfig routing tests
    // -----------------------------------------------------------------------

    #[test]
    fn backends_routing_parses_domain_to_backend_map() {
        let toml_content = r#"
[project]
name = "routing-test"

[backends.routing]
qa = "cursor"
core = "claude"
default = "claude"
"#;
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", toml_content).unwrap();
        let cfg = Config::load(tmp.path()).expect("should load");
        assert_eq!(cfg.backends.routing.get("qa").map(|s| s.as_str()), Some("cursor"));
        assert_eq!(cfg.backends.routing.get("core").map(|s| s.as_str()), Some("claude"));
        assert_eq!(cfg.backends.routing.get("default").map(|s| s.as_str()), Some("claude"));
    }

    #[test]
    fn backend_for_domain_returns_mapped_backend() {
        let mut cfg = Config::default_for("proj");
        cfg.backends.routing.insert("qa".to_string(), "cursor".to_string());
        cfg.backends.routing.insert("core".to_string(), "claude".to_string());
        assert_eq!(cfg.backends.backend_for_domain("qa"), "cursor");
        assert_eq!(cfg.backends.backend_for_domain("core"), "claude");
    }

    #[test]
    fn backend_for_domain_falls_back_to_routing_default() {
        let mut cfg = Config::default_for("proj");
        cfg.backends.routing.insert("default".to_string(), "codex".to_string());
        assert_eq!(cfg.backends.backend_for_domain("unknown-domain"), "codex");
    }

    #[test]
    fn backend_for_domain_falls_back_to_backends_default() {
        let cfg = Config::default_for("proj");
        // No routing configured — falls back to backends.default = "claude"
        assert_eq!(cfg.backends.backend_for_domain("anything"), "claude");
    }

    #[test]
    fn backends_routing_does_not_pollute_definitions() {
        let toml_content = r#"
[project]
name = "routing-test"

[backends.routing]
qa = "cursor"

[backends.claude]
command = "claude"
args = ["-p", "{prompt}"]
"#;
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", toml_content).unwrap();
        let cfg = Config::load(tmp.path()).expect("should load");
        // "routing" must not appear as a backend definition
        assert!(!cfg.backends.definitions.contains_key("routing"));
        assert!(cfg.backends.definitions.contains_key("claude"));
    }
}
