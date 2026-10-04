//! Explicit owned Go-peer diagnostics; the unchanged full parity gate owns acceptance.
#![cfg(windows)]

use std::{fs, io::Write, path::Path, process::Command, time::Instant};

use serde_json::json;
use symbrain_broker::{Client, Options};

const SAFE_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "USERPROFILE",
    "SYSTEMROOT",
    "WINDIR",
    "TMP",
    "TEMP",
    "TMPDIR",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "APPDATA",
    "LOCALAPPDATA",
    "LANG",
    "LC_ALL",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

fn event(path: &Path, phase: &str, started: Instant, fields: &serde_json::Value) {
    let mut stream = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(
        stream,
        "{}",
        json!({
            "phase": phase, "elapsed_ns": started.elapsed().as_nanos().to_string(),
            "fields": fields,
        })
    )
    .unwrap();
    stream.sync_all().unwrap();
}

fn child_probe() {
    let binary = std::env::var("SYMBRAIN_HEALTH_GO_DIAGNOSTIC_BINARY").unwrap();
    let profile = std::env::var("OWNED_HEALTH_PROFILE").unwrap();
    let mode = std::env::var("OWNED_HEALTH_MODE").unwrap();
    let log = std::env::var("OWNED_HEALTH_PHASES").unwrap();
    let log = Path::new(&log);
    let started = Instant::now();
    let env: Vec<_> = std::env::vars()
        .filter(|(name, _)| {
            if mode == "safe" {
                SAFE_ENV
                    .iter()
                    .any(|allowed| name.eq_ignore_ascii_case(allowed))
            } else {
                !name.starts_with("OWNED_HEALTH_") && name != "SYMBRAIN_HEALTH_GO_DIAGNOSTIC_BINARY"
            }
        })
        .collect();
    event(
        log,
        "spawn_begin",
        started,
        &json!({
            "binary": binary, "args": ["mcp", "--profile-file", profile],
            "cwd": std::env::current_dir().unwrap(), "mode": mode,
            "environment_names": env.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        }),
    );
    let spawned = Client::spawn(
        &binary,
        Options {
            args: vec!["mcp".into(), "--profile-file".into(), profile],
            env: Some(env),
            capture_stderr: true,
        },
    );
    let client = match spawned {
        Ok(client) => client,
        Err(error) => {
            event(
                log,
                "spawn_error",
                started,
                &json!({"error": format!("{error:?}")}),
            );
            panic!("owned Go peer spawn failed: {error:?}");
        }
    };
    event(log, "spawn_end", started, &json!({"pid": client.pid()}));
    probe_lifecycle(client, log, started);
}

fn probe_lifecycle(client: Client, log: &Path, started: Instant) {
    // The production health budget begins after successful spawn and spans init + ping.
    let probe_started = Instant::now();
    let deadline = probe_started + std::time::Duration::from_secs(5);
    event(
        log,
        "initialize_begin",
        started,
        &json!({"budget_ms": 5000}),
    );
    let initialized = deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .map(|remaining| client.initialize(remaining));
    event(
        log,
        "initialize_end",
        started,
        &json!({
            "result": format!("{initialized:?}"), "stderr": client.stderr_bytes(),
            "exited": format!("{:?}", client.exited()),
        }),
    );
    let ping = if initialized.as_ref().is_some_and(Result::is_ok) {
        let before_event = deadline.saturating_duration_since(Instant::now());
        event(
            log,
            "ping_begin",
            started,
            &json!({"remaining_ns_before_event": before_event.as_nanos().to_string()}),
        );
        // Durable logging consumes the same budget as both peer calls.
        deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .map(|remaining| client.ping(remaining))
    } else {
        None
    };
    event(
        log,
        "ping_end",
        started,
        &json!({"result": format!("{ping:?}")}),
    );
    let closed = client.close();
    let killed = client.kill();
    event(
        log,
        "cleanup_end",
        started,
        &json!({
            "close": format!("{closed:?}"), "kill": format!("{killed:?}"),
            "stderr": client.stderr_bytes(), "exited": format!("{:?}", client.exited()),
        }),
    );
    drop(client);
    event(log, "drop_end", started, &json!({}));
    assert!(
        initialized.as_ref().is_some_and(Result::is_ok),
        "owned Go init failed or deadline exhausted: {initialized:?}"
    );
    assert!(
        matches!(ping, Some(Ok(()))),
        "owned Go ping failed: {ping:?}"
    );
}

fn command_for_peer(mode: &str, root: &Path, output: &Path, binary: &std::ffi::OsStr) -> Command {
    let profile = root.join("probe.toml");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "owned_go_health_initialize_phases",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .current_dir(root.join("project"))
        .env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("home"))
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("TZ", "UTC")
        .env("PROJECT", root.join("project"))
        .env("SYMBRAIN_GO_BINARY", binary)
        .env("SYMBRAIN_HEALTH_GO_DIAGNOSTIC_BINARY", binary)
        .env("OWNED_HEALTH_MODE", mode)
        .env("OWNED_HEALTH_PROFILE", profile)
        .env(
            "OWNED_HEALTH_PHASES",
            output.join(format!("peer-{mode}.jsonl")),
        );
    for (name, directory) in [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_STATE_HOME", "state"),
    ] {
        command.env(name, root.join(directory));
    }
    for name in [
        "SystemRoot",
        "windir",
        "ComSpec",
        "PATHEXT",
        "TMPDIR",
        "TMP",
        "TEMP",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

#[test]
#[ignore = "requires an explicitly allocated actual Go binary and durable private output directory"]
fn owned_go_health_initialize_phases() {
    if std::env::var_os("OWNED_HEALTH_MODE").is_some() {
        child_probe();
        return;
    }
    let binary = std::env::var_os("SYMBRAIN_HEALTH_GO_DIAGNOSTIC_BINARY").unwrap();
    let output = std::env::var_os("SYMBRAIN_HEALTH_DIAGNOSTIC_OUTPUT").unwrap();
    let output = Path::new(&output);
    assert!(
        output.is_absolute(),
        "diagnostic output must survive child cwd changes"
    );
    fs::create_dir(output).unwrap();
    let mut failures = Vec::new();
    for mode in ["fixture", "safe"] {
        let root = tempfile::tempdir().unwrap();
        for name in ["home", "config", "cache", "data", "state", "project"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        let profile = root.path().join("probe.toml");
        fs::write(&profile, "[profile]\nname = \"probe\"\n").unwrap();
        let mut command = command_for_peer(mode, root.path(), output, &binary);
        // Keep the original parent's ten-second child budget, including cleanup.
        let mut child = command.spawn().unwrap();
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if started.elapsed() >= std::time::Duration::from_secs(10) {
                // This private test child can own a Go descendant; reap the complete tree.
                let taskkill = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                    .join("System32/taskkill.exe");
                let tree = Command::new(taskkill)
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .status();
                let _ = child.kill();
                let _ = child.wait();
                event(
                    &output.join(format!("peer-{mode}.jsonl")),
                    "parent_timeout",
                    started,
                    &json!({"taskkill": format!("{tree:?}")}),
                );
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        if !status.is_some_and(|status| status.success()) {
            failures.push((mode, status));
        }
    }
    assert!(
        failures.is_empty(),
        "owned Go peer diagnostic failures: {failures:?}"
    );
}
