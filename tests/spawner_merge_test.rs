use std::process::Command;
use std::path::Path;

use acs::spawner::Spawner;

/// Creates a temporary git repository with an initial commit, returning the tempdir.
fn setup_git_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();

    run_git(p, &["init", "-b", "main"]);
    run_git(p, &["config", "user.email", "test@test.com"]);
    run_git(p, &["config", "user.name", "Test"]);

    std::fs::write(p.join("README.md"), "# Test\n").unwrap();
    run_git(p, &["add", "."]);
    run_git(p, &["commit", "-m", "initial"]);

    dir
}

fn run_git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git command failed");
    assert!(
        status.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&status.stderr)
    );
}

fn spawner(dir: &Path) -> Spawner {
    Spawner::new(dir, "claude", "acs")
}

#[test]
fn test_find_branch_for_ticket() {
    let dir = setup_git_repo();
    let p = dir.path();

    // Create a branch matching the acs naming convention
    run_git(p, &["branch", "acs/t-001-abcd"]);

    let s = spawner(p);
    let branch = s.find_branch_for_ticket("t-001").unwrap();
    assert_eq!(branch, Some("acs/t-001-abcd".to_string()));

    // Non-existent ticket returns None
    let branch = s.find_branch_for_ticket("t-999").unwrap();
    assert_eq!(branch, None);
}

#[test]
fn test_merge_branch_success() {
    let dir = setup_git_repo();
    let p = dir.path();

    // Create a feature branch with a commit
    run_git(p, &["checkout", "-b", "acs/t-002-1234"]);
    std::fs::write(p.join("feature.txt"), "new feature\n").unwrap();
    run_git(p, &["add", "."]);
    run_git(p, &["commit", "-m", "add feature"]);

    // Go back to main
    run_git(p, &["checkout", "main"]);

    let s = spawner(p);
    let result = s.merge_branch("acs/t-002-1234", "main").unwrap();
    assert!(result, "merge should succeed");

    // Verify the file exists on main now
    assert!(p.join("feature.txt").exists());
}

#[test]
fn test_merge_branch_conflict() {
    let dir = setup_git_repo();
    let p = dir.path();

    // Create a feature branch with a conflicting change
    run_git(p, &["checkout", "-b", "acs/t-003-5678"]);
    std::fs::write(p.join("README.md"), "branch content\n").unwrap();
    run_git(p, &["add", "."]);
    run_git(p, &["commit", "-m", "branch change"]);

    // Go back to main and make a conflicting change
    run_git(p, &["checkout", "main"]);
    std::fs::write(p.join("README.md"), "main content\n").unwrap();
    run_git(p, &["add", "."]);
    run_git(p, &["commit", "-m", "main change"]);

    let s = spawner(p);
    let result = s.merge_branch("acs/t-003-5678", "main").unwrap();
    assert!(!result, "merge should fail due to conflict");

    // Verify main is still clean (merge was aborted)
    let content = std::fs::read_to_string(p.join("README.md")).unwrap();
    assert_eq!(content, "main content\n");
}

#[test]
fn test_delete_branch() {
    let dir = setup_git_repo();
    let p = dir.path();

    // Create and merge a branch, then delete it
    run_git(p, &["checkout", "-b", "acs/t-004-aaaa"]);
    std::fs::write(p.join("file.txt"), "content\n").unwrap();
    run_git(p, &["add", "."]);
    run_git(p, &["commit", "-m", "add file"]);
    run_git(p, &["checkout", "main"]);
    run_git(p, &["merge", "--no-ff", "acs/t-004-aaaa"]);

    let s = spawner(p);
    s.delete_branch("acs/t-004-aaaa");

    // Branch should no longer exist
    let branch = s.find_branch_for_ticket("t-004").unwrap();
    assert_eq!(branch, None);
}
