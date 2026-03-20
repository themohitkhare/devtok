use anyhow::{anyhow, bail, Context, Result};
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use semver::Version;
use serde::Deserialize;
use std::fs;
use std::io::Read;
use std::path::Path;
use tar::Archive;
use tempfile::NamedTempFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetInfo {
    pub name: String,
    pub download_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub version: Version,
    pub asset: AssetInfo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheck {
    pub current_version: Version,
    pub latest_release: ReleaseInfo,
    pub update_available: bool,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseResponse {
    tag_name: String,
    assets: Vec<GithubAssetResponse>,
}

#[derive(Debug, Deserialize)]
struct GithubAssetResponse {
    name: String,
    browser_download_url: String,
}

pub fn current_version() -> Result<Version> {
    parse_version(env!("CARGO_PKG_VERSION"))
}

pub fn default_repo() -> Result<String> {
    parse_repo_from_url(env!("CARGO_PKG_REPOSITORY"))
}

pub fn current_target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("macos-arm64"),
        ("macos", "x86_64") => Ok("macos-x64"),
        ("linux", "x86_64") => Ok("linux-x64"),
        (os, arch) => bail!("self-update is not supported on {os}/{arch}"),
    }
}

pub fn asset_name(version: &Version, target: &str) -> String {
    format!("acs-{version}-{target}.tar.gz")
}

pub fn check_for_update(
    client: &Client,
    api_base: &str,
    repo: &str,
    current_version: &Version,
    target: &str,
) -> Result<UpdateCheck> {
    let latest_release = fetch_latest_release(client, api_base, repo, target)?;
    Ok(UpdateCheck {
        current_version: current_version.clone(),
        update_available: latest_release.version > *current_version,
        latest_release,
    })
}

pub fn install_release(client: &Client, asset: &AssetInfo, install_path: &Path) -> Result<()> {
    let response = client
        .get(&asset.download_url)
        .send()
        .with_context(|| format!("failed to download release asset {}", asset.download_url))?
        .error_for_status()
        .with_context(|| format!("release asset request failed for {}", asset.download_url))?;

    let bytes = response
        .bytes()
        .context("failed to read release asset response body")?;
    install_archive(bytes.as_ref(), install_path)
}

pub fn install_archive(archive_bytes: &[u8], install_path: &Path) -> Result<()> {
    let archive_reader = GzDecoder::new(std::io::Cursor::new(archive_bytes));
    let mut archive = Archive::new(archive_reader);
    let mut binary_contents = Vec::new();

    for entry in archive
        .entries()
        .context("failed to read tar archive entries")?
    {
        let mut entry = entry.context("failed to open tar archive entry")?;
        let path = entry
            .path()
            .context("failed to inspect tar archive entry path")?;
        if path.file_name().and_then(|name| name.to_str()) == Some("acs") {
            entry
                .read_to_end(&mut binary_contents)
                .context("failed to read bundled acs binary from tar archive")?;
            break;
        }
    }

    if binary_contents.is_empty() {
        bail!("release archive does not contain an acs binary");
    }

    let install_dir = install_path.parent().ok_or_else(|| {
        anyhow!(
            "install path {} has no parent directory",
            install_path.display()
        )
    })?;
    fs::create_dir_all(install_dir).with_context(|| {
        format!(
            "failed to create install directory {}",
            install_dir.display()
        )
    })?;

    let mut temp_file = NamedTempFile::new_in(install_dir)
        .with_context(|| format!("failed to create temp file in {}", install_dir.display()))?;
    use std::io::Write;
    temp_file
        .write_all(&binary_contents)
        .with_context(|| format!("failed to write temp binary {}", install_path.display()))?;
    temp_file
        .flush()
        .with_context(|| format!("failed to flush temp binary {}", install_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp_file
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o755))
            .with_context(|| format!("failed to chmod {}", install_path.display()))?;
    }

    temp_file
        .persist(install_path)
        .map_err(|e| e.error)
        .with_context(|| {
            format!(
                "failed to install updated binary to {}",
                install_path.display()
            )
        })?;

    Ok(())
}

fn fetch_latest_release(
    client: &Client,
    api_base: &str,
    repo: &str,
    target: &str,
) -> Result<ReleaseInfo> {
    let api_base = api_base.trim_end_matches('/');
    let url = format!("{api_base}/repos/{repo}/releases/latest");
    let release: GithubReleaseResponse = client
        .get(&url)
        .send()
        .with_context(|| format!("failed to query GitHub Releases API at {url}"))?
        .error_for_status()
        .with_context(|| format!("GitHub Releases API returned an error for {url}"))?
        .json()
        .context("failed to decode GitHub release response")?;

    let version = parse_version(release.tag_name.trim_start_matches('v'))?;
    let expected_asset = asset_name(&version, target);
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name == expected_asset)
        .map(|asset| AssetInfo {
            name: asset.name,
            download_url: asset.browser_download_url,
        })
        .ok_or_else(|| anyhow!("latest release is missing asset {expected_asset}"))?;

    Ok(ReleaseInfo {
        tag_name: release.tag_name,
        version,
        asset,
    })
}

fn parse_version(raw: &str) -> Result<Version> {
    Version::parse(raw).with_context(|| format!("invalid version string {raw}"))
}

fn parse_repo_from_url(url: &str) -> Result<String> {
    let trimmed = url.trim_end_matches('/').trim_end_matches(".git");
    let repo = trimmed
        .split_once("github.com/")
        .map(|(_, path)| path)
        .or_else(|| trimmed.rsplit_once(':').map(|(_, path)| path))
        .unwrap_or(trimmed);

    if repo.split('/').count() != 2 {
        bail!("could not derive GitHub owner/repo from repository URL {url}");
    }

    Ok(repo.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::collections::HashMap;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[test]
    fn parses_github_https_repo_url() {
        assert_eq!(
            parse_repo_from_url("https://github.com/themohitkhare/devtok").unwrap(),
            "themohitkhare/devtok"
        );
    }

    #[test]
    fn parses_github_ssh_repo_url() {
        assert_eq!(
            parse_repo_from_url("git@github.com:themohitkhare/devtok.git").unwrap(),
            "themohitkhare/devtok"
        );
    }

    #[test]
    fn computes_expected_asset_name() {
        let version = Version::parse("1.2.3").unwrap();
        assert_eq!(
            asset_name(&version, "linux-x64"),
            "acs-1.2.3-linux-x64.tar.gz"
        );
    }

    #[test]
    fn installs_binary_from_tarball() {
        let archive = archive_with_binary(b"#!/bin/sh\necho installed\n");
        let temp_dir = tempfile::tempdir().unwrap();
        let install_path = temp_dir.path().join("acs");

        install_archive(&archive, &install_path).unwrap();

        assert_eq!(
            fs::read(&install_path).unwrap(),
            b"#!/bin/sh\necho installed\n"
        );
    }

    #[test]
    fn check_for_update_selects_matching_asset() {
        let release_json = serde_json::json!({
            "tag_name": "v0.2.0",
            "assets": [
                {
                    "name": "acs-0.2.0-linux-x64.tar.gz",
                    "browser_download_url": "http://127.0.0.1/acs-0.2.0-linux-x64.tar.gz"
                }
            ]
        });
        let server = spawn_server(vec![(
            "/repos/example/acs/releases/latest".to_string(),
            http_ok_json(release_json.to_string().into_bytes(), "application/json"),
        )]);
        let client = github_client().unwrap();
        let current = Version::parse("0.1.0").unwrap();

        let result = check_for_update(
            &client,
            &server.base_url,
            "example/acs",
            &current,
            "linux-x64",
        )
        .unwrap();

        assert!(result.update_available);
        assert_eq!(
            result.latest_release.version,
            Version::parse("0.2.0").unwrap()
        );
        assert_eq!(
            result.latest_release.asset.name,
            "acs-0.2.0-linux-x64.tar.gz"
        );
    }

    fn archive_with_binary(contents: &[u8]) -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o755);
        header.set_size(contents.len() as u64);
        header.set_cksum();
        builder.append_data(&mut header, "acs", contents).unwrap();
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap()
    }

    fn github_client() -> Result<Client> {
        Client::builder()
            .user_agent("acs-tests")
            .build()
            .context("failed to build GitHub API client")
    }

    struct TestServer {
        base_url: String,
        handle: thread::JoinHandle<()>,
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            let _ = self.handle.thread().id();
        }
    }

    fn spawn_server(routes: Vec<(String, Vec<u8>)>) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let routes = Arc::new(Mutex::new(
            routes.into_iter().collect::<HashMap<String, Vec<u8>>>(),
        ));
        let handle = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0_u8; 2048];
                let size = stream.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..size]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap()
                    .to_string();
                let body = routes.lock().unwrap().remove(&path).unwrap();
                stream.write_all(&body).unwrap();
            }
        });

        TestServer {
            base_url: format!("http://{}", addr),
            handle,
        }
    }

    fn http_ok_json(body: Vec<u8>, content_type: &str) -> Vec<u8> {
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\n\r\n",
            body.len(),
            content_type
        );
        [headers.into_bytes(), body].concat()
    }
}
