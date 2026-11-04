use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    process::Command,
};

use sha2::{Digest, Sha256};

#[test]
fn native_upgrade_check_reads_fixture_cache_without_applying_or_networking() {
    let cache_root = tempfile::tempdir().expect("isolated XDG cache");
    let mut hash = Sha256::new();
    hash.update(b"danieljustus\0symaira-browse");
    let digest = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let cache = cache_root
        .path()
        .join("symaira")
        .join("updatecheck")
        .join(format!("{digest}.json"));
    fs::create_dir_all(cache.parent().expect("cache parent")).expect("create isolated cache");
    fs::write(&cache, include_str!("fixtures/update-cache.json"))
        .expect("seed Go-compatible local cache fixture");
    let original_cache = fs::read(&cache).expect("read fixture cache");
    let proxy_listener = TcpListener::bind("127.0.0.1:0").expect("reserve local proxy port");
    let blocked_proxy = format!("http://{}", proxy_listener.local_addr().unwrap());
    drop(proxy_listener);

    let output = Command::new(env!("CARGO_BIN_EXE_symbrowse"))
        .args(["upgrade", "--check", "--json"])
        .env("XDG_CACHE_HOME", cache_root.path())
        .env("HTTPS_PROXY", &blocked_proxy)
        .env("https_proxy", &blocked_proxy)
        .env("ALL_PROXY", &blocked_proxy)
        .env("all_proxy", &blocked_proxy)
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .output()
        .expect("run native CLI executable");

    assert!(
        output.status.success(),
        "upgrade --check failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("JSON envelope");
    assert_eq!(envelope["success"], true);
    assert_eq!(envelope["data"]["up_to_date"], false);
    assert_eq!(envelope["data"]["current"], "0.0.0");
    assert_eq!(envelope["data"]["latest"], "v0.0.1");
    assert_eq!(
        envelope["data"]["hint"],
        "symbrowse v0.0.1 is available (current: 0.0.0); run `symbrowse upgrade` to apply it"
    );
    assert_eq!(
        fs::read(&cache).expect("check leaves cache intact"),
        original_cache
    );
}

#[test]
fn native_upgrade_check_failure_exits_nonzero_for_json_and_yaml() {
    let cache_root = tempfile::tempdir().expect("isolated XDG cache");
    let proxy_listener = TcpListener::bind("127.0.0.1:0").expect("reserve local proxy port");
    let blocked_proxy = format!("http://{}", proxy_listener.local_addr().unwrap());
    drop(proxy_listener);

    for format in ["json", "yaml"] {
        let output = Command::new(env!("CARGO_BIN_EXE_symbrowse"))
            .args(["upgrade", "--check", "--output", format])
            .env("XDG_CACHE_HOME", cache_root.path())
            .env("HTTPS_PROXY", &blocked_proxy)
            .env("https_proxy", &blocked_proxy)
            .env("ALL_PROXY", &blocked_proxy)
            .env("all_proxy", &blocked_proxy)
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .output()
            .expect("run native CLI executable");

        assert_eq!(
            output.status.code(),
            Some(1),
            "{format} must report failure"
        );
        assert!(output.stderr.is_empty(), "{format} uses structured stdout");
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 envelope");
        if format == "json" {
            let envelope: serde_json::Value = serde_json::from_str(&stdout).expect("JSON envelope");
            assert_eq!(envelope["success"], false);
            assert_eq!(envelope["error"]["code"], "operation_failed");
        } else {
            assert!(stdout.contains("success: false\n"), "{stdout}");
            assert!(stdout.contains("code: operation_failed\n"), "{stdout}");
        }
    }
}

/// Serves `routes` (path -> body) over plain HTTP on loopback until dropped.
fn serve(routes: Vec<(String, Vec<u8>)>) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        listener.set_nonblocking(false).unwrap();
        for stream in listener.incoming().take(2) {
            let mut stream = stream.unwrap();
            let mut buffer = [0u8; 4096];
            let read = stream.read(&mut buffer).unwrap();
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
            let (status, body) = routes
                .iter()
                .find(|(route, _)| *route == path)
                .map_or(("404 Not Found", Vec::new()), |(_, body)| {
                    ("200 OK", body.clone())
                });
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    (base, handle)
}

fn run_apply(corrupt_checksum: bool) -> (std::process::Output, Vec<u8>, tempfile::TempDir) {
    let (os, arch) = (
        match std::env::consts::OS {
            "macos" => "darwin",
            other => other,
        },
        match std::env::consts::ARCH {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            other => other,
        },
    );
    let asset_name = format!("symbrowse_{os}_{arch}");
    let payload = b"#!/bin/sh\necho upgraded\n".to_vec();
    let digest = if corrupt_checksum {
        "0".repeat(64)
    } else {
        Sha256::digest(&payload)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    let checksums = format!("{digest}  {asset_name}\n").into_bytes();
    let (base, server) = serve(vec![
        (format!("/{asset_name}"), payload.clone()),
        ("/checksums.txt".to_owned(), checksums),
    ]);

    let root = tempfile::tempdir().unwrap();
    let install = root.path().join("bin");
    fs::create_dir(&install).unwrap();
    let binary = install.join("symbrowse");
    fs::copy(env!("CARGO_BIN_EXE_symbrowse"), &binary).unwrap();
    let before = fs::read(&binary).unwrap();

    let cache_root = root.path().join("cache");
    let mut hash = Sha256::new();
    hash.update(b"danieljustus\0symaira-browse");
    let digest = hash
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let cache = cache_root
        .join("symaira")
        .join("updatecheck")
        .join(format!("{digest}.json"));
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(
        &cache,
        serde_json::json!({
            "timestamp": "2099-01-01T00:00:00Z",
            "release": {
                "TagName": "v0.0.1", "Body": "fixture",
                "HTMLURL": "https://github.com/danieljustus/symaira-browse/releases/tag/v0.0.1",
                "Assets": [
                    {"Name": asset_name, "BrowserDownloadURL": format!("{base}/{asset_name}"), "Size": 0},
                    {"Name": "checksums.txt", "BrowserDownloadURL": format!("{base}/checksums.txt"), "Size": 0}
                ]
            }
        })
        .to_string(),
    )
    .unwrap();

    let output = Command::new(&binary)
        .args(["upgrade", "--json"])
        .env_clear()
        .env("HOME", root.path())
        .env("XDG_CACHE_HOME", &cache_root)
        .output()
        .expect("run copied CLI");
    drop(server); // the server thread exits once both routes were requested or the process ends
    (output, before, root)
}

#[test]
fn upgrade_applies_verified_asset_atomically() {
    let (output, _, root) = run_apply(false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        envelope,
        serde_json::json!({"success": true, "data": {
            "applied": true, "latest": "v0.0.1",
            "hint": "restart symbrowse to use the new version"
        }})
    );
    let installed = root.path().join("bin").join("symbrowse");
    assert_eq!(fs::read(&installed).unwrap(), b"#!/bin/sh\necho upgraded\n");
    assert!(
        !root.path().join("bin").join("symbrowse.bak").exists(),
        "backup removed"
    );
}

#[test]
fn upgrade_checksum_mismatch_leaves_binary_untouched() {
    let (output, before, root) = run_apply(true);
    assert_eq!(output.status.code(), Some(1));
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["success"], false);
    let message = envelope["error"]["message"].as_str().unwrap();
    assert!(
        message.starts_with("update failed (rollback performed): updateapply: checksum mismatch"),
        "{message}"
    );
    assert_eq!(
        fs::read(root.path().join("bin").join("symbrowse")).unwrap(),
        before
    );
}
