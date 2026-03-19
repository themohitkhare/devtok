use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::db::Db;

/// Result of a cleanup run, for reporting.
pub struct CleanupReport {
    pub branches_found: Vec<String>,
    pub branches_deleted: Vec<String>,
    pub worktrees_removed: Vec<String>,
    pub pruned: bool,
}

/// Run the full cleanup sequence:
/// 1. List all acs/* branches
/// 2. Delete branches whose tickets are completed
/// 3. Remove orphaned worktrees in .acs/worktrees/
/// 4. Prune git worktree list
pub fn run_cleanup(project_dir: &Path, db: &Db) -> Result<CleanupReport> {
    let acs_branches = list_acs_branches(project_dir)?;

    let mut deleted = Vec::new();
    for branch in &acs_branches {
        if let Some(ticket_id) = extract_ticket_id(branch) {
            if is_ticket_completed(db, &ticket_id)? {
                delete_branch(project_dir, branch);
                deleted.push(branch.clone());
            }
        }
    }

    let removed_worktrees = remove_orphaned_worktrees(project_dir)?;
    prune_worktrees(project_dir)?;

    Ok(CleanupReport {
        branches_found: acs_branches,
        branches_deleted: deleted,
        worktrees_removed: removed_worktrees,
        pruned: true,
    })
}

/// List all local branches matching `acs/*`.
fn list_acs_branches(project_dir: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["branch", "--list", "acs/*"])
        .current_dir(project_dir)
        .output()
        .with_context(|| "Failed to run git branch --list")?;

    if !output.status.success() {
        anyhow::bail!("git branch --list acs/* failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let branches: Vec<String> = stdout
        .lines()
        .map(|l| l.trim().trim_start_matches("* ").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    Ok(branches)
}

/// Extract the ticket ID from a branch name like `acs/t-007-a1b2`.
/// Returns the ticket ID portion (e.g. `t-007`).
fn extract_ticket_id(branch: &str) -> Option<String> {
    // Branch format: acs/{ticket_id}-{4hex}
    let rest = branch.strip_prefix("acs/")?;
    // ticket_id is everything up to the last hyphen-followed-by-4-hex-chars
    // e.g. "t-007-a1b2" -> we want "t-007"
    // Find the last '-' where everything after it is exactly 4 hex digits
    let last_dash = rest.rfind('-')?;
    let suffix = &rest[last_dash + 1..];
    if suffix.len() == 4 && suffix.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(rest[..last_dash].to_string())
    } else {
        // Maybe a branch without the random suffix — use the full rest as ticket id
        Some(rest.to_string())
    }
}

/// Check if a ticket exists and has status "completed".
fn is_ticket_completed(db: &Db, ticket_id: &str) -> Result<bool> {
    match db.get_ticket(ticket_id)? {
        Some(ticket) => Ok(ticket.status == "completed"),
        // If the ticket doesn't exist, the branch is orphaned — safe to clean
        None => Ok(true),
    }
}

/// Delete a local git branch (best-effort, using -D to force).
fn delete_branch(project_dir: &Path, branch: &str) {
    let _ = Command::new("git")
        .args(["branch", "-D", branch])
        .current_dir(project_dir)
        .status();
}

/// Scan `.acs/worktrees/` for directories that are not registered as active
/// git worktrees, and remove them.
fn remove_orphaned_worktrees(project_dir: &Path) -> Result<Vec<String>> {
    let worktrees_dir = project_dir.join(".acs").join("worktrees");
    if !worktrees_dir.exists() {
        return Ok(vec![]);
    }

    // Get the list of active worktrees from git
    let active = list_active_worktree_paths(project_dir)?;

    let mut removed = Vec::new();
    let entries = std::fs::read_dir(&worktrees_dir)
        .with_context(|| format!("Failed to read {}", worktrees_dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if !active.contains(&canonical) {
            // Try git worktree remove first, fall back to rm
            let status = Command::new("git")
                .args(["worktree", "remove", "--force"])
                .arg(&path)
                .current_dir(project_dir)
                .status();

            match status {
                Ok(s) if s.success() => {}
                _ => {
                    let _ = std::fs::remove_dir_all(&path);
                }
            }

            let name = entry.file_name().to_string_lossy().to_string();
            removed.push(name);
        }
    }

    Ok(removed)
}

/// Get canonical paths of all active git worktrees.
fn list_active_worktree_paths(project_dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(project_dir)
        .output()
        .with_context(|| "Failed to run git worktree list")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<std::path::PathBuf> = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(|p| {
            std::fs::canonicalize(p).unwrap_or_else(|_| std::path::PathBuf::from(p))
        })
        .collect();

    Ok(paths)
}

/// Run `git worktree prune` to clean up stale worktree references.
fn prune_worktrees(project_dir: &Path) -> Result<()> {
    Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(project_dir)
        .status()
        .with_context(|| "Failed to run git worktree prune")?;
    Ok(())
}

/// Execute the `acs cleanup` CLI command.
pub fn execute() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let project_dir = acs_dir
        .parent()
        .context("Expected `.acs/` to be inside a project directory")?
        .to_path_buf();

    let db = Db::open(&acs_dir.join("project.db"))?;
    let report = run_cleanup(&project_dir, &db)?;

    // Output as JSON
    let output = serde_json::json!({
        "branches_found": report.branches_found,
        "branches_deleted": report.branches_deleted,
        "worktrees_removed": report.worktrees_removed,
        "pruned": report.pruned,
    });
    println!("{}", serde_json::to_string_pretty(&output)?);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_ticket_id_standard_branch() {
        assert_eq!(
            extract_ticket_id("acs/t-007-a1b2"),
            Some("t-007".to_string())
        );
    }

    #[test]
    fn extract_ticket_id_double_digit() {
        assert_eq!(
            extract_ticket_id("acs/t-024-ff00"),
            Some("t-024".to_string())
        );
    }

    #[test]
    fn extract_ticket_id_no_suffix() {
        // Branch without random suffix — returns full rest
        assert_eq!(
            extract_ticket_id("acs/t-001"),
            Some("t-001".to_string())
        );
    }

    #[test]
    fn extract_ticket_id_not_acs_branch() {
        assert_eq!(extract_ticket_id("feature/foo-bar"), None);
    }

    #[test]
    fn extract_ticket_id_non_hex_suffix() {
        // "zzzz" is not valid hex — treated as no suffix
        assert_eq!(
            extract_ticket_id("acs/t-005-zzzz"),
            Some("t-005-zzzz".to_string())
        );
    }

    #[test]
    fn is_ticket_completed_returns_true_for_completed() {
        let db = Db::open_memory().unwrap();
        let id = db.create_ticket("test", "desc", "general", 1).unwrap();
        db.update_ticket(&id, "completed", None, None, None).unwrap();
        assert!(is_ticket_completed(&db, &id).unwrap());
    }

    #[test]
    fn is_ticket_completed_returns_false_for_in_progress() {
        let db = Db::open_memory().unwrap();
        let id = db.create_ticket("test", "desc", "general", 1).unwrap();
        db.update_ticket(&id, "in_progress", None, None, None).unwrap();
        assert!(!is_ticket_completed(&db, &id).unwrap());
    }

    #[test]
    fn is_ticket_completed_returns_true_for_missing_ticket() {
        let db = Db::open_memory().unwrap();
        assert!(is_ticket_completed(&db, "t-999").unwrap());
    }

    #[test]
    fn cleanup_in_real_repo_deletes_completed_branch() {
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = tmp.path();

        // Init git repo
        Command::new("git").args(["init"]).current_dir(repo).output().unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(repo)
            .output()
            .unwrap();

        // Create a branch like a worker would
        Command::new("git")
            .args(["branch", "acs/t-001-abcd"])
            .current_dir(repo)
            .output()
            .unwrap();

        // Set up DB with completed ticket
        std::fs::create_dir_all(repo.join(".acs")).unwrap();
        let db = Db::open(&repo.join(".acs/project.db")).unwrap();
        // Manually insert ticket t-001
        db.create_ticket("task one", "desc", "general", 1).unwrap();
        db.update_ticket("t-001", "completed", None, None, None).unwrap();

        let report = run_cleanup(repo, &db).unwrap();
        assert!(report.branches_found.contains(&"acs/t-001-abcd".to_string()));
        assert!(report.branches_deleted.contains(&"acs/t-001-abcd".to_string()));
        assert!(report.pruned);

        // Verify branch is actually gone
        let output = Command::new("git")
            .args(["branch", "--list", "acs/t-001-*"])
            .current_dir(repo)
            .output()
            .unwrap();
        let branches = String::from_utf8_lossy(&output.stdout);
        assert!(branches.trim().is_empty(), "branch should be deleted");
    }

    #[test]
    fn cleanup_preserves_in_progress_branch() {
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = tmp.path();

        Command::new("git").args(["init"]).current_dir(repo).output().unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(repo)
            .output()
            .unwrap();

        Command::new("git")
            .args(["branch", "acs/t-001-abcd"])
            .current_dir(repo)
            .output()
            .unwrap();

        std::fs::create_dir_all(repo.join(".acs")).unwrap();
        let db = Db::open(&repo.join(".acs/project.db")).unwrap();
        db.create_ticket("task one", "desc", "general", 1).unwrap();
        db.update_ticket("t-001", "in_progress", None, None, None).unwrap();

        let report = run_cleanup(repo, &db).unwrap();
        assert!(report.branches_found.contains(&"acs/t-001-abcd".to_string()));
        assert!(report.branches_deleted.is_empty(), "should not delete in_progress branch");
    }
}
