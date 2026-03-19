use acs::spawner::Spawner;
use std::path::Path;

#[test]
fn test_spawner_new() {
    let spawner = Spawner::new(Path::new("/tmp/project"), "claude", "acs");
    assert_eq!(spawner.tool_path(), "acs");
}

#[test]
fn test_log_path_format() {
    let spawner = Spawner::new(Path::new("/tmp/project"), "claude", "acs");
    let log_path = spawner.log_path("w-0");
    assert_eq!(log_path, Path::new("/tmp/project/.acs/logs/w-0.log"));
}

#[test]
fn test_log_path_different_workers() {
    let spawner = Spawner::new(Path::new("/tmp/project"), "claude", "acs");
    let log0 = spawner.log_path("w-0");
    let log1 = spawner.log_path("w-1");
    assert_ne!(log0, log1);
    assert!(log0.to_str().unwrap().contains("w-0"));
    assert!(log1.to_str().unwrap().contains("w-1"));
}

#[test]
fn test_remove_worktree_nonexistent_is_ok() {
    let dir = tempfile::tempdir().unwrap();
    // Initialize a git repo so the command has a valid project dir
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let spawner = Spawner::new(dir.path(), "claude", "acs");
    // Removing a nonexistent worktree should not error (idempotent)
    let result = spawner.remove_worktree("nonexistent-worker");
    assert!(result.is_ok());
}

#[test]
fn test_kill_process_nonexistent_pid() {
    // Killing a PID that doesn't exist should succeed (process already gone)
    let result = Spawner::kill_process(999999);
    assert!(result.is_ok());
}

#[test]
fn test_tool_path_custom() {
    let spawner = Spawner::new(Path::new("/tmp"), "/usr/local/bin/claude", "/usr/local/bin/acs");
    assert_eq!(spawner.tool_path(), "/usr/local/bin/acs");
}
