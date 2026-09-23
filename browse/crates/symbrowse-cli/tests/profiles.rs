use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime},
};

// Snapshot content, type, mode and modification time, never access time.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (bool, u32, SystemTime, Vec<u8>)> {
    fn visit(
        root: &Path,
        path: &Path,
        out: &mut BTreeMap<PathBuf, (bool, u32, SystemTime, Vec<u8>)>,
    ) {
        let meta = fs::symlink_metadata(path).unwrap();
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            meta.permissions().mode()
        };
        #[cfg(not(unix))]
        let mode = u32::from(meta.permissions().readonly());
        let bytes = if meta.is_file() {
            fs::read(path).unwrap()
        } else {
            Vec::new()
        };
        out.insert(
            path.strip_prefix(root).unwrap().to_owned(),
            (meta.is_dir(), mode, meta.modified().unwrap(), bytes),
        );
        if meta.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), out);
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

fn chrome_root(root: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        root.join("home")
            .join("Library")
            .join("Application Support")
            .join("Google")
            .join("Chrome")
    } else if cfg!(windows) {
        root.join("local-app-data")
            .join("Google")
            .join("Chrome")
            .join("User Data")
    } else {
        root.join("home").join(".config").join("google-chrome")
    }
}

fn run(
    binary: &Path,
    root: &Path,
    logs: &Path,
    label: &str,
    args: &[&str],
    empty_home: bool,
) -> (i32, Vec<u8>, Vec<u8>) {
    let stdout = logs.join(format!("{label}.stdout"));
    let stderr = logs.join(format!("{label}.stderr"));
    let mut cmd = Command::new(binary);
    cmd.args(args)
        .current_dir(root)
        .env_clear()
        .env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("user-profile"))
        .env("LOCALAPPDATA", root.join("local-app-data"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("TMPDIR", root.join("tmp"))
        .env("TMP", root.join("tmp"))
        .env("TEMP", root.join("tmp"))
        .env("PATH", root.join("empty-path"))
        .env("LANG", "C")
        .env("TZ", "UTC")
        .stdin(Stdio::null())
        .stdout(fs::File::create(&stdout).unwrap())
        .stderr(fs::File::create(&stderr).unwrap());
    if empty_home {
        cmd.env("HOME", "").env("USERPROFILE", "");
    }
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        cmd.env("SystemRoot", system_root);
    }
    let mut child = cmd.spawn().expect("launch isolated profile CLI");
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(15) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "profile CLI exceeded 15 seconds; raw streams retained at {}",
                logs.display()
            );
        }
        thread::sleep(Duration::from_millis(10));
    };
    (
        status.code().expect("CLI must not die by signal"),
        fs::read(stdout).unwrap(),
        fs::read(stderr).unwrap(),
    )
}

#[test]
fn browser_profiles_match_go_cli_without_touching_browser_state() {
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let capture = std::env::temp_dir().join(format!(
        "symbrowse-profiles-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&capture).unwrap();
    println!("profiles raw evidence: {}", capture.display());
    let rust = Path::new(env!("CARGO_BIN_EXE_symbrowse"));
    let go = std::env::var_os("SYMBROWSE_GO_ORACLE").map(PathBuf::from);
    if let Some(go) = &go {
        assert!(
            fs::read(go).unwrap() != fs::read(rust).unwrap(),
            "Go oracle must not be identical to the Rust CLI"
        );
    }
    for layout in ["missing", "empty", "populated", "root-file", "empty-home"] {
        let scenario = capture.join(layout);
        let root = scenario.join("fixture");
        let logs = scenario.join("streams");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir(&logs).unwrap();
        for dir in [
            "home",
            "user-profile",
            "local-app-data",
            "xdg-config",
            "xdg-data",
            "xdg-cache",
            "xdg-state",
            "tmp",
            "empty-path",
        ] {
            fs::create_dir(root.join(dir)).unwrap();
        }
        // XDG is intentionally not HOME/.config; Go browser discovery ignores it.
        let decoy = root
            .join("xdg-config")
            .join("google-chrome")
            .join("Profile 9");
        fs::create_dir_all(&decoy).unwrap();
        fs::write(decoy.join("Preferences"), b"{}\n").unwrap();
        let browser = chrome_root(&root);
        if layout != "missing" {
            fs::create_dir_all(browser.parent().unwrap()).unwrap();
            if layout == "root-file" {
                fs::write(&browser, b"not a directory\n").unwrap();
            } else {
                fs::create_dir(&browser).unwrap();
            }
        }
        let names = if matches!(layout, "populated" | "empty-home") {
            for name in [
                "Profile 2",
                "Default",
                "Profile 10",
                "Profile 1",
                "Profile 11",
                "Profile 01",
                "Cache",
                "GPUCache",
            ] {
                let dir = browser.join(name);
                fs::create_dir(&dir).unwrap();
                fs::write(dir.join("Preferences"), b"{\"synthetic\":true}\n").unwrap();
            }
            fs::create_dir(browser.join("Profile 5")).unwrap(); // No marker.
            fs::write(browser.join("Profile 3"), b"not a directory\n").unwrap();
            fs::create_dir_all(browser.join("Profile 4").join("Preferences")).unwrap(); // Go uses stat, not is_file.
            if layout == "populated" {
                vec![
                    "Default",
                    "Profile 1",
                    "Profile 10",
                    "Profile 2",
                    "Profile 4",
                ]
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        let profiles = if names.is_empty() {
            Value::Null
        } else {
            json!(names.iter().map(|name| json!({"name":name,"path":browser.join(name),"browser":"chrome","is_default":*name=="Default"})).collect::<Vec<_>>())
        };
        let expected = json!({"success":true,"data":{"profiles":profiles}});
        let before = snapshot(&root);
        let cases: &[(&str, &[&str], bool)] = &[
            ("json-after", &["profiles", "--json"], true),
            ("json-before", &["--json", "profiles"], true),
            ("output-before", &["--output=json", "profiles"], true),
            ("output-after", &["profiles", "--output", "json"], true),
            (
                "json-precedence",
                &["profiles", "--json", "--output=invalid"],
                true,
            ),
            (
                "json-false",
                &["profiles", "--json=false", "--output=json"],
                true,
            ),
            ("text", &["profiles"], false),
            ("yaml", &["profiles", "--output", "yaml"], false),
        ];
        for (case, args, structured) in cases {
            let reference = go.as_ref().map(|go| {
                run(
                    go,
                    &root,
                    &logs,
                    &format!("{case}-go"),
                    args,
                    layout == "empty-home",
                )
            });
            assert_eq!(
                snapshot(&root),
                before,
                "Go changed fixture: {layout}/{case}"
            );
            let actual = run(
                rust,
                &root,
                &logs,
                &format!("{case}-rust"),
                args,
                layout == "empty-home",
            );
            assert_eq!(
                snapshot(&root),
                before,
                "Rust changed fixture: {layout}/{case}"
            );
            assert_eq!(actual.0, 0, "{layout}/{case}: {actual:?}");
            assert!(actual.2.is_empty(), "{layout}/{case}: {actual:?}");
            if *structured {
                assert_eq!(
                    serde_json::from_slice::<Value>(&actual.1).unwrap(),
                    expected,
                    "{layout}/{case}"
                );
            } else if *case == "text" {
                let expected_text = if names.is_empty() {
                    "no Chrome profiles found\n".to_owned()
                } else {
                    names
                        .iter()
                        .map(|name| {
                            format!(
                                "{name}\t{}{}\n",
                                browser.join(name).display(),
                                if *name == "Default" { " (default)" } else { "" }
                            )
                        })
                        .collect::<String>()
                };
                assert_eq!(actual.1, expected_text.as_bytes());
            } else {
                assert!(actual.1.starts_with(b"success: true\n"));
            }
            if let Some(reference) = reference {
                assert_eq!((actual.0, &actual.2), (reference.0, &reference.2));
                if *structured {
                    assert_eq!(
                        serde_json::from_slice::<Value>(&actual.1).unwrap(),
                        serde_json::from_slice::<Value>(&reference.1).unwrap()
                    );
                } else {
                    assert_eq!(actual.1, reference.1, "{layout}/{case}");
                }
                println!("Go/Rust profiles case={layout}/{case} passed");
            }
        }
        if layout == "missing" {
            for (case, args, code, structured, message) in [
                (
                    "reject-list-json",
                    vec!["profiles", "list", "--json"],
                    2,
                    true,
                    "unknown command \"list\" for \"symbrowse profiles\"",
                ),
                (
                    "reject-list-text",
                    vec!["profiles", "list"],
                    2,
                    false,
                    "unknown command \"list\" for \"symbrowse profiles\"",
                ),
                (
                    "terminator",
                    vec!["profiles", "--", "list", "--json"],
                    2,
                    false,
                    "unknown command \"list\" for \"symbrowse profiles\"",
                ),
                (
                    "unknown-after-json",
                    vec!["profiles", "--json", "--unknown"],
                    1,
                    true,
                    "unknown flag: --unknown",
                ),
                (
                    "unknown-before-json",
                    vec!["profiles", "--unknown", "--json"],
                    1,
                    false,
                    "unknown flag: --unknown",
                ),
                (
                    "invalid-output",
                    vec!["profiles", "--output", "invalid"],
                    2,
                    false,
                    "invalid --output format \"invalid\": want text, json or yaml",
                ),
                (
                    "missing-output",
                    vec!["profiles", "--json", "--output"],
                    1,
                    true,
                    "flag needs an argument: --output",
                ),
            ] {
                let reference = go
                    .as_ref()
                    .map(|go| run(go, &root, &logs, &format!("{case}-go"), &args, false));
                let actual = run(rust, &root, &logs, &format!("{case}-rust"), &args, false);
                assert_eq!(actual.0, code, "{case}: {actual:?}");
                if structured {
                    let json: Value = serde_json::from_slice(&actual.1).unwrap();
                    assert_eq!(
                        json,
                        json!({"success":false,"error":{"code":if code==2 {"invalid_args"} else {"internal"},"message":message}})
                    );
                    assert!(actual.2.is_empty());
                    if let Some(reference) = &reference {
                        assert_eq!(json, serde_json::from_slice::<Value>(&reference.1).unwrap());
                    }
                } else {
                    assert!(actual.1.is_empty());
                    assert_eq!(actual.2, format!("{message}\n").as_bytes());
                }
                if let Some(reference) = reference {
                    assert_eq!((actual.0, actual.2), (reference.0, reference.2));
                    println!("Go/Rust profiles case={case} passed");
                }
                assert_eq!(snapshot(&root), before, "argument errors changed fixture");
            }
            let args = ["mcp", "--list-profiles", "--json"];
            let actual = run(rust, &root, &logs, "mcp-rust", &args, false);
            assert_eq!(actual.0, 0);
            assert!(actual.1.starts_with(b"core     14 tools") && actual.2.is_empty());
            if let Some(go) = &go {
                assert_eq!(actual, run(go, &root, &logs, "mcp-go", &args, false));
                println!("Go/Rust profiles case=separate-mcp-catalog passed");
            }
            assert_eq!(snapshot(&root), before, "catalog changed fixture");
        }
    }
    // Retain these synthetic raw streams and fixture snapshots for gate review.
}
