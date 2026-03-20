use crate::release;
use anyhow::{Context, Result};
use clap::Args;
use reqwest::blocking::Client;
use serde_json::json;
use std::path::PathBuf;

#[derive(Args)]
pub struct UpdateArgs {
    /// Check whether an update is available without installing it.
    #[arg(long)]
    pub check: bool,
    /// Override the GitHub owner/repo used for release discovery.
    #[arg(long)]
    pub repo: Option<String>,
    /// Override the GitHub API base URL.
    #[arg(long, default_value = "https://api.github.com")]
    pub github_api_base: String,
    /// Override the install path. Defaults to the current executable.
    #[arg(long)]
    pub install_path: Option<PathBuf>,
}

pub fn execute(args: UpdateArgs) -> Result<()> {
    let client = github_client()?;
    let current_version = release::current_version()?;
    let repo = args.repo.unwrap_or(release::default_repo()?);
    let target = release::current_target()?;
    let update = release::check_for_update(
        &client,
        &args.github_api_base,
        &repo,
        &current_version,
        target,
    )?;

    if args.check {
        let status = if update.update_available {
            "update_available"
        } else {
            "up_to_date"
        };
        println!(
            "{}",
            json!({
                "status": status,
                "current_version": current_version.to_string(),
                "latest_version": update.latest_release.version.to_string(),
                "target": target,
                "asset": update.latest_release.asset.name,
            })
        );
        return Ok(());
    }

    if !update.update_available {
        println!(
            "{}",
            json!({
                "status": "up_to_date",
                "current_version": current_version.to_string(),
                "latest_version": update.latest_release.version.to_string(),
                "target": target,
            })
        );
        return Ok(());
    }

    let install_path = match args.install_path {
        Some(path) => path,
        None => std::env::current_exe().context("failed to locate current acs executable")?,
    };
    release::install_release(&client, &update.latest_release.asset, &install_path)?;
    println!(
        "{}",
        json!({
            "status": "updated",
            "from": current_version.to_string(),
            "to": update.latest_release.version.to_string(),
            "path": install_path.display().to_string(),
            "target": target,
        })
    );

    Ok(())
}

fn github_client() -> Result<Client> {
    Client::builder()
        .user_agent(format!("acs/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to build GitHub API client")
}
