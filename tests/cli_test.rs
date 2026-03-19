use std::process::Command;
use std::fs;

fn acs_bin() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let output = Command::new("cargo")
        .args(["build"])
        .current_dir(manifest_dir)
        .output()
        .expect("cargo build failed");
    assert!(output.status.success(), "cargo build failed: {}", String::from_utf8_lossy(&output.stderr));
    format!("{}/target/debug/acs", manifest_dir)
}

fn setup_test_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let acs_dir = dir.path().join(".acs");
    fs::create_dir_all(acs_dir.join("logs")).unwrap();
    let _db = acs::db::Db::open(&acs_dir.join("project.db")).unwrap();
    let config = acs::config::Config::default_for("test-project");
    fs::write(acs_dir.join("config.toml"), config.to_toml()).unwrap();
    dir
}

// --- Ticket CLI tests ---

#[test]
fn test_ticket_create_returns_json() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    let output = Command::new(&bin)
        .args(["ticket", "create", "--title", "Test", "--description", "Desc", "--domain", "backend", "--priority", "1"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["status"], "created");
    assert!(json["id"].as_str().unwrap().starts_with("t-"));
}

#[test]
fn test_ticket_list_returns_json_array() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    // Create some tickets
    Command::new(&bin)
        .args(["ticket", "create", "--title", "T1", "--description", "D1", "--domain", "backend", "--priority", "1"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = Command::new(&bin)
        .args(["ticket", "list"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(json.is_array());
    assert_eq!(json.as_array().unwrap().len(), 1);
}

#[test]
fn test_ticket_list_with_status_filter_json() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    Command::new(&bin)
        .args(["ticket", "create", "--title", "T1", "--description", "D1", "--domain", "backend", "--priority", "1"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = Command::new(&bin)
        .args(["ticket", "list", "--status", "completed"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(json.is_array());
    assert!(json.as_array().unwrap().is_empty());
}

#[test]
fn test_ticket_update_returns_json() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    Command::new(&bin)
        .args(["ticket", "create", "--title", "T1", "--description", "D1", "--domain", "backend", "--priority", "1"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = Command::new(&bin)
        .args(["ticket", "update", "--id", "t-001", "--status", "in_progress"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["status"], "updated");
}

#[test]
fn test_ticket_show_nonexistent_returns_error() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    let output = Command::new(&bin)
        .args(["ticket", "show", "t-999"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

// --- KB CLI tests ---

#[test]
fn test_kb_write_returns_json() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    let output = Command::new(&bin)
        .args(["kb", "write", "--domain", "backend", "--key", "stack", "--value", "Rust"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["status"], "written");
}

#[test]
fn test_kb_read_nonexistent_returns_error() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    let output = Command::new(&bin)
        .args(["kb", "read", "--domain", "backend", "--key", "nonexistent"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

// --- Inbox CLI tests ---

#[test]
fn test_inbox_push_returns_json() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    let output = Command::new(&bin)
        .args(["inbox", "push", "--to", "w-0", "--type", "test", "--payload", "data"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["status"], "pushed");
}

#[test]
fn test_inbox_pop_empty_returns_json() {
    let bin = acs_bin();
    let dir = setup_test_dir();

    let output = Command::new(&bin)
        .args(["inbox", "pop", "--agent", "w-0"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["status"], "empty");
}

// --- Missing .acs directory tests ---

#[test]
fn test_status_without_acs_dir_errors() {
    let bin = acs_bin();
    let dir = tempfile::tempdir().unwrap();

    let output = Command::new(&bin)
        .args(["status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(".acs/") || stderr.contains("not found"));
}

#[test]
fn test_log_without_acs_dir_errors() {
    let bin = acs_bin();
    let dir = tempfile::tempdir().unwrap();

    let output = Command::new(&bin)
        .args(["log"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(".acs/") || stderr.contains("not found"));
}

#[test]
fn test_ticket_without_acs_dir_errors() {
    let bin = acs_bin();
    let dir = tempfile::tempdir().unwrap();

    let output = Command::new(&bin)
        .args(["ticket", "list"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
}
