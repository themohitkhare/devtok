use std::process::Command;
use std::fs;

fn acs_bin() -> String {
    // Build first, then get the binary path
    let output = Command::new("cargo")
        .args(["build"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo build failed");
    assert!(output.status.success(), "cargo build failed: {}", String::from_utf8_lossy(&output.stderr));

    // The binary is at target/debug/acs
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/target/debug/acs", manifest_dir)
}

fn setup_test_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    // Create .acs directory and DB
    let acs_dir = dir.path().join(".acs");
    fs::create_dir_all(acs_dir.join("logs")).unwrap();

    // Initialize DB
    let _db = acs::db::Db::open(&acs_dir.join("project.db")).unwrap();

    // Write a minimal config
    let config = acs::config::Config::default_for("test-project");
    fs::write(acs_dir.join("config.toml"), config.to_toml()).unwrap();

    dir
}

#[test]
fn test_acs_help() {
    let bin = acs_bin();
    let output = Command::new(&bin)
        .args(["--help"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("init"), "help should mention init");
    assert!(stdout.contains("run"), "help should mention run");
    assert!(stdout.contains("status"), "help should mention status");
    assert!(stdout.contains("ticket"), "help should mention ticket");
}

#[test]
fn test_ticket_roundtrip() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    // Create a ticket
    let output = Command::new(&bin)
        .args(["ticket", "create", "--title", "Test ticket", "--description", "A test ticket", "--domain", "backend", "--priority", "1"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "ticket create failed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("t-001"), "should contain ticket id");

    // List tickets
    let output = Command::new(&bin)
        .args(["ticket", "list"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Test ticket"));

    // Show ticket
    let output = Command::new(&bin)
        .args(["ticket", "show", "t-001"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("backend"));

    // Update ticket
    let output = Command::new(&bin)
        .args(["ticket", "update", "--id", "t-001", "--status", "in_progress"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn test_kb_roundtrip() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    // Write to KB
    let output = Command::new(&bin)
        .args(["kb", "write", "--domain", "backend", "--key", "stack", "--value", "Rust, SQLite"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    // Read from KB
    let output = Command::new(&bin)
        .args(["kb", "read", "--domain", "backend", "--key", "stack"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Rust, SQLite"));
}

#[test]
fn test_inbox_roundtrip() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    // Push message
    let output = Command::new(&bin)
        .args(["inbox", "push", "--to", "w-0", "--type", "test_msg", "--payload", r#"{"hello":"world"}"#])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "inbox push failed: {}", String::from_utf8_lossy(&output.stderr));

    // Pop message
    let output = Command::new(&bin)
        .args(["inbox", "pop", "--agent", "w-0"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("test_msg"));
}

#[test]
fn test_status_on_empty_project() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    let output = Command::new(&bin)
        .args(["status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "status failed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Tickets: 0"));
}

#[test]
fn test_errors_are_json() {
    let bin = acs_bin();
    // Run in a temp dir without .acs/ — should produce a JSON error
    let dir = tempfile::tempdir().unwrap();

    let output = Command::new(&bin)
        .args(["status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success(), "should fail without .acs/");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed: serde_json::Value = serde_json::from_str(stderr.trim())
        .expect("error output should be valid JSON");
    assert!(parsed["error"].is_string(), "should have an 'error' field");
    assert!(parsed["error"].as_str().unwrap().contains(".acs/ not found"));
}

#[test]
fn test_ticket_not_found_is_json_error() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    let output = Command::new(&bin)
        .args(["ticket", "show", "t-999"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed: serde_json::Value = serde_json::from_str(stderr.trim())
        .expect("error output should be valid JSON");
    assert!(parsed["error"].as_str().unwrap().contains("not found"));
}

#[test]
fn test_kb_not_found_is_json_error() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    let output = Command::new(&bin)
        .args(["kb", "read", "--domain", "x", "--key", "y"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed: serde_json::Value = serde_json::from_str(stderr.trim())
        .expect("error output should be valid JSON");
    assert!(parsed["error"].as_str().unwrap().contains("not found"));
}
