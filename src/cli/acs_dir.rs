use anyhow::Result;
use std::path::{Path, PathBuf};

/// Resolve the root `.acs/` directory for the current process.
///
/// Workers run inside git worktrees under `<project>/.acs/worktrees/<worker_id>`,
/// where relative paths like `./.acs/project.db` would point to
/// `<worker_cwd>/.acs/project.db` (non-existent). This helper walks upward
/// until it finds a `.acs/` directory.
pub fn resolve_acs_dir(start: &Path) -> Result<PathBuf> {
    let mut cur = start;
    loop {
        let candidate = cur.join(".acs");
        if candidate.is_dir() {
            return Ok(candidate);
        }

        match cur.parent() {
            Some(parent) => cur = parent,
            None => break,
        }
    }

    anyhow::bail!(".acs/ not found. Run `acs init` first.");
}

