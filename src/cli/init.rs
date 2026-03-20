// src/cli/init.rs
use anyhow::{Context, Result};
use std::fs;

use crate::config::Config;
use crate::db::Db;
use crate::prompts;
use crate::spawner::Spawner;

/// Try to derive a project name from the git remote URL (origin).
/// Strips common URL schemes, `.git` suffix, and returns the final path component.
/// Returns `None` if `git remote get-url origin` fails or produces no usable output.
fn detect_name_from_git_remote(cwd: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout);
    let url = url.trim().trim_end_matches('/');
    // Strip .git suffix
    let url = url.strip_suffix(".git").unwrap_or(url);
    // Take the last path component (handles both https and ssh URLs)
    let name = url.split('/').last().or_else(|| url.split(':').last())?;
    let name = name.trim();
    if name.is_empty() { None } else { Some(name.to_string()) }
}

/// Add `.acs/` to `.gitignore` in `dir` if not already present.
/// Creates `.gitignore` if it does not exist.
fn ensure_gitignore(dir: &std::path::Path) -> Result<()> {
    let gitignore = dir.join(".gitignore");
    let entry = ".acs/";
    if gitignore.exists() {
        let contents = fs::read_to_string(&gitignore)?;
        // Already ignored?
        if contents.lines().any(|l| l.trim() == entry || l.trim() == ".acs") {
            return Ok(());
        }
        // Append
        let separator = if contents.ends_with('\n') { "" } else { "\n" };
        fs::write(&gitignore, format!("{}{}{}\n", contents, separator, entry))?;
    } else {
        fs::write(&gitignore, format!("{}\n", entry))?;
    }
    println!("Added {} to .gitignore", entry);
    Ok(())
}

pub fn execute(spec: Option<String>, auto: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = cwd.join(".acs");

    if acs_dir.exists() {
        anyhow::bail!(".acs/ already exists. Use `acs run` to start agents.");
    }

    // Create directories
    fs::create_dir_all(acs_dir.join("logs"))?;

    // Detect project name: git remote preferred, fall back to directory name
    let project_name = detect_name_from_git_remote(&cwd)
        .or_else(|| {
            cwd.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "project".to_string());

    // Add .acs/ to .gitignore
    ensure_gitignore(&cwd)?;

    // Write config
    let config = Config::default_for(&project_name);
    fs::write(acs_dir.join("config.toml"), config.to_toml())?;

    // Create database
    let db = Db::open(&acs_dir.join("project.db"))?;

    println!("Initialized ACS in .acs/");

    // Read spec if provided
    let spec_text = if let Some(ref spec_path) = spec {
        Some(fs::read_to_string(spec_path).context("Failed to read spec file")?)
    } else {
        None
    };

    if auto || spec.is_some() {
        println!("Bootstrapping project...");

        let tool_path = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "acs".to_string());

        let system_prompt = prompts::bootstrap_prompt(
            &cwd.to_string_lossy(),
            spec_text.as_deref(),
            &tool_path,
        );

        let task_prompt = format!(
            "Analyze the repository at {} and create tickets for all work needed. \
             Use the Bash tool to run `{}` commands as described in your system prompt. \
             IMPORTANT: Always use the Bash tool to call acs commands. Do not try MCP tools.",
            cwd.display(),
            tool_path,
        );

        let spawner = Spawner::new(&cwd, &config.agents.claude_path, &tool_path);
        let mut child = spawner.spawn_claude("bootstrap", &cwd, &task_prompt, &system_prompt)?;

        let status = child.wait()?;

        // Count tickets
        let tickets = db.list_tickets(None)?;
        let count = tickets.len();

        if status.success() {
            println!("Bootstrap complete! Created {} tickets.", count);
        } else {
            println!(
                "Bootstrap exited with code {:?}. Created {} tickets.",
                status.code(),
                count
            );
        }

        db.log_event(
            Some("bootstrap"),
            "bootstrap_complete",
            &format!("Created {} tickets", count),
            None,
        )?;

        // Generate bootstrap summary report.
        if let Err(e) = crate::cli::report::generate_bootstrap_report(&acs_dir, &db) {
            eprintln!("[report] warning: failed to generate bootstrap report: {:#}", e);
        }
    } else {
        println!("Run `acs init --auto` to auto-analyze, or `acs init --spec <file>` to bootstrap from a spec.");
    }

    println!("Run `acs run` to start the AI team.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_name_from_git_remote_url_parsing() {
        // Test the URL-parsing logic mirrors detect_name_from_git_remote
        let parse = |url: &str| -> String {
            let url = url.trim().trim_end_matches('/');
            let url = url.strip_suffix(".git").unwrap_or(url);
            let name = url.split('/').last().or_else(|| url.split(':').last()).unwrap_or("");
            name.to_string()
        };

        assert_eq!(parse("https://github.com/owner/myproject.git"), "myproject");
        assert_eq!(parse("https://github.com/owner/myproject"), "myproject");
        assert_eq!(parse("git@github.com:owner/myproject.git"), "myproject");
        assert_eq!(parse("git@github.com:owner/myproject"), "myproject");
        assert_eq!(parse("https://github.com/owner/repo.git/"), "repo");
    }

    #[test]
    fn test_ensure_gitignore_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        ensure_gitignore(dir.path()).unwrap();
        let contents = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(contents.contains(".acs/"));
    }

    #[test]
    fn test_ensure_gitignore_appends_to_existing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "node_modules/\n").unwrap();
        ensure_gitignore(dir.path()).unwrap();
        let contents = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(contents.contains("node_modules/"));
        assert!(contents.contains(".acs/"));
    }

    #[test]
    fn test_ensure_gitignore_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), ".acs/\nnode_modules/\n").unwrap();
        ensure_gitignore(dir.path()).unwrap();
        let contents = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        // Should not duplicate the entry
        let count = contents.lines().filter(|l| l.trim() == ".acs/").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_ensure_gitignore_idempotent_no_slash() {
        let dir = tempfile::tempdir().unwrap();
        // .acs (without trailing slash) should also be recognized as already present
        std::fs::write(dir.path().join(".gitignore"), ".acs\n").unwrap();
        ensure_gitignore(dir.path()).unwrap();
        let contents = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        let has_slash = contents.lines().any(|l| l.trim() == ".acs/");
        assert!(!has_slash, ".acs already present without slash — should not add .acs/");
    }
}
