use std::fs;
use std::process::Command;

fn acs_bin() -> String {
    format!("{}", env!("CARGO_BIN_EXE_acs"))
}

fn setup_test_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let acs_dir = dir.path().join(".acs");
    fs::create_dir_all(acs_dir.join("logs")).expect("create logs dir");
    let db = acs::db::Db::open(&acs_dir.join("project.db")).expect("open db");
    let config = acs::config::Config::default_for("test-project");
    fs::write(acs_dir.join("config.toml"), config.to_toml()).expect("write config");

    db.create_ticket("Closed ticket", "Already done", "core", 1)
        .expect("create ticket");
    db.update_ticket("t-001", "completed", None, Some("w-1"), None)
        .expect("complete ticket");
    db.write_knowledge("architecture", "ADR-101", "Prefer SQLite WAL")
        .expect("write adr");
    db.write_knowledge("general", "stack", "Rust + Clap")
        .expect("write kb");

    dir
}

#[test]
fn test_export_json_contains_required_top_level_sections() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    let output = Command::new(&bin)
        .args(["export", "--format", "json"])
        .current_dir(dir.path())
        .output()
        .expect("run export json");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid json");

    assert!(json.get("tickets").is_some());
    assert!(json.get("merged_branches").is_some());
    assert!(json.get("knowledge_by_domain").is_some());
    assert!(json.get("token_usage").is_some());
    assert!(json.get("architecture_decisions").is_some());
}

#[test]
fn test_export_markdown_out_starts_with_expected_header() {
    let bin = acs_bin();
    let dir = setup_test_dir();
    let out_path = dir.path().join("handoff.md");

    let output = Command::new(&bin)
        .args([
            "export",
            "--format",
            "markdown",
            "--out",
            out_path.to_str().expect("path to str"),
        ])
        .current_dir(dir.path())
        .output()
        .expect("run export markdown");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let content = fs::read_to_string(&out_path).expect("read markdown output");
    assert!(content.starts_with("# ACS Project Summary Export"));
}

#[test]
fn adr_extraction_is_case_insensitive() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    let db = acs::db::Db::open(&dir.path().join(".acs").join("project.db")).expect("open db");
    db.write_knowledge("architecture", "Decision-Auth", "Use bearer tokens")
        .expect("write decision key");

    let output = Command::new(&bin)
        .args(["export", "--format", "json"])
        .current_dir(dir.path())
        .output()
        .expect("run export json");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid json");
    let adrs = json["architecture_decisions"]
        .as_array()
        .expect("architecture_decisions array");
    assert!(adrs.iter().any(|entry| entry["key"] == "ADR-101"));
    assert!(adrs
        .iter()
        .any(|entry| entry["key"] == "Decision-Auth"));
}
