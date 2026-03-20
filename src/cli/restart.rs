use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Config;

pub fn execute(workers: Option<usize>, backend: Option<String>, wait_seconds: u64) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let config = Config::load(&acs_dir.join("config.toml"))?;
    let workers = workers.unwrap_or(config.project.default_workers);

    stop_existing_if_any(&acs_dir, wait_seconds)?;
    println!(
        "Restarting ACS with {} workers{}...",
        workers,
        backend
            .as_ref()
            .map(|b| format!(" (backend: {})", b))
            .unwrap_or_default()
    );

    crate::cli::run::execute(Some(workers), backend, false, 1, None)
}

pub fn stop_existing_if_any(acs_dir: &Path, wait_seconds: u64) -> Result<()> {
    let pid_path = acs_dir.join("run.pid");
    if !pid_path.is_file() {
        println!("No running ACS instance found (no run.pid).");
        return Ok(());
    }

    let pid_str = fs::read_to_string(&pid_path)
        .with_context(|| format!("Failed to read {}", pid_path.display()))?;
    let pid = match pid_str.trim().parse::<u32>() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Warning: invalid run.pid contents; removing stale file.");
            let _ = fs::remove_file(&pid_path);
            return Ok(());
        }
    };

    if pid == std::process::id() {
        eprintln!("Warning: run.pid points to current process; skipping stop.");
        return Ok(());
    }

    if !is_alive(pid) {
        eprintln!(
            "Warning: run.pid points to non-running pid {}; removing stale file.",
            pid
        );
        let _ = fs::remove_file(&pid_path);
        return Ok(());
    }

    println!("Stopping running ACS instance (pid {})...", pid);
    let _ = Command::new("kill").args(["-2", &pid.to_string()]).status();

    let deadline = Instant::now() + Duration::from_secs(wait_seconds);
    while Instant::now() < deadline {
        if !is_alive(pid) {
            let _ = fs::remove_file(&pid_path);
            println!("Previous ACS instance stopped.");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(200));
    }

    eprintln!(
        "Warning: process {} did not stop within {}s, sending SIGKILL.",
        pid, wait_seconds
    );
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
    let _ = fs::remove_file(&pid_path);
    Ok(())
}

fn is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
