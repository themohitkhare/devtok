// src/config.rs
use anyhow::Result;
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
/// ```
#[derive(Debug, Clone)]
pub struct BackendsConfig {
    /// Default backend name (used when `--backend` is not specified and no
    /// domain mapping applies). Defaults to `"claude"`.
    pub default: String,
    /// Named backend definitions. Keys match the `--backend` flag value.
    pub definitions: HashMap<String, BackendTemplate>,
}

impl Default for BackendsConfig {
    fn default() -> Self {
        Self { default: "claude".into(), definitions: HashMap::new() }
    }
}

impl<'de> serde::Deserialize<'de> for BackendsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as DeError;

        // Deserialise as a raw TOML value map so we can handle the mixed
        // structure: a scalar "default" key alongside sub-tables for each
        // backend definition.
        let raw: HashMap<String, toml::Value> = HashMap::deserialize(deserializer)?;

        let default = raw
            .get("default")
            .and_then(|v| v.as_str())
            .unwrap_or("claude")
            .to_string();

        let mut definitions = HashMap::new();
        for (k, v) in &raw {
            if k == "default" {
                continue;
            }
            // Each backend value must be a TOML table
            let bt: BackendTemplate = v
                .clone()
                .try_into()
                .map_err(|e| DeError::custom(format!("backend '{}': {}", k, e)))?;
            definitions.insert(k.clone(), bt);
        }

        Ok(BackendsConfig { default, definitions })
    }
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
    m.insert("infra".into(), "devops".into());
    m.insert("core".into(), "tech-lead".into());
    m.insert("general".into(), "backend-dev".into());
    m
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self { cycle_seconds: default_cycle(), worker_timeout_seconds: default_timeout(), worker_poll_seconds: default_poll() }
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
            backends: HashMap::new(),
        }
    }

    pub fn persona_for_domain(&self, domain: &str) -> &str {
        self.personas.mapping.get(domain).map(|s| s.as_str()).unwrap_or("backend-dev")
    }

    pub fn to_toml(&self) -> String {
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
        let mut backend_names: Vec<&String> = self.backends.keys().collect();
        backend_names.sort(); // deterministic output
        for name in backend_names {
            let tpl = &self.backends[name];
            if !tpl.command.is_empty() {
                out.push_str(&format!(
                    "\n\n[backends.{}]\ncommand = \"{}\"",
                    name,
                    tpl.command.replace('\\', "\\\\").replace('"', "\\\""),
                ));
            }
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
        };
        let (prog, args) = t.expand("do the thing", "you are an AI").unwrap();
        assert_eq!(prog, "mytool");
        assert_eq!(args, vec!["--task", "do the thing", "--ctx", "you are an AI"]);
    }

    #[test]
    fn backend_template_expand_prompt_with_spaces() {
        let t = BackendTemplate {
            command: "echo {prompt}".to_string(),
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
        };
        let (prog, args) = t.expand("unused", "unused").unwrap();
        assert_eq!(prog, "echo");
        assert_eq!(args, vec!["static"]);
    }

    #[test]
    fn backend_template_expand_empty_returns_none() {
        let t = BackendTemplate { command: "".to_string() };
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

        assert_eq!(cfg.backends.len(), 2);
        assert!(cfg.backends.contains_key("my-claude"));
        assert!(cfg.backends.contains_key("custom"));

        let mc = &cfg.backends["my-claude"];
        assert!(mc.command.contains("{prompt}"));
        assert!(mc.command.contains("{system_prompt}"));

        let cu = &cfg.backends["custom"];
        assert!(cu.command.contains("{prompt}"));
    }

    #[test]
    fn to_toml_serializes_backends() {
        let mut cfg = Config::default_for("proj");
        cfg.backends.insert("my-backend".to_string(), BackendTemplate {
            command: "mytool {prompt} {system_prompt}".to_string(),
        });
        let toml_str = cfg.to_toml();
        assert!(toml_str.contains("[backends.my-backend]"), "missing backends section");
        assert!(toml_str.contains("command = \"mytool {prompt} {system_prompt}\""), "missing command");

        // Should round-trip
        let reparsed: Config = toml::from_str(&toml_str).expect("should parse generated TOML");
        assert_eq!(reparsed.backends["my-backend"].command, "mytool {prompt} {system_prompt}");
    }

    #[test]
    fn config_without_backends_section_defaults_to_empty_map() {
        let toml_content = "[project]\nname = \"minimal\"\n";
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", toml_content).unwrap();
        let cfg = Config::load(tmp.path()).expect("should load");
        assert!(cfg.backends.is_empty());
    }
}
