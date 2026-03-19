use acs::config::Config;
use std::io::Write;

#[test]
fn test_default_for_project_name() {
    let config = Config::default_for("my-project");
    assert_eq!(config.project.name, "my-project");
    assert_eq!(config.project.default_workers, 2);
}

#[test]
fn test_default_manager_config() {
    let config = Config::default_for("test");
    assert_eq!(config.manager.cycle_seconds, 15);
    assert_eq!(config.manager.worker_timeout_seconds, 300);
    assert_eq!(config.manager.worker_poll_seconds, 3);
}

#[test]
fn test_default_agent_config() {
    let config = Config::default_for("test");
    assert_eq!(config.agents.tool_path, "acs");
    assert_eq!(config.agents.claude_path, "claude");
}

#[test]
fn test_persona_for_domain_frontend() {
    let config = Config::default_for("test");
    assert_eq!(config.persona_for_domain("frontend"), "frontend-dev");
}

#[test]
fn test_persona_for_domain_backend() {
    let config = Config::default_for("test");
    assert_eq!(config.persona_for_domain("backend"), "backend-dev");
}

#[test]
fn test_persona_for_domain_devops() {
    let config = Config::default_for("test");
    assert_eq!(config.persona_for_domain("devops"), "devops");
}

#[test]
fn test_persona_for_domain_qa() {
    let config = Config::default_for("test");
    assert_eq!(config.persona_for_domain("qa"), "qa");
}

#[test]
fn test_persona_for_domain_infra() {
    let config = Config::default_for("test");
    assert_eq!(config.persona_for_domain("infra"), "devops");
}

#[test]
fn test_persona_for_domain_core() {
    let config = Config::default_for("test");
    assert_eq!(config.persona_for_domain("core"), "tech-lead");
}

#[test]
fn test_persona_for_domain_general() {
    let config = Config::default_for("test");
    assert_eq!(config.persona_for_domain("general"), "backend-dev");
}

#[test]
fn test_persona_for_domain_unknown_falls_back() {
    let config = Config::default_for("test");
    assert_eq!(config.persona_for_domain("unknown-domain"), "backend-dev");
}

#[test]
fn test_to_toml_roundtrip() {
    let config = Config::default_for("roundtrip-project");
    let toml_str = config.to_toml();

    // Parse back
    let parsed: Config = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.project.name, "roundtrip-project");
    assert_eq!(parsed.project.default_workers, 2);
    assert_eq!(parsed.manager.cycle_seconds, 15);
    assert_eq!(parsed.manager.worker_timeout_seconds, 300);
    assert_eq!(parsed.agents.tool_path, "acs");
    assert_eq!(parsed.agents.claude_path, "claude");
}

#[test]
fn test_config_load_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let config = Config::default_for("file-test");
    let mut f = std::fs::File::create(&config_path).unwrap();
    f.write_all(config.to_toml().as_bytes()).unwrap();

    let loaded = Config::load(&config_path).unwrap();
    assert_eq!(loaded.project.name, "file-test");
}

#[test]
fn test_config_load_missing_file_returns_error() {
    let result = Config::load(std::path::Path::new("/nonexistent/config.toml"));
    assert!(result.is_err());
}

#[test]
fn test_config_load_invalid_toml_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("bad.toml");
    std::fs::write(&config_path, "this is not valid toml {{{{").unwrap();

    let result = Config::load(&config_path);
    assert!(result.is_err());
}
