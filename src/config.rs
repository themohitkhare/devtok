// src/config.rs
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub project: ProjectConfig,
    #[serde(default)]
    pub manager: ManagerConfig,
    #[serde(default)]
    pub personas: PersonaConfig,
    #[serde(default)]
    pub agents: AgentConfig,
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
        Self { mapping: default_persona_mapping() }
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
}
