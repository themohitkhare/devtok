use anyhow::Result;

pub fn execute(wait_seconds: u64) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    crate::cli::restart::stop_existing_if_any(&acs_dir, wait_seconds)
}
