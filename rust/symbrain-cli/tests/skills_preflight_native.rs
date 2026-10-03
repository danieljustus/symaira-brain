//! Native preflight regressions; every child has a bounded deadline.

#[cfg(unix)]
use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::process::Command;

use tempfile::TempDir;

#[path = "support/skills_preflight_process.rs"]
mod process;
use process::run;
#[cfg(unix)]
use process::run_os;

#[test]
fn native_skills_preflight_contracts() {
    check_sync_flag_errors();
    check_sync_boolean_flags();
    check_rejected_markers_override_fallback();
    check_marker_probe_does_not_create_directories();
    #[cfg(unix)]
    check_special_markers();
    #[cfg(unix)]
    check_raw_argument_values();
    #[cfg(unix)]
    check_raw_target_whitespace();
}

fn check_sync_flag_errors() {
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
            &["--scope", "--bad"],
            "symbrain skills sync: unknown scope \"-bad\"",
        ),
        (
            &["--target", "--bad"],
            "symbrain skills sync: unknown target \"-bad\"",
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
}

fn check_sync_boolean_flags() {
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

fn check_marker_probe_does_not_create_directories() {
    use symbrain_skills::install::{MarkerState, read_marker};
    let root = TempDir::new().unwrap();
    let missing = root.path().join("missing/parent/skill");
    assert!(matches!(
        read_marker(&missing).unwrap(),
        MarkerState::Missing
    ));
    assert!(!root.path().join("missing").exists());
    let original = root.path().join("enumerated/skill");
    fs::create_dir_all(&original).unwrap();
    let entry = fs::read_dir(original.parent().unwrap())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let moved = root.path().join("moved");
    fs::rename(&original, &moved).unwrap();
    assert!(matches!(read_marker(&entry).unwrap(), MarkerState::Missing));
    assert!(
        !entry.exists(),
        "marker preflight recreated a removed directory"
    );
    assert!(moved.is_dir());
}

#[cfg(unix)]
fn check_raw_argument_values() {
    use std::os::unix::ffi::OsStringExt;
    for (bytes, separated, expected) in [
        (
            b"--scope=bad\xff".as_slice(),
            None,
            "unknown scope \"bad\\xff\"",
        ),
        (
            b"--scope".as_slice(),
            Some(b"bad\xff".as_slice()),
            "unknown scope \"bad\\xff\"",
        ),
        (
            b"--target=bad\xff".as_slice(),
            None,
            "unknown target \"bad\\xff\"",
        ),
        (
            b"--target".as_slice(),
            Some(b"bad\xff".as_slice()),
            "unknown target \"bad\\xff\"",
        ),
        (
            b"--dry-run=bad\xff".as_slice(),
            None,
            "invalid boolean value \"bad\\xff\"",
        ),
        (
            b"--scope= bad\xff ".as_slice(),
            None,
            "unknown scope \" bad\\xff \"",
        ),
        (
            b"--scope".as_slice(),
            Some(b" bad\xff ".as_slice()),
            "unknown scope \" bad\\xff \"",
        ),
        (
            b"--target= \xe2\x80\x8bbad\xff\xe2\x80\x8b ".as_slice(),
            None,
            "unknown target \"\\u200bbad\\xff\\u200b\"",
        ),
        (
            b"--target".as_slice(),
            Some(b" \xe2\x80\x8bbad\xff\xe2\x80\x8b ".as_slice()),
            "unknown target \"\\u200bbad\\xff\\u200b\"",
        ),
        (
            b"--target= \xa0bad\xff\xc2 ".as_slice(),
            None,
            "unknown target \"\\xa0bad\\xff\\xc2\"",
        ),
    ] {
        let root = TempDir::new().unwrap();
        let mut args = vec![
            OsString::from("skills"),
            OsString::from("sync"),
            OsString::from_vec(bytes.to_vec()),
        ];
        if let Some(value) = separated {
            args.push(OsString::from_vec(value.to_vec()));
        }
        let output = run_os(&root, &args, false);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{:?}",
            output.stderr
        );
    }
}

#[cfg(unix)]
fn check_raw_target_whitespace() {
    use std::os::unix::ffi::OsStringExt;
    // All Go Unicode White_Space runes, with invalid bytes kept at the edge
    // after trimming. Also cover non-whitespace Unicode boundary characters.
    for padding in [
        " ",
        "\t\n\u{000b}\u{000c}\r",
        "\u{0085}",
        "\u{00a0}",
        "\u{1680}",
        "\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}",
        "\u{2028}\u{2029}",
        "\u{202f}",
        "\u{205f}",
        "\u{3000}",
    ] {
        let value = [padding.as_bytes(), b"bad\xff", padding.as_bytes()].concat();
        for inline in [false, true] {
            let root = TempDir::new().unwrap();
            let mut args = vec![OsString::from("skills"), OsString::from("sync")];
            if inline {
                args.push(OsString::from_vec(
                    [b"--target=".as_slice(), &value].concat(),
                ));
            } else {
                args.extend([
                    OsString::from("--target"),
                    OsString::from_vec(value.clone()),
                ]);
            }
            let output = run_os(&root, &args, false);
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
            assert_eq!(
                output.stderr,
                b"symbrain skills sync: unknown target \"bad\\xff\" (known: claude, opencode, codex, antigravity, hermes, openclaw)\n",
                "{args:?}"
            );
            assert!(!root.path().join("data/symbrain/skills").exists());
        }
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
