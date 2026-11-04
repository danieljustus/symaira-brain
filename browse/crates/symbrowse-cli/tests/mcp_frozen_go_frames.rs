//! Replays the 13 frozen Go `symbrowse mcp` stdout fixtures (MCP-001..008)
//! against the production Rust MCP server proxying to the production Rust
//! daemon, without Go. Provenance: `testdata/port/mcp/frozen-go-provenance.json`.
#![cfg(unix)]

use std::fs;
use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use symbrowse_mcp::{ServeOptions, serve_stdio};

const SESSION: &str = "default";
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/port/mcp");

struct Daemon {
    child: Child,
    root: PathBuf,
    endpoint: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn start_daemon() -> Daemon {
    // Short root: macOS AF_UNIX paths are limited to 104 bytes.
    let root = std::env::temp_dir().join(format!("mf{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let home = root.join("h");
    let bin = root.join("b");
    for dir in [&home, &bin, &root.join("r")] {
        fs::create_dir_all(dir).unwrap();
    }
    // The Go oracle treated a missing `security` tool as a failure, not as an
    // absent key; the recorder supplied one that exits 44. Mirror it.
    let security = bin.join("security");
    fs::write(&security, "#!/bin/sh\nexit 44\n").unwrap();
    fs::set_permissions(&security, fs::Permissions::from_mode(0o700)).unwrap();
    let endpoint = if cfg!(target_os = "macos") {
        home.join("Library/Caches/symbrowse/run")
            .join(format!("{SESSION}.sock"))
    } else {
        root.join("r/symbrowse").join(format!("{SESSION}.sock"))
    };
    let child = Command::new(env!("CARGO_BIN_EXE_symbrowse"))
        .args(["daemon", "--session", SESSION, "--ssrf"])
        .current_dir(&root)
        .env_clear()
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", root.join("c"))
        .env("XDG_CACHE_HOME", root.join("k"))
        .env("XDG_DATA_HOME", root.join("d"))
        .env("XDG_STATE_HOME", root.join("s"))
        .env("XDG_RUNTIME_DIR", root.join("r"))
        .env("PATH", &bin)
        .env("SYMBROWSE_NO_AUTOSTART", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let daemon = Daemon {
        child,
        root,
        endpoint,
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    while UnixStream::connect(&daemon.endpoint).is_err() {
        assert!(
            Instant::now() < deadline,
            "daemon did not listen on {}",
            daemon.endpoint.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    daemon
}

fn replay(endpoint: &Path, name: &str, profiles: &str) {
    let input = fs::read(Path::new(FIXTURES).join(format!("{name}.in"))).unwrap();
    let expected = fs::read(Path::new(FIXTURES).join(format!("{name}.out")))
        .unwrap_or_else(|error| panic!("frozen Go output {name}.out is required: {error}"));
    let options = ServeOptions {
        // The Go fixture binary was built with version v0.8.0.
        version: "v0.8.0".to_owned(),
        session: SESSION.to_owned(),
        profiles: profiles.to_owned(),
        executable: String::new(),
        allow_private: false,
        engine: None,
        daemon_log_path: None,
        endpoint: Some(endpoint.to_string_lossy().into_owned()),
    };
    let mut actual = Vec::new();
    serve_stdio(Cursor::new(input), &mut actual, options).expect("serve MCP fixture");
    assert_eq!(
        String::from_utf8_lossy(&actual),
        String::from_utf8_lossy(&expected),
        "fixture={name}"
    );
}

#[test]
fn rust_mcp_and_daemon_match_all_frozen_go_frames() {
    let daemon = start_daemon();
    let cases = [
        ("initialize", "core"),
        ("framed_initialize", "core"),
        ("initialized", "core"),
        ("tools_core", "core"),
        ("tools_nav", "nav"),
        ("tools_all", "all"),
        ("malformed", "core"),
        ("unknown_method", "core"),
        ("unknown_tool", "core"),
        ("missing_argument", "core"),
        ("tool_error", "core"),
        ("notifications", "core"),
        ("eof", "core"),
    ];
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(Path::new(FIXTURES).join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["fixtures"].as_object().unwrap().len(), cases.len());
    for (name, profiles) in cases {
        replay(&daemon.endpoint, name, profiles);
    }
}
