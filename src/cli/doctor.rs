// src/cli/doctor.rs — acs doctor: system health check and auto-fix (t-081)
//
// Runs 10 diagnostic checks against the local environment. By default,
// every check is read-only and the command exits 1 if any check fails.
// With --fix, auto-fixable problems are repaired and the exit code reflects
// the post-fix state.

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ── ANSI colour helpers ───────────────────────────────────────────────────────
fn green(s: &str) -> String {
    format!("\x1b[32m{}\x1b[0m", s)
}
fn red(s: &str) -> String {
    format!("\x1b[31m{}\x1b[0m", s)
}
fn yellow(s: &str) -> String {
    format!("\x1b[33m{}\x1b[0m", s)
}
fn bold(s: &str) -> String {
    format!("\x1b[1m{}\x1b[0m", s)
}

// ── Status labels ─────────────────────────────────────────────────────────────
fn label_pass() -> String {
    green("PASS")
}
fn label_fail() -> String {
    red("FAIL")
}
fn label_fixed() -> String {
    yellow("FIXED")
}

fn print_check(name: &str, pass: bool, fixed: bool, note: &str) {
    let label = if fixed {
        label_fixed()
    } else if pass {
        label_pass()
    } else {
        label_fail()
    };
    println!("  [{}] {:.<45} {}", label, format!("{} ", name), note);
}

// ── Entry point ───────────────────────────────────────────────────────────────
pub fn execute(fix: bool) -> Result<()> {
    println!("{}", bold("acs doctor — system health check"));
    println!();

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Try to find project root (walk up looking for .acs/)
    let project_dir = find_project_root(&cwd);
    let acs_dir = project_dir
        .as_ref()
        .map(|p| p.join(".acs"))
        .unwrap_or_else(|| cwd.join(".acs"));

    let mut any_fail = false;

    // 1. git installed and repo is a git repo
    {
        let (pass, note) = check_git_repo(&project_dir.as_deref().unwrap_or(&cwd));
        if !pass {
            any_fail = true;
        }
        print_check("git: installed & repo", pass, false, &note);
    }

    // 2. claude CLI found at configured path and responds to --version
    {
        let claude_path = resolve_claude_path(&acs_dir);
        let (pass, note) = check_claude_cli(&claude_path);
        if !pass {
            any_fail = true;
        }
        print_check("claude: CLI found & responds", pass, false, &note);
    }

    // 3. .acs/ directory exists and is writable
    {
        let (pass, fixed, note) = check_acs_dir(&acs_dir, fix);
        if !pass && !fixed {
            any_fail = true;
        }
        print_check(".acs/: exists and writable", pass || fixed, fixed, &note);
    }

    // 4. project.db exists and schema is current version
    {
        let (pass, note) = check_project_db(&acs_dir);
        if !pass {
            any_fail = true;
        }
        print_check("project.db: exists & schema current", pass, false, &note);
    }

    // 5. No orphaned worktrees
    {
        let (pass, fixed, note) = check_orphaned_worktrees(
            project_dir.as_deref().unwrap_or(&cwd),
            &acs_dir,
            fix,
        );
        if !pass && !fixed {
            any_fail = true;
        }
        print_check("worktrees: no orphans", pass || fixed, fixed, &note);
    }

    // 6. No zombie workers (run.pid exists but process is dead)
    {
        let (pass, fixed, note) = check_zombie_pid(&acs_dir, fix);
        if !pass && !fixed {
            any_fail = true;
        }
        print_check("run.pid: no zombie worker", pass || fixed, fixed, &note);
    }

    // 7. Config.toml is valid TOML and has required fields
    {
        let (pass, note) = check_config_toml(&acs_dir);
        if !pass {
            any_fail = true;
        }
        print_check("config.toml: valid & required fields", pass, false, &note);
    }

    // 8. claude_path binary is executable
    {
        let claude_path = resolve_claude_path(&acs_dir);
        let (pass, note) = check_claude_executable(&claude_path);
        if !pass {
            any_fail = true;
        }
        print_check("claude_path: binary executable", pass, false, &note);
    }

    // 9. Disk space: at least 500 MB free
    {
        let (pass, note) = check_disk_space(&cwd);
        if !pass {
            any_fail = true;
        }
        print_check("disk: ≥500 MB free", pass, false, &note);
    }

    // 10. All in_progress tickets have a live worker
    {
        let (pass, fixed, note) = check_in_progress_workers(&acs_dir, fix);
        if !pass && !fixed {
            any_fail = true;
        }
        print_check("tickets: in_progress have live worker", pass || fixed, fixed, &note);
    }

    // 11. .acs in .gitignore
    {
        let (pass, fixed, note) = check_gitignore(
            project_dir.as_deref().unwrap_or(&cwd),
            fix,
        );
        if !pass && !fixed {
            any_fail = true;
        }
        print_check(".gitignore: .acs/ excluded", pass || fixed, fixed, &note);
    }

    println!();
    if any_fail {
        if fix {
            println!("{}", red("Some checks still FAIL after --fix. See details above."));
        } else {
            println!(
                "{} — re-run with {} to attempt automatic repairs",
                red("One or more checks FAILED"),
                bold("acs doctor --fix")
            );
        }
        // Non-zero exit: caller in main.rs will map Err to exit 1.
        anyhow::bail!("doctor: one or more checks failed");
    } else {
        println!("{}", green("All checks passed."));
        Ok(())
    }
}

// ── Project root discovery ─────────────────────────────────────────────────────
fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".acs").is_dir() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

// ── Config helper ─────────────────────────────────────────────────────────────
/// Read `claude_path` from config.toml if possible, else fall back to "claude".
fn resolve_claude_path(acs_dir: &Path) -> String {
    let config_path = acs_dir.join("config.toml");
    if let Ok(raw) = fs::read_to_string(&config_path) {
        if let Ok(table) = raw.parse::<toml::Value>() {
            if let Some(p) = table
                .get("agents")
                .and_then(|a| a.get("claude_path"))
                .and_then(|v| v.as_str())
            {
                return p.to_string();
            }
        }
    }
    "claude".to_string()
}

// ── Check 1: git ──────────────────────────────────────────────────────────────
fn check_git_repo(project_dir: &Path) -> (bool, String) {
    // Is git installed?
    let git_ok = Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !git_ok {
        return (false, "git not found in PATH".to_string());
    }

    // Is this inside a git repo?
    let inside = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(project_dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if inside {
        (true, "git installed, inside a git repo".to_string())
    } else {
        (false, "current directory is not inside a git repo".to_string())
    }
}

// ── Check 2: claude CLI ────────────────────────────────────────────────────────
fn check_claude_cli(claude_path: &str) -> (bool, String) {
    let result = Command::new(claude_path)
        .arg("--version")
        .output();

    match result {
        Ok(o) if o.status.success() => {
            let ver = String::from_utf8_lossy(&o.stdout).trim().to_string();
            (true, format!("found '{}' — {}", claude_path, ver))
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
            (
                false,
                format!("'{}' exited non-zero: {}", claude_path, stderr),
            )
        }
        Err(e) => (false, format!("cannot run '{}': {}", claude_path, e)),
    }
}

// ── Check 3: .acs/ directory ──────────────────────────────────────────────────
fn check_acs_dir(acs_dir: &Path, fix: bool) -> (bool, bool, String) {
    if acs_dir.is_dir() {
        // Check writability by attempting a probe
        let probe = acs_dir.join(".doctor_probe");
        match fs::write(&probe, b"") {
            Ok(_) => {
                let _ = fs::remove_file(&probe);
                (true, false, "exists and writable".to_string())
            }
            Err(e) => (false, false, format!("not writable: {}", e)),
        }
    } else if fix {
        match fs::create_dir_all(acs_dir) {
            Ok(_) => (true, true, format!("created {}", acs_dir.display())),
            Err(e) => (false, false, format!("failed to create: {}", e)),
        }
    } else {
        (false, false, format!("{} does not exist", acs_dir.display()))
    }
}

// ── Check 4: project.db schema ────────────────────────────────────────────────
fn check_project_db(acs_dir: &Path) -> (bool, String) {
    let db_path = acs_dir.join("project.db");
    if !db_path.exists() {
        return (false, "project.db not found (run `acs init` first)".to_string());
    }

    match crate::db::Db::open(&db_path) {
        Ok(db) => match db.schema_version() {
            Ok(v) => (true, format!("schema v{} (current)", v)),
            Err(e) => (false, format!("cannot read schema version: {}", e)),
        },
        Err(e) => (false, format!("cannot open DB: {}", e)),
    }
}

// ── Check 5: orphaned worktrees ────────────────────────────────────────────────
fn check_orphaned_worktrees(project_dir: &Path, acs_dir: &Path, fix: bool) -> (bool, bool, String) {
    let worktrees_dir = acs_dir.join("worktrees");
    if !worktrees_dir.is_dir() {
        return (true, false, "no worktrees directory".to_string());
    }

    // Collect paths registered with git
    let active = match list_git_worktree_paths(project_dir) {
        Ok(paths) => paths,
        Err(e) => {
            return (false, false, format!("git worktree list failed: {}", e));
        }
    };

    let mut orphans: Vec<PathBuf> = Vec::new();
    let entries = match fs::read_dir(&worktrees_dir) {
        Ok(e) => e,
        Err(e) => {
            return (false, false, format!("cannot read worktrees dir: {}", e));
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let canonical = fs::canonicalize(&path).unwrap_or(path.clone());
        if !active.contains(&canonical) {
            orphans.push(path);
        }
    }

    if orphans.is_empty() {
        return (true, false, "no orphaned worktrees".to_string());
    }

    if fix {
        let mut removed = 0usize;
        let mut errs: Vec<String> = Vec::new();
        for p in &orphans {
            // Try git worktree remove first (cleans up git metadata)
            let removed_by_git = Command::new("git")
                .args(["worktree", "remove", "--force"])
                .arg(p)
                .current_dir(project_dir)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

            if removed_by_git {
                removed += 1;
            } else {
                // Fall back to plain directory removal
                match fs::remove_dir_all(p) {
                    Ok(_) => removed += 1,
                    Err(e) => errs.push(format!("{}: {}", p.display(), e)),
                }
            }
        }
        let note = if errs.is_empty() {
            format!("removed {} orphaned worktree(s)", removed)
        } else {
            format!(
                "removed {}, {} error(s): {}",
                removed,
                errs.len(),
                errs.join("; ")
            )
        };
        (errs.is_empty(), errs.is_empty(), note)
    } else {
        let names: Vec<String> = orphans
            .iter()
            .map(|p| p.file_name().unwrap_or_default().to_string_lossy().into())
            .collect();
        (
            false,
            false,
            format!("{} orphan(s): {}", orphans.len(), names.join(", ")),
        )
    }
}

fn list_git_worktree_paths(project_dir: &Path) -> Result<std::collections::HashSet<PathBuf>> {
    let out = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(project_dir)
        .output()?;

    if !out.status.success() {
        anyhow::bail!("git worktree list exited non-zero");
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut set = std::collections::HashSet::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("worktree ") {
            let p = PathBuf::from(rest.trim());
            let canonical = fs::canonicalize(&p).unwrap_or(p);
            set.insert(canonical);
        }
    }
    Ok(set)
}

// ── Check 6: zombie run.pid ────────────────────────────────────────────────────
fn check_zombie_pid(acs_dir: &Path, fix: bool) -> (bool, bool, String) {
    let pid_path = acs_dir.join("run.pid");
    if !pid_path.exists() {
        return (true, false, "no run.pid file".to_string());
    }

    let content = match fs::read_to_string(&pid_path) {
        Ok(c) => c,
        Err(e) => return (false, false, format!("cannot read run.pid: {}", e)),
    };

    let pid: u32 = match content.trim().parse() {
        Ok(p) => p,
        Err(_) => {
            return if fix {
                match fs::remove_file(&pid_path) {
                    Ok(_) => (true, true, "removed invalid run.pid".to_string()),
                    Err(e) => (false, false, format!("invalid PID, could not remove: {}", e)),
                }
            } else {
                (false, false, "run.pid contains non-numeric content".to_string())
            };
        }
    };

    // Check if process is alive (kill -0 sends no signal, just checks existence)
    let alive = is_process_alive(pid);

    if alive {
        (true, false, format!("pid {} is running", pid))
    } else if fix {
        match fs::remove_file(&pid_path) {
            Ok(_) => (true, true, format!("deleted zombie run.pid (pid {} dead)", pid)),
            Err(e) => (false, false, format!("pid {} dead but cannot delete run.pid: {}", pid, e)),
        }
    } else {
        (false, false, format!("pid {} is not running (zombie run.pid)", pid))
    }
}

fn is_process_alive(pid: u32) -> bool {
    // POSIX: kill -0 <pid> succeeds iff the process exists and we can signal it
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── Check 7: config.toml ──────────────────────────────────────────────────────
fn check_config_toml(acs_dir: &Path) -> (bool, String) {
    let config_path = acs_dir.join("config.toml");
    if !config_path.exists() {
        return (false, "config.toml not found".to_string());
    }

    let raw = match fs::read_to_string(&config_path) {
        Ok(r) => r,
        Err(e) => return (false, format!("cannot read config.toml: {}", e)),
    };

    let table: toml::Value = match raw.parse() {
        Ok(t) => t,
        Err(e) => return (false, format!("invalid TOML: {}", e)),
    };

    // Required: [project] section with `name` field
    let has_project_name = table
        .get("project")
        .and_then(|p| p.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    if has_project_name {
        (true, "valid TOML with required fields".to_string())
    } else {
        (false, "missing required [project] name field".to_string())
    }
}

// ── Check 8: claude_path binary executable ────────────────────────────────────
fn check_claude_executable(claude_path: &str) -> (bool, String) {
    // If it's a bare name (no slash), try `which`
    if !claude_path.contains('/') {
        let which = Command::new("which")
            .arg(claude_path)
            .output()
            .ok()
            .filter(|o| o.status.success());
        return match which {
            Some(o) => {
                let resolved = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if is_executable(&resolved) {
                    (true, format!("'{}' found at {} (executable)", claude_path, resolved))
                } else {
                    (false, format!("'{}' found at {} but not executable", claude_path, resolved))
                }
            }
            None => (false, format!("'{}' not found in PATH", claude_path)),
        };
    }

    // Absolute or relative path
    if is_executable(claude_path) {
        (true, format!("'{}' is executable", claude_path))
    } else if Path::new(claude_path).exists() {
        (false, format!("'{}' exists but is not executable", claude_path))
    } else {
        (false, format!("'{}' does not exist", claude_path))
    }
}

#[cfg(unix)]
fn is_executable(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &str) -> bool {
    // On non-Unix, just check existence
    Path::new(path).exists()
}

// ── Check 9: disk space ────────────────────────────────────────────────────────
const MIN_FREE_MB: u64 = 500;

fn check_disk_space(dir: &Path) -> (bool, String) {
    // Use `df -k` to get kilobytes available for the given path
    let out = Command::new("df")
        .args(["-k"])
        .arg(dir)
        .output();

    match out {
        Err(e) => (false, format!("df failed: {}", e)),
        Ok(o) if !o.status.success() => {
            (false, format!("df exited {}", o.status))
        }
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            // df -k output: header line then data line.
            // Columns: Filesystem, 1K-blocks, Used, Available, Use%, Mounted
            // On macOS the Available column is index 3; on Linux also index 3.
            if let Some(avail_kb) = parse_df_available_kb(&stdout) {
                let avail_mb = avail_kb / 1024;
                if avail_mb >= MIN_FREE_MB {
                    (true, format!("{} MB free", avail_mb))
                } else {
                    (false, format!("only {} MB free (need ≥{})", avail_mb, MIN_FREE_MB))
                }
            } else {
                (false, "could not parse df output".to_string())
            }
        }
    }
}

fn parse_df_available_kb(output: &str) -> Option<u64> {
    let mut lines = output.lines();
    let _header = lines.next()?; // skip header
    let data = lines.next()?;
    // Split on whitespace; Available is 4th column (index 3)
    let cols: Vec<&str> = data.split_whitespace().collect();
    // macOS df -k gives: Filesystem 512-blocks Used Available Capacity Mounted
    // Linux df -k gives:  Filesystem 1K-blocks Used Available Use% Mounted
    // We look for a numeric value at position 3 (index 3).
    if cols.len() >= 4 {
        cols[3].parse::<u64>().ok()
    } else {
        None
    }
}

// ── Check 10: in_progress tickets have live workers ────────────────────────────
fn check_in_progress_workers(acs_dir: &Path, fix: bool) -> (bool, bool, String) {
    let db_path = acs_dir.join("project.db");
    if !db_path.exists() {
        return (true, false, "no project.db (skipped)".to_string());
    }

    let db = match crate::db::Db::open(&db_path) {
        Ok(db) => db,
        Err(e) => return (false, false, format!("cannot open DB: {}", e)),
    };

    let in_progress = match db.list_tickets(Some("in_progress")) {
        Ok(t) => t,
        Err(e) => return (false, false, format!("DB query failed: {}", e)),
    };

    if in_progress.is_empty() {
        return (true, false, "no in_progress tickets".to_string());
    }

    // Build map of agent_id → pid from agents table
    let agents = match db.list_agents() {
        Ok(a) => a,
        Err(e) => return (false, false, format!("cannot list agents: {}", e)),
    };

    let agent_pid_map: std::collections::HashMap<String, Option<u32>> = agents
        .into_iter()
        .map(|a| (a.id.clone(), a.pid))
        .collect();

    let mut dead: Vec<String> = Vec::new();
    for ticket in &in_progress {
        let assignee = match ticket.assignee.as_deref() {
            Some(a) if !a.is_empty() => a,
            _ => {
                // No assignee — counts as no live worker
                dead.push(ticket.id.clone());
                continue;
            }
        };

        let pid = agent_pid_map.get(assignee).and_then(|p| *p);
        match pid {
            Some(p) if is_process_alive(p) => {}
            _ => dead.push(ticket.id.clone()),
        }
    }

    if dead.is_empty() {
        return (true, false, format!("{} in_progress, all have live workers", in_progress.len()));
    }

    if fix {
        let mut fixed_count = 0usize;
        let mut errs: Vec<String> = Vec::new();
        for ticket_id in &dead {
            match db.update_ticket(ticket_id, "pending", None, None, None) {
                Ok(_) => fixed_count += 1,
                Err(e) => errs.push(format!("{}: {}", ticket_id, e)),
            }
        }
        let note = if errs.is_empty() {
            format!("reset {} orphaned in_progress ticket(s) to pending", fixed_count)
        } else {
            format!(
                "reset {}, errors: {}",
                fixed_count,
                errs.join("; ")
            )
        };
        (errs.is_empty(), errs.is_empty(), note)
    } else {
        (
            false,
            false,
            format!(
                "{} ticket(s) in_progress with no live worker: {}",
                dead.len(),
                dead.join(", ")
            ),
        )
    }
}

// ── Check 11: .acs in .gitignore ──────────────────────────────────────────────
fn check_gitignore(project_dir: &Path, fix: bool) -> (bool, bool, String) {
    let gitignore_path = project_dir.join(".gitignore");

    let content = fs::read_to_string(&gitignore_path).unwrap_or_default();
    let already_ignored = content
        .lines()
        .any(|l| l.trim() == ".acs" || l.trim() == ".acs/");

    if already_ignored {
        return (true, false, ".acs/ already in .gitignore".to_string());
    }

    if fix {
        // Append to .gitignore (or create if missing)
        let new_entry = if content.ends_with('\n') || content.is_empty() {
            ".acs/\n".to_string()
        } else {
            "\n.acs/\n".to_string()
        };
        let new_content = format!("{}{}", content, new_entry);
        match fs::write(&gitignore_path, new_content) {
            Ok(_) => (true, true, "added .acs/ to .gitignore".to_string()),
            Err(e) => (false, false, format!("could not write .gitignore: {}", e)),
        }
    } else {
        (false, false, ".acs/ not in .gitignore".to_string())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_df_available_kb_linux_style() {
        // Filesystem  1K-blocks  Used  Available  Use%  Mounted
        let sample =
            "Filesystem     1K-blocks      Used Available Use% Mounted on\n/dev/sda1      999999999 123456789 800000000  14% /\n";
        assert_eq!(parse_df_available_kb(sample), Some(800_000_000));
    }

    #[test]
    fn parse_df_available_kb_insufficient_columns() {
        let sample = "Filesystem\n/dev/sda1\n";
        assert_eq!(parse_df_available_kb(sample), None);
    }

    #[test]
    fn check_git_repo_in_cwd_passes() {
        // The test runner is inside a git repo (the ACS project itself).
        let cwd = std::env::current_dir().unwrap();
        let (pass, _) = check_git_repo(&cwd);
        assert!(pass);
    }

    #[test]
    fn check_acs_dir_creates_on_fix() {
        let tmp = TempDir::new().unwrap();
        let acs = tmp.path().join(".acs");
        assert!(!acs.exists());
        let (pass, fixed, _) = check_acs_dir(&acs, true);
        assert!(pass);
        assert!(fixed);
        assert!(acs.is_dir());
    }

    #[test]
    fn check_acs_dir_fails_without_fix() {
        let tmp = TempDir::new().unwrap();
        let acs = tmp.path().join(".acs");
        let (pass, fixed, _) = check_acs_dir(&acs, false);
        assert!(!pass);
        assert!(!fixed);
    }

    #[test]
    fn check_acs_dir_writable() {
        let tmp = TempDir::new().unwrap();
        let acs = tmp.path().join(".acs");
        std::fs::create_dir_all(&acs).unwrap();
        let (pass, fixed, _) = check_acs_dir(&acs, false);
        assert!(pass);
        assert!(!fixed);
    }

    #[test]
    fn check_config_toml_missing_file() {
        let tmp = TempDir::new().unwrap();
        let (pass, _) = check_config_toml(tmp.path());
        assert!(!pass);
    }

    #[test]
    fn check_config_toml_valid() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("config.toml"),
            "[project]\nname = \"test\"\n",
        )
        .unwrap();
        let (pass, note) = check_config_toml(tmp.path());
        assert!(pass, "note: {}", note);
    }

    #[test]
    fn check_config_toml_missing_name() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("config.toml"), "[project]\n").unwrap();
        let (pass, _) = check_config_toml(tmp.path());
        assert!(!pass);
    }

    #[test]
    fn check_config_toml_invalid_toml() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("config.toml"), "[[[[invalid").unwrap();
        let (pass, _) = check_config_toml(tmp.path());
        assert!(!pass);
    }

    #[test]
    fn check_zombie_pid_no_file() {
        let tmp = TempDir::new().unwrap();
        let (pass, fixed, _) = check_zombie_pid(tmp.path(), false);
        assert!(pass);
        assert!(!fixed);
    }

    #[test]
    fn check_zombie_pid_dead_pid_fixed() {
        let tmp = TempDir::new().unwrap();
        // PID 99999999 is virtually guaranteed not to exist
        std::fs::write(tmp.path().join("run.pid"), "99999999\n").unwrap();
        let (pass, fixed, note) = check_zombie_pid(tmp.path(), true);
        assert!(pass, "note: {}", note);
        assert!(fixed);
        assert!(!tmp.path().join("run.pid").exists());
    }

    #[test]
    fn check_zombie_pid_dead_no_fix_fails() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("run.pid"), "99999999\n").unwrap();
        let (pass, fixed, _) = check_zombie_pid(tmp.path(), false);
        assert!(!pass);
        assert!(!fixed);
    }

    #[test]
    fn check_gitignore_adds_entry() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "target/\n").unwrap();
        let (pass, fixed, _) = check_gitignore(tmp.path(), true);
        assert!(pass);
        assert!(fixed);
        let content = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert!(content.contains(".acs/"));
    }

    #[test]
    fn check_gitignore_already_present() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "target/\n.acs/\n").unwrap();
        let (pass, fixed, _) = check_gitignore(tmp.path(), false);
        assert!(pass);
        assert!(!fixed);
    }

    #[test]
    fn check_gitignore_fails_without_fix() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "target/\n").unwrap();
        let (pass, fixed, _) = check_gitignore(tmp.path(), false);
        assert!(!pass);
        assert!(!fixed);
    }

    #[test]
    fn is_process_alive_current_process() {
        let pid = std::process::id();
        assert!(is_process_alive(pid));
    }

    #[test]
    fn is_process_alive_dead_pid() {
        assert!(!is_process_alive(99_999_999));
    }
}
