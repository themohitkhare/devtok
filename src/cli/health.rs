use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::json;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;
use crate::db::Db;

const STATUS_OK: &str = "ok";
const STATUS_WARN: &str = "warn";
const STATUS_ERROR: &str = "error";

#[derive(Debug, Clone, Serialize)]
pub struct CheckReport {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub overall: String,
    pub db: CheckReport,
    pub stuck_workers: CheckReport,
    pub orphaned_worktrees: CheckReport,
    pub git_merge_safety: CheckReport,
    pub blocked_vs_stale: CheckReport,
    pub conflict_deferred_warnings: CheckReport,
}

impl HealthReport {
    pub fn short_summary(&self) -> String {
        format!(
            "overall={} db={} stuck_workers={} orphaned_worktrees={} git_merge_safety={} blocked_vs_stale={} conflict_deferred_warnings={}",
            self.overall,
            self.db.status,
            self.stuck_workers.status,
            self.orphaned_worktrees.status,
            self.git_merge_safety.status,
            self.blocked_vs_stale.status,
            self.conflict_deferred_warnings.status
        )
    }
}

pub fn execute() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let project_dir = acs_dir
        .parent()
        .context("Expected `.acs/` to be inside a project directory")?
        .to_path_buf();

    let config = Config::load(&acs_dir.join("config.toml"))?;
    let report = run_health_checks(&project_dir, &acs_dir, &config);
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

pub fn run_health_checks(project_dir: &Path, acs_dir: &Path, config: &Config) -> HealthReport {
    let db_path = acs_dir.join("project.db");

    // Each check is best-effort; the report always prints.
    let db_report = check_db_accessible_and_not_locked(&db_path);

    let stuck_workers_report = if db_report.status == STATUS_OK {
        check_stuck_workers(project_dir, acs_dir, config, &db_path)
    } else {
        CheckReport {
            status: STATUS_ERROR.to_string(),
            details: Some(json!({
                "reason": "db_unavailable_or_locked",
                "db_check_status": db_report.status,
            })),
        }
    };

    let orphaned_worktrees_report = check_orphaned_worktrees(project_dir, acs_dir);
    let git_merge_safety_report = check_git_merge_safety(project_dir);
    let blocked_vs_stale_report = if db_report.status == STATUS_OK {
        check_blocked_vs_stale(project_dir, acs_dir, &db_path)
    } else {
        CheckReport {
            status: STATUS_ERROR.to_string(),
            details: Some(json!({
                "reason": "db_unavailable_or_locked",
                "db_check_status": db_report.status,
            })),
        }
    };

    let conflict_deferred_warnings_report = if db_report.status == STATUS_OK {
        check_conflict_deferred_warnings(&db_path)
    } else {
        CheckReport {
            status: STATUS_ERROR.to_string(),
            details: Some(json!({
                "reason": "db_unavailable_or_locked",
                "db_check_status": db_report.status,
            })),
        }
    };

    let overall = worst_overall_status(&[
        &db_report,
        &stuck_workers_report,
        &orphaned_worktrees_report,
        &git_merge_safety_report,
        &blocked_vs_stale_report,
        &conflict_deferred_warnings_report,
    ]);

    HealthReport {
        overall,
        db: db_report,
        stuck_workers: stuck_workers_report,
        orphaned_worktrees: orphaned_worktrees_report,
        git_merge_safety: git_merge_safety_report,
        blocked_vs_stale: blocked_vs_stale_report,
        conflict_deferred_warnings: conflict_deferred_warnings_report,
    }
}

fn worst_overall_status(checks: &[&CheckReport]) -> String {
    if checks.iter().any(|c| c.status == STATUS_ERROR) {
        STATUS_ERROR.to_string()
    } else if checks.iter().any(|c| c.status == STATUS_WARN) {
        STATUS_WARN.to_string()
    } else {
        STATUS_OK.to_string()
    }
}

fn sqlite_error_mentions_lock(e: &anyhow::Error) -> bool {
    let s = e.to_string().to_lowercase();
    s.contains("database is locked") || s.contains("is locked") || s.contains("locked")
}

fn check_db_accessible_and_not_locked(db_path: &Path) -> CheckReport {
    if !db_path.exists() {
        return CheckReport {
            status: STATUS_ERROR.to_string(),
            details: Some(json!({ "db_path": db_path, "error": "project.db missing" })),
        };
    }

    match Db::open(db_path) {
        Ok(db) => match db.count_by_status() {
            Ok(_) => CheckReport {
                status: STATUS_OK.to_string(),
                details: Some(json!({ "db_path": db_path })),
            },
            Err(e) => {
                if sqlite_error_mentions_lock(&e) {
                    CheckReport {
                        status: STATUS_ERROR.to_string(),
                        details: Some(json!({ "db_path": db_path, "error": e.to_string() })),
                    }
                } else {
                    CheckReport {
                        status: STATUS_ERROR.to_string(),
                        details: Some(json!({ "db_path": db_path, "error": e.to_string() })),
                    }
                }
            }
        },
        Err(e) => {
            if sqlite_error_mentions_lock(&e) {
                CheckReport {
                    status: STATUS_ERROR.to_string(),
                    details: Some(json!({ "db_path": db_path, "error": e.to_string() })),
                }
            } else {
                CheckReport {
                    status: STATUS_ERROR.to_string(),
                    details: Some(json!({ "db_path": db_path, "error": e.to_string() })),
                }
            }
        }
    }
}

fn check_stuck_workers(
    _project_dir: &Path,
    _acs_dir: &Path,
    config: &Config,
    db_path: &Path,
) -> CheckReport {
    let timeout_seconds = config.manager.worker_timeout_seconds;
    let db = match Db::open(db_path) {
        Ok(db) => db,
        Err(e) => {
            return CheckReport {
                status: STATUS_ERROR.to_string(),
                details: Some(json!({ "error": e.to_string() })),
            };
        }
    };

    let in_progress = match db.list_tickets(Some("in_progress")) {
        Ok(v) => v,
        Err(e) => {
            return CheckReport {
                status: STATUS_ERROR.to_string(),
                details: Some(json!({ "error": e.to_string() })),
            };
        }
    };

    let now = Utc::now();
    let mut stuck = Vec::new();
    for t in &in_progress {
        let ts = match parse_rfc3339_to_utc(&t.updated_at) {
            Some(dt) => dt,
            None => {
                return CheckReport {
                    status: STATUS_ERROR.to_string(),
                    details: Some(json!({ "ticket_id": t.id, "error": "invalid updated_at" })),
                };
            }
        };
        let elapsed = now.signed_duration_since(ts);
        if elapsed > Duration::seconds(timeout_seconds as i64) {
            stuck.push(json!({
                "ticket_id": t.id,
                "assignee": t.assignee,
                "age_secs": elapsed.num_seconds(),
            }));
        }
    }

    if stuck.is_empty() {
        CheckReport {
            status: STATUS_OK.to_string(),
            details: Some(json!({ "timeout_seconds": timeout_seconds, "in_progress": in_progress.len() })),
        }
    } else {
        // Keep output small for readability; the CLI output is still JSON and can be pretty-printed.
        let full_count = stuck.len();
        let max_show = 25usize;
        let truncated = stuck.into_iter().take(max_show).collect::<Vec<_>>();
        let shown = truncated.len();
        CheckReport {
            status: STATUS_WARN.to_string(),
            details: Some(json!({
                "timeout_seconds": timeout_seconds,
                "stuck_count": full_count,
                "stuck_count_shown": shown,
                "stuck_sample": truncated,
                "in_progress_total": in_progress.len(),
            })),
        }
    }
}

fn parse_rfc3339_to_utc(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn check_orphaned_worktrees(project_dir: &Path, acs_dir: &Path) -> CheckReport {
    let worktrees_dir = acs_dir.join("worktrees");
    if !worktrees_dir.exists() {
        return CheckReport {
            status: STATUS_OK.to_string(),
            details: Some(json!({ "worktrees_dir": worktrees_dir, "orphaned": 0 })),
        };
    }

    let active_paths = match list_active_git_worktree_paths(project_dir) {
        Ok(p) => p,
        Err(e) => {
            return CheckReport {
                status: STATUS_ERROR.to_string(),
                details: Some(json!({ "error": e.to_string() })),
            };
        }
    };

    let mut orphans: Vec<String> = Vec::new();
    let mut entries_total = 0usize;

    let entries = std::fs::read_dir(&worktrees_dir);
    if entries.is_err() {
        return CheckReport {
            status: STATUS_ERROR.to_string(),
            details: Some(json!({ "worktrees_dir": worktrees_dir, "error": "failed to read_dir" })),
        };
    }

    let entries = entries.unwrap();
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        entries_total += 1;

        let canonical = std::fs::canonicalize(&path).unwrap_or(path.clone());
        if !active_paths.contains(&canonical) {
            orphans.push(entry.file_name().to_string_lossy().to_string());
        }
    }

    if orphans.is_empty() {
        CheckReport {
            status: STATUS_OK.to_string(),
            details: Some(json!({
                "orphaned": 0,
                "entries_total": entries_total
            })),
        }
    } else {
        CheckReport {
            status: STATUS_WARN.to_string(),
            details: Some(json!({
                "orphaned": orphans.len(),
                "entries_total": entries_total,
                "orphaned_worktrees": orphans
            })),
        }
    }
}

fn list_active_git_worktree_paths(project_dir: &Path) -> Result<HashSet<PathBuf>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(project_dir)
        .output()
        .with_context(|| "Failed to run git worktree list")?;

    if !output.status.success() {
        anyhow::bail!("git worktree list failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut set: HashSet<PathBuf> = HashSet::new();
    for p in parse_git_worktree_list_porcelain(&stdout) {
        let canonical = std::fs::canonicalize(&p).unwrap_or(p);
        set.insert(canonical);
    }
    Ok(set)
}

fn parse_git_worktree_list_porcelain(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(|p| PathBuf::from(p.trim()))
        .collect()
}

fn check_git_merge_safety(project_dir: &Path) -> CheckReport {
    // Any of these conditions makes merges risky:
    // - MERGE_HEAD exists
    // - unmerged/conflicting paths in porcelain status
    // - any tracked changes present (warn)
    let merge_in_progress = Command::new("git")
        .args(["rev-parse", "-q", "--verify", "MERGE_HEAD"])
        .current_dir(project_dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if merge_in_progress {
        return CheckReport {
            status: STATUS_ERROR.to_string(),
            details: Some(json!({ "merge_in_progress": true })),
        };
    }

    let output = Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(project_dir)
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            return CheckReport {
                status: STATUS_ERROR.to_string(),
                details: Some(json!({ "error": e.to_string() })),
            };
        }
    };

    if !output.status.success() {
        return CheckReport {
            status: STATUS_ERROR.to_string(),
            details: Some(json!({ "error": "git status --porcelain failed" })),
        };
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut untracked = 0usize;
    let mut dirty_tracked = false;
    let mut conflict_unmerged = false;

    for line in stdout.lines() {
        let line = line.trim_end();
        if line.len() < 2 {
            continue;
        }
        let c1 = line.chars().nth(0).unwrap_or(' ');
        let c2 = line.chars().nth(1).unwrap_or(' ');
        let code = format!("{}{}", c1, c2);

        // Unmerged/conflict codes in porcelain v1
        let is_conflict = matches!(
            code.as_str(),
            "UU" | "AA" | "DD" | "AU" | "UA" | "DU" | "UD"
        );

        if is_conflict {
            conflict_unmerged = true;
            break;
        }

        if code == "??" {
            untracked += 1;
            continue;
        }

        if code != "  " {
            dirty_tracked = true;
        }
    }

    // Confirm unmerged paths explicitly (stronger signal than porcelain alone).
    let unmerged_output = Command::new("git")
        .args(["ls-files", "-u"])
        .current_dir(project_dir)
        .output();
    let unmerged_output = match unmerged_output {
        Ok(o) => o,
        Err(_) => {
            return CheckReport {
                status: STATUS_ERROR.to_string(),
                details: Some(json!({ "error": "git ls-files -u failed" })),
            };
        }
    };

    if unmerged_output.status.success() {
        let s = String::from_utf8_lossy(&unmerged_output.stdout);
        if !s.trim().is_empty() {
            return CheckReport {
                status: STATUS_ERROR.to_string(),
                details: Some(json!({ "unmerged_paths": true })),
            };
        }
    }

    if conflict_unmerged {
        return CheckReport {
            status: STATUS_ERROR.to_string(),
            details: Some(json!({ "conflict_unmerged": true })),
        };
    }

    if dirty_tracked {
        return CheckReport {
            status: STATUS_WARN.to_string(),
            details: Some(json!({
                "dirty_tracked": true,
                "untracked_files": untracked
            })),
        };
    }

    if untracked > 0 {
        return CheckReport {
            status: STATUS_WARN.to_string(),
            details: Some(json!({
                "dirty_tracked": false,
                "untracked_files": untracked
            })),
        };
    }

    CheckReport {
        status: STATUS_OK.to_string(),
        details: Some(json!({ "dirty_tracked": false, "untracked_files": 0 })),
    }
}

fn compute_blocked_vs_stale(db: &Db) -> Result<(Vec<String>, Vec<String>)> {
    let blocked = db.list_tickets(Some("blocked"))?;

    let mut truly_blocked: Vec<String> = Vec::new();
    let mut stale: Vec<String> = Vec::new();

    for t in &blocked {
        let blocker_id = match t.blocked_by.as_deref() {
            Some(id) if !id.is_empty() => id,
            _ => {
                stale.push(t.id.clone());
                continue;
            }
        };

        match db.get_ticket(blocker_id)? {
            Some(blocker) => {
                if blocker.status != "completed" {
                    truly_blocked.push(t.id.clone());
                } else {
                    stale.push(t.id.clone());
                }
            }
            None => stale.push(t.id.clone()),
        }
    }

    Ok((truly_blocked, stale))
}

fn check_conflict_deferred_warnings(db_path: &Path) -> CheckReport {
    let db = match Db::open(db_path) {
        Ok(db) => db,
        Err(e) => {
            return CheckReport {
                status: STATUS_ERROR.to_string(),
                details: Some(json!({ "error": e.to_string() })),
            };
        }
    };

    let high_defer = match db.list_tickets_with_defer_count_gt(3) {
        Ok(v) => v,
        Err(e) => {
            return CheckReport {
                status: STATUS_ERROR.to_string(),
                details: Some(json!({ "error": e.to_string() })),
            };
        }
    };

    if high_defer.is_empty() {
        CheckReport {
            status: STATUS_OK.to_string(),
            details: Some(json!({ "high_defer_count": 0 })),
        }
    } else {
        let sample: Vec<serde_json::Value> = high_defer
            .iter()
            .take(25)
            .map(|t| json!({ "ticket_id": t.id, "defer_count": t.defer_count, "status": t.status }))
            .collect();
        CheckReport {
            status: STATUS_WARN.to_string(),
            details: Some(json!({
                "high_defer_count": high_defer.len(),
                "tickets": sample,
                "note": "tickets deferred >3 times may be stuck in conflict loop"
            })),
        }
    }
}

fn check_blocked_vs_stale(_project_dir: &Path, _acs_dir: &Path, db_path: &Path) -> CheckReport {
    let db = match Db::open(db_path) {
        Ok(db) => db,
        Err(e) => {
            return CheckReport {
                status: STATUS_ERROR.to_string(),
                details: Some(json!({ "error": e.to_string() })),
            };
        }
    };

    let (truly_blocked, stale) = match compute_blocked_vs_stale(&db) {
        Ok(v) => v,
        Err(e) => {
            return CheckReport {
                status: STATUS_ERROR.to_string(),
                details: Some(json!({ "error": e.to_string() })),
            };
        }
    };

    let status = if truly_blocked.is_empty() && stale.is_empty() {
        STATUS_OK
    } else {
        STATUS_WARN
    };

    CheckReport {
        status: status.to_string(),
        details: Some(json!({
            "truly_blocked_count": truly_blocked.len(),
            "stale_count": stale.len(),
            "truly_blocked_sample": truly_blocked.into_iter().take(25).collect::<Vec<_>>(),
            "stale_sample": stale.into_iter().take(25).collect::<Vec<_>>(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration as StdDuration;

    #[test]
    fn blocked_vs_stale_classifies_true_blocked() {
        let db = Db::open_memory().unwrap();

        let blocker = db.create_ticket("Blocker", "desc", "general", 1).unwrap();
        let blocked = db.create_ticket("Blocked", "desc", "general", 1).unwrap();
        db.update_ticket(&blocked, "blocked", None, Some(&blocker), None)
            .unwrap();
        // blocker is not completed → truly blocked
        db.update_ticket(&blocker, "in_progress", None, None, None).unwrap();

        let (truly_blocked, stale) = compute_blocked_vs_stale(&db).unwrap();
        assert_eq!(truly_blocked, vec!["t-002".to_string()]);
        assert!(stale.is_empty());
    }

    #[test]
    fn blocked_vs_stale_classifies_stale_when_blocker_completed() {
        let db = Db::open_memory().unwrap();

        let blocker = db.create_ticket("Blocker", "desc", "general", 1).unwrap();
        let blocked = db.create_ticket("Blocked", "desc", "general", 1).unwrap();
        db.update_ticket(&blocked, "blocked", None, Some(&blocker), None)
            .unwrap();
        db.update_ticket(&blocker, "completed", None, None, None).unwrap();

        let (truly_blocked, stale) = compute_blocked_vs_stale(&db).unwrap();
        assert!(truly_blocked.is_empty());
        assert_eq!(stale, vec!["t-002".to_string()]);
    }

    #[test]
    fn rfc3339_parse_to_utc_works() {
        let ts = "2026-03-19T18:31:35.301826+00:00";
        let parsed = parse_rfc3339_to_utc(ts);
        assert!(parsed.is_some());
    }

    #[test]
    fn parse_git_worktree_list_porcelain_extracts_paths() {
        let sample = "worktree /tmp/a\nbare\nworktree /tmp/b\n";
        let paths = parse_git_worktree_list_porcelain(sample);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/tmp/a"));
        assert_eq!(paths[1], PathBuf::from("/tmp/b"));
    }

    // Note: unit tests for stuck workers + blocked-vs-stale counting require
    // direct DB access. In this codebase, DB helpers open paths, so we keep
    // tests focused on helper logic and time classification.
    #[test]
    fn stuck_workers_time_threshold_logic_triggers_immediately_with_timeout_zero() {
        let db = Db::open_memory().unwrap();
        let timeout_seconds = 0u64;

        let id = db.create_ticket("T", "D", "general", 1).unwrap();
        db.update_ticket(&id, "in_progress", None, None, None).unwrap();

        // Sleep a hair so "now - updated_at" is definitely > 0ns.
        std::thread::sleep(StdDuration::from_millis(10));

        let ticket = db.get_ticket(&id).unwrap().unwrap();
        let ts = parse_rfc3339_to_utc(&ticket.updated_at).unwrap();
        let elapsed = Utc::now().signed_duration_since(ts);
        assert!(elapsed > Duration::seconds(timeout_seconds as i64));
    }
}

