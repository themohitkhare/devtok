use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{bail, Context, Result};
use rand::Rng;

use crate::config::{AgentConfig, BackendTemplate, BackendsConfig};

/// Outcome of a `merge_branch` operation.
#[derive(Debug, PartialEq)]
pub enum MergeOutcome {
    /// Branch was rebased onto base and merged successfully.
    Success,
    /// Rebase conflicted — rebase was aborted; branch needs manual rebase.
    RebaseConflict,
    /// Rebase succeeded but the subsequent merge conflicted — merge was aborted.
    MergeConflict,
}

/// Abstraction over subprocess spawning so worker logic can be unit-tested
/// without invoking the real Claude Code binary.
pub trait SpawnProvider {
    fn create_worktree(&self, worker_id: &str, ticket_id: &str) -> Result<PathBuf>;
    fn remove_worktree(&self, worker_id: &str) -> Result<()>;
    fn spawn_provider(
        &self,
        provider: &str,
        model: Option<&str>,
        worker_id: &str,
        worktree: &Path,
        prompt: &str,
        system_prompt: &str,
    ) -> Result<Child>;
    fn log_path(&self, worker_id: &str) -> PathBuf;
}

pub struct Spawner {
    project_dir: PathBuf,
    acs_dir: PathBuf,
    claude_path: String,
    codex_path: Option<String>,
    agent_path: Option<String>,
    /// Custom backend templates keyed by backend name (from `[backends.*]` config).
    backends: BackendsConfig,
    tool_path: String,
}

/// Returns the name of the currently checked-out branch in `project_dir`,
/// or `None` if the repo is in detached-HEAD state or the command fails.
pub fn current_branch(project_dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if s.is_empty() || s == "HEAD" { None } else { Some(s) }
    } else {
        None
    }
}

/// Checks out `base_branch` in `project_dir` so that subsequent git operations
/// (merge, branch listing) target the correct branch.
///
/// Runs `git checkout <base_branch>` with suppressed output.
pub fn checkout_base_branch(project_dir: &Path, base_branch: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["checkout", base_branch])
        .current_dir(project_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("Failed to run git checkout {}", base_branch))?;
    if !status.success() {
        anyhow::bail!("git checkout {} failed in {}", base_branch, project_dir.display());
    }
    Ok(())
}

impl Spawner {
    /// Backwards-compatible constructor: only `claude` is enabled, no custom backends.
    pub fn new(project_dir: &Path, claude_path: &str, tool_path: &str) -> Self {
        let acs_dir = project_dir.join(".acs");
        Spawner {
            project_dir: project_dir.to_path_buf(),
            acs_dir,
            claude_path: claude_path.to_string(),
            codex_path: None,
            agent_path: None,
            tool_path: tool_path.to_string(),
            backends: BackendsConfig::default(),
        }
    }

    pub fn new_with_agent_config(project_dir: &Path, agents: &AgentConfig) -> Self {
        let acs_dir = project_dir.join(".acs");
        let codex_path = (!agents.codex_path.trim().is_empty()).then(|| agents.codex_path.clone());
        let agent_path = (!agents.agent_path.trim().is_empty()).then(|| agents.agent_path.clone());

        Spawner {
            project_dir: project_dir.to_path_buf(),
            acs_dir,
            claude_path: agents.claude_path.clone(),
            codex_path,
            agent_path,
            tool_path: agents.tool_path.clone(),
            backends: BackendsConfig::default(),
        }
    }

    /// Attach custom backend templates loaded from `config.backends`.
    pub fn with_backends(mut self, backends: BackendsConfig) -> Self {
        self.backends = backends;
        self
    }

    /// Creates a git worktree at `.acs/worktrees/{worker_id}` on a new branch
    /// `acs/{ticket_id}-{4_random_hex}`.
    pub fn create_worktree(&self, worker_id: &str, ticket_id: &str) -> Result<PathBuf> {
        let suffix: String = {
            let mut rng = rand::rng();
            format!("{:04x}", rng.random::<u16>())
        };
        let branch_name = format!("acs/{}-{}", ticket_id, suffix);
        let worktree_path = self.acs_dir.join("worktrees").join(worker_id);

        // Ensure parent directory exists
        if let Some(parent) = worktree_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create worktrees directory: {}", parent.display()))?;
        }

        // Remove any stale worktree at this path from a previous run before creating a new one.
        if worktree_path.exists() {
            let _ = Command::new("git")
                .args(["worktree", "remove", "--force"])
                .arg(&worktree_path)
                .current_dir(&self.project_dir)
                .status();
            let _ = std::fs::remove_dir_all(&worktree_path);
            let _ = Command::new("git")
                .args(["worktree", "prune"])
                .current_dir(&self.project_dir)
                .status();
        }

        let status = Command::new("git")
            .arg("worktree")
            .arg("add")
            .arg(&worktree_path)
            .arg("-b")
            .arg(&branch_name)
            .current_dir(&self.project_dir)
            .status()
            .with_context(|| "Failed to run git worktree add")?;

        if !status.success() {
            bail!(
                "git worktree add failed for worker '{}' on branch '{}'",
                worker_id,
                branch_name
            );
        }

        Ok(worktree_path)
    }

    /// Creates a git worktree at `.acs/worktrees/{reviewer_id}` checking out an
    /// *existing* branch (no `-b`). Used by the Tech Lead review flow to review
    /// a worker's branch without creating a new one.
    pub fn create_review_worktree(&self, reviewer_id: &str, branch: &str) -> Result<PathBuf> {
        let worktree_path = self.acs_dir.join("worktrees").join(reviewer_id);

        // Ensure parent directory exists
        if let Some(parent) = worktree_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create worktrees directory: {}", parent.display()))?;
        }

        // Remove any stale worktree at this path before creating a new one.
        if worktree_path.exists() {
            let _ = Command::new("git")
                .args(["worktree", "remove", "--force"])
                .arg(&worktree_path)
                .current_dir(&self.project_dir)
                .status();
            let _ = std::fs::remove_dir_all(&worktree_path);
            let _ = Command::new("git")
                .args(["worktree", "prune"])
                .current_dir(&self.project_dir)
                .status();
        }

        // Checkout the existing branch — no -b flag.
        let status = Command::new("git")
            .arg("worktree")
            .arg("add")
            .arg(&worktree_path)
            .arg(branch)
            .current_dir(&self.project_dir)
            .status()
            .with_context(|| format!("Failed to run git worktree add for branch '{}'", branch))?;

        if !status.success() {
            bail!(
                "git worktree add failed for reviewer '{}' on branch '{}'",
                reviewer_id,
                branch
            );
        }

        Ok(worktree_path)
    }

    /// Removes the git worktree at `.acs/worktrees/{worker_id}`. Errors are
    /// silently ignored if the worktree does not exist.
    pub fn remove_worktree(&self, worker_id: &str) -> Result<()> {
        let worktree_path = self.acs_dir.join("worktrees").join(worker_id);

        let status = Command::new("git")
            .arg("worktree")
            .arg("remove")
            .arg(&worktree_path)
            .arg("--force")
            .current_dir(&self.project_dir)
            .status();

        // Ignore errors — worktree may already be gone
        match status {
            Ok(s) if s.success() => {}
            _ => {
                // Best-effort: also try removing the directory directly
                let _ = fs::remove_dir_all(&worktree_path);
            }
        }

        Ok(())
    }

    /// Spawns a provider subprocess inside `worktree`.
    ///
    /// Provider resolution order:
    /// 1. If `provider` matches a key in `self.backends`, use the command template
    ///    with `{prompt}` and `{system_prompt}` expanded (custom backend).
    /// 2. Built-in providers: `claude`, `codex`, `agent`.
    ///
    /// stdout/stderr are redirected to `.acs/logs/{worker_id}.log`. Returns the `Child` handle.
    pub fn spawn_provider(
        &self,
        provider: &str,
        model: Option<&str>,
        worker_id: &str,
        worktree: &Path,
        prompt: &str,
        system_prompt: &str,
    ) -> Result<Child> {
        let logs_dir = self.acs_dir.join("logs");
        fs::create_dir_all(&logs_dir)
            .with_context(|| format!("Failed to create logs directory: {}", logs_dir.display()))?;

        let log_path = logs_dir.join(format!("{}.log", worker_id));
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("Failed to open log file: {}", log_path.display()))?;

        // Clone the file handle for stderr
        let log_file_stderr = log_file
            .try_clone()
            .with_context(|| "Failed to clone log file handle for stderr")?;

        // Combine "system prompt" + ticket instructions because codex/agent do not
        // necessarily support Claude-style `--append-system-prompt`.
        let combined_prompt = format!("{}\n\n{}", system_prompt, prompt);

        // --- Custom backend via [backends.*] config section ---
        if let Some(template) = self.backends.definitions.get(provider) {
            let (program, args) = template
                .expand_with_model(prompt, system_prompt, model)
                .ok_or_else(|| anyhow::anyhow!("backend '{}' has an empty command template", provider))?;
            let mut cmd = Command::new(&program);
            cmd.args(&args)
                .current_dir(worktree)
                .stdout(Stdio::from(log_file))
                .stderr(Stdio::from(log_file_stderr));
            return cmd
                .spawn()
                .with_context(|| format!("Failed to spawn custom backend '{}' (program: '{}') in '{}'", provider, program, worktree.display()));
        }

        let child = match provider {
            "claude" => {
                // Claude Code uses `-p` for non-interactive prompt printing.
                let mut cmd = Command::new(&self.claude_path);
                cmd.arg("-p")
                    .arg(prompt)
                    .arg("--append-system-prompt")
                    .arg(system_prompt)
                    .arg("--dangerously-skip-permissions")
                    .arg("--output-format")
                    .arg("json");
                if let Some(m) = model {
                    cmd.arg("--model").arg(m);
                }
                cmd.current_dir(worktree)
                    .stdout(Stdio::from(log_file))
                    .stderr(Stdio::from(log_file_stderr))
                    .spawn()
            }
            "codex" => {
                let codex_path = self.codex_path.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("codex provider is not configured (codex_path is empty)")
                })?;

                // Codex exec:
                // - use `--json` so logs are JSONL-ish
                // - set sandbox to allow editing
                // - pass model via `-m/--model`
                let mut cmd = Command::new(codex_path);
                cmd.arg("exec")
                    .arg("--json")
                    .arg("--sandbox")
                    .arg("workspace-write")
                    .arg("--dangerously-bypass-approvals-and-sandbox")
                    .arg("-C")
                    .arg(worktree)
                    .stdout(Stdio::from(log_file))
                    .stderr(Stdio::from(log_file_stderr));
                if let Some(m) = model {
                    cmd.arg("-m").arg(m);
                }
                cmd.arg(combined_prompt).spawn()
            }
            "agent" => {
                let agent_path = self.agent_path.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("agent provider is not configured (agent_path is empty)")
                })?;

                // Cursor agent:
                // - `--print` + `--output-format json` for non-interactive output
                // - `--workspace` to ensure it edits within the worktree
                let mut cmd = Command::new(agent_path);
                cmd.arg("--print")
                    .arg("--output-format")
                    .arg("json")
                    .arg("--force")
                    .arg("--workspace")
                    .arg(worktree)
                    .current_dir(worktree)
                    .stdout(Stdio::from(log_file))
                    .stderr(Stdio::from(log_file_stderr));
                if let Some(m) = model {
                    cmd.arg("--model").arg(m);
                }
                cmd.arg(combined_prompt).spawn()
            }
            other => bail!("unknown provider '{}' (not a built-in and not in [backends] config)", other),
        }
        .with_context(|| format!("Failed to spawn provider '{}' in '{}'", provider, worktree.display()))?;

        Ok(child)
    }

    pub fn spawn_claude(
        &self,
        worker_id: &str,
        worktree: &Path,
        prompt: &str,
        system_prompt: &str,
    ) -> Result<Child> {
        self.spawn_provider("claude", None, worker_id, worktree, prompt, system_prompt)
    }

    /// Spawn a generic agent backend, using the template's command/args directly.
    /// This is the preferred path for new [backends.*] config entries.
    pub fn spawn_agent(
        &self,
        backend: &BackendTemplate,
        model: Option<&str>,
        worker_id: &str,
        worktree: &std::path::Path,
        prompt: &str,
        system_prompt: &str,
    ) -> Result<std::process::Child> {
        let log_path = self.log_path(worker_id);
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create logs directory: {}", parent.display()))?;
        }
        let log_file = OpenOptions::new()
            .create(true).append(true).open(&log_path)
            .with_context(|| format!("open log {}", log_path.display()))?;
        let log_file_stderr = log_file.try_clone()
            .with_context(|| "clone log fd")?;

        let (program, expanded_args) = backend
            .expand_with_model(prompt, system_prompt, model)
            .ok_or_else(|| anyhow::anyhow!("backend has an empty command template"))?;

        let mut cmd = Command::new(&program);
        cmd.args(&expanded_args)
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_file_stderr));

        if backend.cwd_in_worktree {
            cmd.current_dir(worktree);
        }

        cmd.spawn()
            .with_context(|| format!("spawn backend command '{}'", program))
    }

    /// Sends SIGTERM to the given PID, waits up to 5 seconds, then sends
    /// SIGKILL if the process is still alive.
    pub fn kill_process(pid: u32) -> Result<()> {
        // Send SIGTERM
        let term_status = Command::new("kill")
            .arg(pid.to_string())
            .status();

        match term_status {
            Err(e) => {
                // kill(1) not found or other OS error — best effort
                eprintln!("Warning: could not send SIGTERM to {}: {}", pid, e);
            }
            Ok(s) if !s.success() => {
                // Process may already be gone — not an error
                return Ok(());
            }
            Ok(_) => {}
        }

        // Wait up to 5 seconds for process to exit
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            // Check if the process is still alive via kill -0
            let alive = Command::new("kill")
                .arg("-0")
                .arg(pid.to_string())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

            if !alive {
                return Ok(());
            }

            if std::time::Instant::now() >= deadline {
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        // SIGKILL
        let _ = Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .status();

        Ok(())
    }

    /// Returns the path to the log file for the given worker.
    pub fn log_path(&self, worker_id: &str) -> PathBuf {
        self.acs_dir.join("logs").join(format!("{}.log", worker_id))
    }

    /// Returns the resolved path to the `acs` tool binary.
    pub fn tool_path(&self) -> &str {
        &self.tool_path
    }

    /// Finds the branch name matching `acs/{ticket_id}-*` by listing git branches.
    /// Returns `None` if no matching branch exists.
    pub fn find_branch_for_ticket(&self, ticket_id: &str) -> Result<Option<String>> {
        let prefix = format!("acs/{}-", ticket_id);
        let output = Command::new("git")
            .arg("branch")
            .arg("--list")
            .arg(format!("{}*", prefix))
            .current_dir(&self.project_dir)
            .output()
            .with_context(|| "Failed to run git branch --list")?;

        if !output.status.success() {
            bail!("git branch --list failed");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // git branch output has "  branch" or "* branch" prefix per line
        let branch = stdout
            .lines()
            .map(|l| l.trim().trim_start_matches("* ").trim())
            .find(|l| l.starts_with(&prefix))
            .map(|s| s.to_string());

        Ok(branch)
    }

    /// Rebases `branch` onto `base_branch`, then merges using `--no-ff`.
    ///
    /// This is the primary defence against stale-history conflicts: worker
    /// branches are created from a point in time, then main advances. By
    /// rebasing first we ensure the worker's commits apply cleanly on top of
    /// the current main before we attempt the merge.
    ///
    /// Strategy:
    /// 1. `git fetch origin <base_branch>` (best-effort — no remote is fine).
    /// 2. Checkout `branch`, run `git rebase <base_branch>`.
    ///    On conflict: abort rebase, restore `base_branch`, return `RebaseConflict`.
    /// 3. On successful rebase: checkout `base_branch`, run `git merge --no-ff`.
    ///    On merge conflict: abort, return `MergeConflict`.
    /// 4. Return `Success`.
    pub fn merge_branch(&self, branch: &str, base_branch: &str) -> Result<MergeOutcome> {
        // Step 1: best-effort fetch so local base_branch is current.
        let _ = Command::new("git")
            .args(["fetch", "origin", base_branch])
            .current_dir(&self.project_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        // Step 2: checkout the worker branch and rebase onto base_branch.
        let checkout_ok = Command::new("git")
            .args(["checkout", branch])
            .current_dir(&self.project_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("Failed to git checkout {}", branch))?
            .success();

        if !checkout_ok {
            // Branch disappeared between find and merge.
            let _ = checkout_base_branch(&self.project_dir, base_branch);
            return Ok(MergeOutcome::RebaseConflict);
        }

        let rebase_ok = Command::new("git")
            .args(["rebase", base_branch])
            .current_dir(&self.project_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("Failed to run git rebase {}", base_branch))?
            .success();

        if !rebase_ok {
            let _ = Command::new("git")
                .args(["rebase", "--abort"])
                .current_dir(&self.project_dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = checkout_base_branch(&self.project_dir, base_branch);
            return Ok(MergeOutcome::RebaseConflict);
        }

        eprintln!("[manager] rebased {} onto {} before merge", branch, base_branch);

        // Step 3: back to base_branch, then merge.
        checkout_base_branch(&self.project_dir, base_branch)?;

        let merge_ok = Command::new("git")
            .args(["merge", "--no-ff", branch])
            .current_dir(&self.project_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .with_context(|| format!("Failed to run git merge --no-ff {}", branch))?
            .success();

        if merge_ok {
            return Ok(MergeOutcome::Success);
        }

        // Merge failed — abort to restore clean state.
        let _ = Command::new("git")
            .args(["merge", "--abort"])
            .current_dir(&self.project_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        Ok(MergeOutcome::MergeConflict)
    }

    /// Deletes a local branch. Best-effort — errors are ignored.
    pub fn delete_branch(&self, branch: &str) {
        let _ = Command::new("git")
            // Merge conflicts can leave the branch "not fully merged".
            // Use -D so we can always discard the failed topic branch.
            .args(["branch", "-D", branch])
            .current_dir(&self.project_dir)
            .status();
    }
}

impl SpawnProvider for Spawner {
    fn create_worktree(&self, worker_id: &str, ticket_id: &str) -> Result<PathBuf> {
        self.create_worktree(worker_id, ticket_id)
    }
    fn remove_worktree(&self, worker_id: &str) -> Result<()> {
        self.remove_worktree(worker_id)
    }
    fn spawn_provider(
        &self,
        provider: &str,
        model: Option<&str>,
        worker_id: &str,
        worktree: &Path,
        prompt: &str,
        system_prompt: &str,
    ) -> Result<Child> {
        self.spawn_provider(provider, model, worker_id, worktree, prompt, system_prompt)
    }
    fn log_path(&self, worker_id: &str) -> PathBuf {
        self.log_path(worker_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use tempfile::TempDir;

    fn make_spawner(project_dir: &Path) -> Spawner {
        Spawner::new(project_dir, "/usr/bin/claude", "/usr/bin/acs")
    }

    // ── Spawner::new / field accessors ──────────────────────────────

    #[test]
    fn new_sets_acs_dir_under_project() {
        let s = make_spawner(Path::new("/tmp/myproject"));
        assert_eq!(s.acs_dir, PathBuf::from("/tmp/myproject/.acs"));
        assert_eq!(s.project_dir, PathBuf::from("/tmp/myproject"));
    }

    #[test]
    fn tool_path_returns_configured_value() {
        let s = Spawner::new(Path::new("/x"), "claude", "/my/acs");
        assert_eq!(s.tool_path(), "/my/acs");
    }

    #[test]
    fn log_path_includes_worker_id() {
        let s = make_spawner(Path::new("/proj"));
        let p = s.log_path("w-42");
        assert_eq!(p, PathBuf::from("/proj/.acs/logs/w-42.log"));
    }

    // ── Branch naming (random suffix) ───────────────────────────────

    #[test]
    fn create_worktree_branch_has_random_4_hex_suffix() {
        // We can't call create_worktree without a real repo, but we can
        // replicate the branch-naming logic and verify its format.
        let mut seen = HashSet::new();
        for _ in 0..20 {
            let mut rng = rand::rng();
            let suffix = format!("{:04x}", rng.random::<u16>());
            // Must be exactly 4 hex characters
            assert_eq!(suffix.len(), 4, "suffix '{}' is not 4 chars", suffix);
            assert!(
                suffix.chars().all(|c| c.is_ascii_hexdigit()),
                "suffix '{}' contains non-hex chars",
                suffix
            );
            let branch = format!("acs/{}-{}", "t-001", suffix);
            assert!(branch.starts_with("acs/t-001-"));
            seen.insert(suffix);
        }
        // With 2^16 possibilities, 20 draws should produce at least 2 distinct values
        assert!(seen.len() >= 2, "expected randomness, got only {:?}", seen);
    }

    // ── create_worktree in a real git repo ──────────────────────────

    #[test]
    fn create_worktree_produces_correct_path_and_branch() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();

        // Initialise a minimal git repo with one commit
        Command::new("git").args(["init"]).current_dir(repo).output().unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(repo)
            .output()
            .unwrap();

        let s = Spawner::new(repo, "claude", "acs");
        let wt_path = s.create_worktree("w-1", "t-007").unwrap();

        // Worktree directory must exist
        assert!(wt_path.is_dir(), "worktree dir should exist");
        assert_eq!(wt_path, repo.join(".acs/worktrees/w-1"));

        // The branch should be listed by git branch
        let out = Command::new("git")
            .args(["branch", "--list", "acs/t-007-*"])
            .current_dir(repo)
            .output()
            .unwrap();
        let branches = String::from_utf8_lossy(&out.stdout);
        assert!(
            branches.contains("acs/t-007-"),
            "expected acs/t-007-XXXX branch, got: {}",
            branches
        );
    }

    // ── remove_worktree ─────────────────────────────────────────────

    #[test]
    fn remove_worktree_succeeds_when_worktree_exists() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();

        Command::new("git").args(["init"]).current_dir(repo).output().unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(repo)
            .output()
            .unwrap();

        let s = Spawner::new(repo, "claude", "acs");
        s.create_worktree("w-rm", "t-rm").unwrap();

        // Remove should succeed
        s.remove_worktree("w-rm").unwrap();

        // Directory should be gone
        let wt = repo.join(".acs/worktrees/w-rm");
        assert!(!wt.exists(), "worktree dir should be removed");
    }

    #[test]
    fn remove_worktree_handles_missing_worktree_gracefully() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();

        Command::new("git").args(["init"]).current_dir(repo).output().unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(repo)
            .output()
            .unwrap();

        let s = Spawner::new(repo, "claude", "acs");

        // Removing a worktree that was never created should not error
        let result = s.remove_worktree("nonexistent");
        assert!(result.is_ok(), "remove_worktree should not error on missing worktree");
    }

    // ── spawn_claude builds correct command ─────────────────────────

    #[test]
    fn spawn_claude_creates_log_file_and_uses_correct_flags() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        let worktree = tmp.path(); // use same dir as worktree stand-in

        // Use `echo` as the "claude" binary so spawn succeeds without the real tool
        let s = Spawner::new(project, "echo", "acs");

        let child = s
            .spawn_claude("w-test", worktree, "do stuff", "you are a bot")
            .unwrap();

        // Log file should have been created
        let log = s.log_path("w-test");
        assert!(log.exists(), "log file should be created at {:?}", log);

        // Wait for echo to finish and capture output from the log
        let mut child = child;
        child.wait().unwrap();

        let log_content = fs::read_to_string(&log).unwrap();
        // `echo` prints all its args space-separated, so the log should contain
        // every flag we pass
        assert!(log_content.contains("-p"), "should contain -p flag");
        assert!(log_content.contains("do stuff"), "should contain the prompt");
        assert!(
            log_content.contains("--append-system-prompt"),
            "should contain --append-system-prompt"
        );
        assert!(
            log_content.contains("you are a bot"),
            "should contain system prompt text"
        );
        assert!(
            log_content.contains("--dangerously-skip-permissions"),
            "should contain --dangerously-skip-permissions"
        );
        assert!(
            log_content.contains("--output-format"),
            "should contain --output-format"
        );
        assert!(log_content.contains("json"), "should contain json output format");
    }

    #[test]
    fn spawn_claude_fails_with_bad_binary() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();

        let s = Spawner::new(project, "/nonexistent/claude-binary", "acs");

        let result = s.spawn_claude("w-bad", tmp.path(), "hi", "sys");
        assert!(result.is_err(), "should fail when binary does not exist");
    }

    // ── Custom backend via [backends] config ─────────────────────────

    #[test]
    fn spawn_provider_uses_custom_backend_when_defined() {
        use crate::config::BackendTemplate;

        let tmp = TempDir::new().unwrap();
        let project = tmp.path();

        let mut backends = BackendsConfig::default();
        backends.definitions.insert(
            "my-echo".to_string(),
            BackendTemplate {
                // Use `echo` so the process succeeds without a real AI binary.
                // Template: echo {prompt} {system_prompt}
                command: "echo {prompt} {system_prompt}".to_string(),
                ..Default::default()
            },
        );

        let s = Spawner::new(project, "claude", "acs").with_backends(backends);

        let mut child = s
            .spawn_provider("my-echo", None, "w-cust", tmp.path(), "hello prompt", "sys context")
            .expect("custom backend should spawn");
        child.wait().unwrap();

        let log = s.log_path("w-cust");
        assert!(log.exists(), "log file should exist");
        let content = fs::read_to_string(&log).unwrap();
        assert!(content.contains("hello prompt"), "log should contain expanded prompt");
        assert!(content.contains("sys context"), "log should contain expanded system_prompt");
    }

    #[test]
    fn spawn_provider_custom_backend_takes_priority_over_builtin_name() {
        use crate::config::BackendTemplate;

        let tmp = TempDir::new().unwrap();
        let project = tmp.path();

        // Register a custom backend named "claude" — it should override the built-in
        let mut backends = BackendsConfig::default();
        backends.definitions.insert(
            "claude".to_string(),
            BackendTemplate {
                command: "echo custom-override {prompt}".to_string(),
                ..Default::default()
            },
        );

        let s = Spawner::new(project, "/nonexistent/real-claude", "acs").with_backends(backends);

        // Should use the custom backend (echo), not the nonexistent real claude path.
        let mut child = s
            .spawn_provider("claude", None, "w-override", tmp.path(), "my task", "sys")
            .expect("custom override should use echo, not real claude");
        child.wait().unwrap();

        let log = s.log_path("w-override");
        let content = fs::read_to_string(&log).unwrap();
        assert!(content.contains("custom-override"), "should have used custom template");
        assert!(content.contains("my task"), "should have expanded the prompt placeholder");
    }

    #[test]
    fn spawn_provider_unknown_without_backend_returns_error() {
        let tmp = TempDir::new().unwrap();
        let s = Spawner::new(tmp.path(), "claude", "acs");
        let result = s.spawn_provider("totally-unknown", None, "w-unk", tmp.path(), "p", "s");
        assert!(result.is_err(), "unknown provider with no backend entry should fail");
    }

    #[test]
    fn with_backends_sets_backends_map() {
        use crate::config::BackendTemplate;

        let tmp = TempDir::new().unwrap();
        let mut backends = BackendsConfig::default();
        backends.definitions.insert("foo".to_string(), BackendTemplate { command: "foo {prompt}".to_string(), ..Default::default() });

        let s = Spawner::new(tmp.path(), "claude", "acs").with_backends(backends);
        assert!(s.backends.definitions.contains_key("foo"));
    }

    // -- spawn_agent --

    #[test]
    fn spawn_agent_uses_backend_template_directly() {
        use crate::config::BackendTemplate;

        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        let s = Spawner::new(project, "claude", "acs");

        let backend = BackendTemplate {
            command: "echo".to_string(),
            args: vec!["{prompt}".to_string()],
            cwd_in_worktree: false,
            output_format: "text".to_string(),
        };

        let mut child = s
            .spawn_agent(&backend, None, "w-agent", tmp.path(), "agent-prompt", "sys")
            .expect("spawn_agent should work with echo");
        child.wait().unwrap();

        let log = s.log_path("w-agent");
        assert!(log.exists(), "log file should exist");
        let content = fs::read_to_string(&log).unwrap();
        assert!(content.contains("agent-prompt"), "log should contain the prompt");
    }
}
