use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{bail, Context, Result};
use rand::Rng;

pub struct Spawner {
    project_dir: PathBuf,
    acs_dir: PathBuf,
    claude_path: String,
    tool_path: String,
}

impl Spawner {
    pub fn new(project_dir: &Path, claude_path: &str, tool_path: &str) -> Self {
        let acs_dir = project_dir.join(".acs");
        Spawner {
            project_dir: project_dir.to_path_buf(),
            acs_dir,
            claude_path: claude_path.to_string(),
            tool_path: tool_path.to_string(),
        }
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

    /// Spawns a `claude` subprocess inside `worktree`, redirecting stdout and
    /// stderr to `.acs/logs/{worker_id}.log`. Returns the `Child` handle.
    pub fn spawn_claude(
        &self,
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

        let child = Command::new(&self.claude_path)
            .arg("-p")
            .arg(prompt)
            .arg("--append-system-prompt")
            .arg(system_prompt)
            .arg("--dangerously-skip-permissions")
            .arg("--output-format")
            .arg("json")
            .current_dir(worktree)
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_file_stderr))
            .spawn()
            .with_context(|| {
                format!(
                    "Failed to spawn claude process '{}' in '{}'",
                    self.claude_path,
                    worktree.display()
                )
            })?;

        Ok(child)
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
}
