use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

fn acs_bin() -> String {
    format!("{}", env!("CARGO_BIN_EXE_acs"))
}

#[test]
fn test_update_check_reports_latest_release() {
    let target = acs::release::current_target().unwrap();
    let server = TestServer::start(target, false);

    let output = Command::new(acs_bin())
        .args([
            "update",
            "--check",
            "--repo",
            "example/acs",
            "--github-api-base",
            &server.base_url,
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "update --check failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["status"], "update_available");
    assert_eq!(json["latest_version"], "0.2.0");
}

#[test]
fn test_update_installs_latest_release_to_override_path() {
    let target = acs::release::current_target().unwrap();
    let server = TestServer::start(target, true);
    let temp_dir = tempfile::tempdir().unwrap();
    let install_path = temp_dir.path().join("acs");

    let output = Command::new(acs_bin())
        .args([
            "update",
            "--repo",
            "example/acs",
            "--github-api-base",
            &server.base_url,
            "--install-path",
            install_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "update install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&install_path).unwrap(), b"updated-acs-binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["status"], "updated");
    assert_eq!(json["to"], "0.2.0");
}

struct TestServer {
    base_url: String,
    _handle: thread::JoinHandle<()>,
}

impl TestServer {
    fn start(target: &str, include_asset: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);
        let asset_name = format!("acs-0.2.0-{target}.tar.gz");
        let asset_path = format!("/downloads/{asset_name}");
        let routes = Arc::new(Mutex::new(build_routes(
            &base_url,
            &asset_name,
            &asset_path,
            include_asset,
        )));

        let handle = thread::spawn(move || loop {
            let done = routes.lock().unwrap().is_empty();
            if done {
                break;
            }

            let (mut stream, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let mut buf = [0_u8; 4096];
            let size = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..size]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap()
                .to_string();
            let body = routes
                .lock()
                .unwrap()
                .remove(&path)
                .unwrap_or_else(http_not_found);
            stream.write_all(&body).unwrap();
        });

        Self {
            base_url,
            _handle: handle,
        }
    }
}

fn build_routes(
    base_url: &str,
    asset_name: &str,
    asset_path: &str,
    include_asset: bool,
) -> HashMap<String, Vec<u8>> {
    let mut routes = HashMap::new();
    let release_json = serde_json::json!({
        "tag_name": "v0.2.0",
        "assets": [{
            "name": asset_name,
            "browser_download_url": format!("{base_url}{asset_path}")
        }]
    });
    routes.insert(
        "/repos/example/acs/releases/latest".to_string(),
        http_ok(release_json.to_string().into_bytes(), "application/json"),
    );

    if include_asset {
        routes.insert(
            asset_path.to_string(),
            http_ok(
                archive_with_binary(b"updated-acs-binary"),
                "application/gzip",
            ),
        );
    }

    routes
}

fn archive_with_binary(contents: &[u8]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_mode(0o755);
    header.set_size(contents.len() as u64);
    header.set_cksum();
    builder.append_data(&mut header, "acs", contents).unwrap();
    let encoder = builder.into_inner().unwrap();
    encoder.finish().unwrap()
}

fn http_ok(body: Vec<u8>, content_type: &str) -> Vec<u8> {
    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\n\r\n",
        body.len(),
        content_type
    );
    [headers.into_bytes(), body].concat()
}

fn http_not_found() -> Vec<u8> {
    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
}
