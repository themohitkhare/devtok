use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn acs_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_acs"))
}

fn run_acs(cwd: &Path, args: &[&str]) -> Output {
    Command::new(acs_bin())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("acs command should run")
}

fn run_acs_with_path(cwd: &Path, path_override: &str, args: &[&str]) -> Output {
    Command::new(acs_bin())
        .args(args)
        .env("PATH", path_override)
        .current_dir(cwd)
        .output()
        .expect("acs command should run")
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON")
}

fn setup_git_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    fs::write(repo.join("README.md"), "# test repo\n").unwrap();
    fs::create_dir(repo.join("src")).unwrap();
    fs::write(
        repo.join("src/lib.rs"),
        "pub fn hello() -> &'static str { \"hello\" }\n",
    )
    .unwrap();

    assert!(Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["config", "user.email", "qa@example.com"])
        .current_dir(repo)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["config", "user.name", "QA"])
        .current_dir(repo)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(repo)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["commit", "-m", "init", "--no-gpg-sign"])
        .current_dir(repo)
        .status()
        .unwrap()
        .success());

    dir
}

fn setup_acs_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let acs_dir = dir.path().join(".acs");
    fs::create_dir_all(acs_dir.join("logs")).unwrap();

    let _db = acs::db::Db::open(&acs_dir.join("project.db")).unwrap();
    let config = acs::config::Config::default_for("test-project");
    fs::write(acs_dir.join("config.toml"), config.to_toml()).unwrap();

    dir
}

fn setup_fake_claude_bin() -> (tempfile::TempDir, String) {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let bin_dir = tempfile::tempdir().unwrap();
    let bin_path = bin_dir.path();

    let shim = bin_path.join("claude");
    fs::write(
        &shim,
        r#"#!/bin/sh
set -eu
acs ticket create --title "Bootstrap ticket" --description "Created by fake bootstrap worker" --domain qa --priority 1 >/dev/null
"#,
    )
    .unwrap();

    let mut perms = fs::metadata(&shim).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&shim, perms).unwrap();
    symlink(acs_bin(), bin_path.join("acs")).unwrap();

    let inherited_path = std::env::var("PATH").unwrap_or_default();
    let path_override = format!("{}:{}", bin_path.display(), inherited_path);

    (bin_dir, path_override)
}

#[test]
fn init_with_spec_bootstraps_acs_and_creates_tickets() {
    let repo = setup_git_repo();
    let spec_path = repo.path().join("spec.md");
    fs::write(
        &spec_path,
        "# Spec\n\nBuild a tiny ACS test project with at least one QA task.\n",
    )
    .unwrap();

    let (_bin_dir, path_override) = setup_fake_claude_bin();
    let output = run_acs_with_path(
        repo.path(),
        &path_override,
        &["init", "--spec", spec_path.to_str().unwrap()],
    );
    assert_success(&output, "acs init --spec");

    assert!(repo.path().join(".acs").is_dir(), ".acs should be created");
    assert!(
        repo.path().join(".acs/project.db").is_file(),
        "project.db should be created"
    );
    assert!(
        repo.path().join(".acs/config.toml").is_file(),
        "config.toml should be created"
    );

    let tickets_output = run_acs(repo.path(), &["ticket", "list"]);
    assert_success(&tickets_output, "acs ticket list");
    let tickets = stdout_json(&tickets_output);
    let ticket_array = tickets
        .as_array()
        .expect("ticket list should be a JSON array");
    assert!(
        !ticket_array.is_empty(),
        "bootstrap should create at least one ticket"
    );
}

#[test]
fn ticket_list_returns_valid_json() {
    let dir = setup_acs_dir();

    let create_output = run_acs(
        dir.path(),
        &[
            "ticket",
            "create",
            "--title",
            "Test ticket",
            "--description",
            "A test ticket",
            "--domain",
            "qa",
            "--priority",
            "1",
        ],
    );
    assert_success(&create_output, "acs ticket create");

    let output = run_acs(dir.path(), &["ticket", "list"]);
    assert_success(&output, "acs ticket list");
    let tickets = stdout_json(&output);
    let ticket_array = tickets
        .as_array()
        .expect("ticket list should be a JSON array");
    assert_eq!(ticket_array.len(), 1, "expected one ticket");
    assert_eq!(ticket_array[0]["title"], "Test ticket");
}

#[test]
fn kb_write_then_read_roundtrips_correctly() {
    let dir = setup_acs_dir();

    let write_output = run_acs(
        dir.path(),
        &[
            "kb",
            "write",
            "--domain",
            "qa",
            "--key",
            "stack",
            "--value",
            "Rust, SQLite",
        ],
    );
    assert_success(&write_output, "acs kb write");

    let read_output = run_acs(
        dir.path(),
        &["kb", "read", "--domain", "qa", "--key", "stack"],
    );
    assert_success(&read_output, "acs kb read");
    let entry = stdout_json(&read_output);
    assert_eq!(entry["domain"], "qa");
    assert_eq!(entry["key"], "stack");
    assert_eq!(entry["value"], "Rust, SQLite");
}

#[test]
fn inbox_push_then_pop_roundtrips_correctly() {
    let dir = setup_acs_dir();

    let push_output = run_acs(
        dir.path(),
        &[
            "inbox",
            "push",
            "--recipient",
            "w-1",
            "--type",
            "test_msg",
            "--payload",
            r#"{"hello":"world"}"#,
            "--sender",
            "qa-test",
        ],
    );
    assert_success(&push_output, "acs inbox push");

    let pop_output = run_acs(dir.path(), &["inbox", "pop", "--agent", "w-1"]);
    assert_success(&pop_output, "acs inbox pop");
    let msg = stdout_json(&pop_output);
    assert_eq!(msg["recipient"], "w-1");
    assert_eq!(msg["msg_type"], "test_msg");
    assert_eq!(msg["sender"], "qa-test");
    assert_eq!(msg["payload"], r#"{"hello":"world"}"#);
}

#[test]
fn health_returns_valid_json_with_overall_field() {
    let dir = setup_acs_dir();

    let output = run_acs(dir.path(), &["health"]);
    assert_success(&output, "acs health");
    let report = stdout_json(&output);
    assert!(
        report.get("overall").and_then(Value::as_str).is_some(),
        "health report should contain an overall field"
    );
}
