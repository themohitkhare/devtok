// src/config.rs
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub project: ProjectConfig,
    #[serde(default)]
    pub manager: ManagerConfig,
    #[serde(default)]
    pub personas: PersonaConfig,
    #[serde(default)]
    pub agents: AgentConfig,
}

#[derive(Debug, Deserialize)]
pub struct ProjectConfig {
    pub name: String,
    #[serde(default = "default_workers")]
    pub default_workers: usize,
}

#[derive(Debug, Deserialize)]
pub struct ManagerConfig {
    #[serde(default = "default_cycle")]
    pub cycle_seconds: u64,
    #[serde(default = "default_timeout")]
    pub worker_timeout_seconds: u64,
    #[serde(default = "default_poll")]
    pub worker_poll_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub struct PersonaConfig {
    #[serde(default = "default_persona_mapping")]
    pub mapping: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_tool_path")]
    pub tool_path: String,
    #[serde(default = "default_claude_path")]
    pub claude_path: String,
}

fn default_workers() -> usize { 2 }
fn default_cycle() -> u64 { 15 }
fn default_timeout() -> u64 { 300 }
fn default_poll() -> u64 { 3 }
fn default_tool_path() -> String { "acs".into() }
fn default_claude_path() -> String { "claude".into() }

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
        Self { tool_path: default_tool_path(), claude_path: default_claude_path() }
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
        format!(
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
            self.project.name, self.project.default_workers,
            self.manager.cycle_seconds, self.manager.worker_timeout_seconds, self.manager.worker_poll_seconds,
            self.agents.tool_path, self.agents.claude_path,
        )
    }
}
