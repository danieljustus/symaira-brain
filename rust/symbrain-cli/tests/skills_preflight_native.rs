//! Native preflight regressions; every child has a bounded deadline.

use std::fs;
use std::io::{Read, Seek};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

fn run(root: &TempDir, args: &[&str], configured: bool) -> Output {
    let home = root.path().join("home");
    let config = root.path().join("config");
    let data = root.path().join("data");
    let project = root.path().join("project");
    for path in [&home, &config, &data, &project] {
        fs::create_dir_all(path).unwrap();
    }
    let stdout = tempfile::tempfile_in(root.path()).unwrap();
    let stderr = tempfile::tempfile_in(root.path()).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_DATA_HOME", &data)
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("TZ", "UTC")
        .current_dir(project)
        .stdout(Stdio::from(stdout.try_clone().unwrap()))
        .stderr(Stdio::from(stderr.try_clone().unwrap()))
        .args(args);
    if configured {
        command.env(
            "SYMBRAIN_SKILLS_LIBRARY_DIR",
            root.path().join("other-library"),
        );
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("native preflight hung for {args:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut stdout = stdout;
    let mut stderr = stderr;
    stdout.rewind().unwrap();
    stderr.rewind().unwrap();
    let mut output = Output {
        status,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    stdout.read_to_end(&mut output.stdout).unwrap();
    stderr.read_to_end(&mut output.stderr).unwrap();
    output
}

#[test]
fn native_skills_preflight_contracts() {
    check_sync_flags();
    check_rejected_markers_override_fallback();
    #[cfg(unix)]
    check_special_markers();
}

fn check_sync_flags() {
    let errors: &[(&[&str], &str)] = &[
        (
            &["--target"],
            "flag needs an argument: -target\nUsage of skills sync:",
        ),
        (
            &["-target"],
            "flag needs an argument: -target\nUsage of skills sync:",
        ),
        (
            &["--scope"],
            "flag needs an argument: -scope\nUsage of skills sync:",
        ),
        (
            &["-scope"],
            "flag needs an argument: -scope\nUsage of skills sync:",
        ),
        (
            &["--unknown"],
            "flag provided but not defined: -unknown\nUsage of skills sync:",
        ),
        (
            &["--dry-run=invalid"],
            "invalid boolean value \"invalid\" for -dry-run: parse error\nUsage of skills sync:",
        ),
        (&["--help"], "Usage of skills sync:"),
        (
            &["--target=not-a-target"],
            "symbrain skills sync: unknown target \"not-a-target\"",
        ),
        (
            &["--scope=not-a-scope"],
            "symbrain skills sync: unknown scope \"not-a-scope\"",
        ),
        (
            &["--scope=bad\x1b"],
            "symbrain skills sync: unknown scope \"bad\\x1b\"",
        ),
        (
            &["--dry-run=bad\x1b"],
            "invalid boolean value \"bad\\x1b\" for -dry-run: parse error\nUsage of skills sync:",
        ),
    ];
    for (flags, prefix) in errors {
        let root = TempDir::new().unwrap();
        let mut args = vec!["skills", "sync"];
        args.extend_from_slice(flags);
        let output = run(&root, &args, false);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).starts_with(prefix),
            "{args:?}: {:?}",
            output.stderr
        );
        assert!(
            !root.path().join("data/symbrain/skills").exists(),
            "parse failure wrote skill data"
        );
    }
    for (flags, expected) in [
        (vec!["--dry-run"], true),
        (vec!["--dry-run=true"], true),
        (vec!["--dry-run=1"], true),
        (vec!["--dry-run=false"], false),
        (vec!["--dry-run=0"], false),
        (vec!["--", "--target"], false),
        (vec!["---", "--target"], false),
        (vec!["positional", "--scope"], false),
        (vec!["--dry-run=true", "--dry-run=false"], false),
    ] {
        let root = TempDir::new().unwrap();
        let mut args = vec!["skills", "sync", "--json"];
        args.extend(flags);
        let output = run(&root, &args, false);
        assert!(output.status.success(), "{args:?}: {:?}", output.stderr);
        assert!(output.stderr.is_empty());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["dry_run"], expected, "{args:?}");
        assert_eq!(report["results"], serde_json::json!([]));
    }
}

fn check_rejected_markers_override_fallback() {
    for configured in [false, true] {
        let root = TempDir::new().unwrap();
        let skills = root.path().join("home/.config/opencode/skills");
        let unsafe_dir = skills.join("z-unsafe");
        fs::create_dir_all(&unsafe_dir).unwrap();
        fs::File::create(unsafe_dir.join(".symskills.json"))
            .unwrap()
            .set_len(symbrain_skills::MAX_INPUT_SIZE + 1)
            .unwrap();
        // A legacy-only malformed marker must not hide a later unsafe entry.
        let malformed = skills.join("a-malformed");
        fs::create_dir_all(&malformed).unwrap();
        fs::write(malformed.join(".symskills.json"), b"{not json").unwrap();
        let output = run(
            &root,
            &["skills", "status", "--target", "opencode", "--json"],
            configured,
        );
        assert!(output.status.success(), "{:?}", output.stderr);
        assert!(output.stderr.is_empty());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(report["installs"].as_array().unwrap().iter().any(|row| {
            row["name"] == "z-unsafe"
                && row["error"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("exceeds maximum input size")
        }));
        assert!(matches!(
            symbrain_skills::install::read_marker(&unsafe_dir).unwrap(),
            symbrain_skills::install::MarkerState::Rejected(_)
        ));
    }
}

#[cfg(unix)]
fn check_special_markers() {
    for kind in ["fifo", "symlink"] {
        let root = TempDir::new().unwrap();
        let installed = root.path().join("home/.config/opencode/skills/unsafe");
        fs::create_dir_all(&installed).unwrap();
        let marker = installed.join(".symskills.json");
        if kind == "fifo" {
            assert!(
                Command::new("mkfifo")
                    .arg(&marker)
                    .status()
                    .unwrap()
                    .success()
            );
        } else {
            let outside = root.path().join("outside-marker");
            fs::write(&outside, b"{not json").unwrap();
            std::os::unix::fs::symlink(&outside, &marker).unwrap();
        }
        let output = run(
            &root,
            &["skills", "status", "--target", "opencode", "--json"],
            false,
        );
        assert!(output.status.success(), "{kind}: {:?}", output.stderr);
        assert!(output.stderr.is_empty());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let error = report["installs"][0]["error"].as_str().unwrap();
        assert!(
            error.contains(if kind == "fifo" {
                "must be a regular file"
            } else {
                "marker is a symlink"
            }),
            "{kind}: {error}"
        );
    }
}
