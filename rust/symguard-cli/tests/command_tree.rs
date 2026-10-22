//! Replay the Go-derived `symguard` command-tree inventory (#770) against the native binary.
//!
//! The fixture `migration/evidence/guard-command-tree-770/inventory.json` is generated from the
//! pinned Go oracle by `scripts/guard-standalone-oracle/command_tree.py --write-fixture`. This test
//! needs no Go toolchain: it executes every recorded probe in a disposable HOME/XDG root and
//! compares exit code, stdout and stderr with the recorded Go bytes.
use serde_json::Value;
use std::fs;
use std::io::{Read, Seek};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Output lines that legitimately name the toolchain, platform or runtime SDK of one binary.
const RUNTIME_LINES: [&str; 8] = [
    "go ",
    "rust ",
    "Go:",
    "Rust:",
    "os/arch",
    "OS/Arch:",
    "toolchain ",
    "Toolchain:",
];

struct Observed {
    code: i32,
    stdout: String,
    stderr: String,
}

fn fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../migration/evidence/guard-command-tree-770/inventory.json");
    serde_json::from_slice(&fs::read(path).expect("inventory fixture")).expect("fixture JSON")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string").to_owned())
        .collect()
}

fn run(args: &[String]) -> Observed {
    let root = TempDir::new().unwrap();
    let home = root.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let stdout = tempfile::tempfile_in(root.path()).unwrap();
    let stderr = tempfile::tempfile_in(root.path()).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_symguard"));
    cmd.env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_DATA_HOME", root.path().join("data"))
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("PATH", "")
        .current_dir(root.path())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.try_clone().unwrap()))
        .stderr(Stdio::from(stderr.try_clone().unwrap()));
    for key in ["SystemRoot", "WINDIR", "TMP", "TEMP"] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    let mut child = cmd.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "symguard {args:?} timed out");
        std::thread::sleep(Duration::from_millis(5));
    };
    let (mut out, mut err) = (stdout, stderr);
    out.rewind().unwrap();
    err.rewind().unwrap();
    let (mut out_text, mut err_text) = (Vec::new(), Vec::new());
    out.read_to_end(&mut out_text).unwrap();
    err.read_to_end(&mut err_text).unwrap();
    let root_text = root.path().to_string_lossy().into_owned();
    let clean = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).replace(&root_text, "<root>");
    Observed {
        code: status.code().expect("exit code"),
        stdout: clean(out_text),
        stderr: clean(err_text),
    }
}

fn without_runtime_lines(text: &str) -> String {
    text.split_inclusive('\n')
        .filter(|line| {
            let trimmed = line.trim_start();
            !RUNTIME_LINES
                .iter()
                .any(|prefix| trimmed.starts_with(prefix))
        })
        .collect()
}

/// `Err` names the first difference between the recorded Go probe and the observed Rust process.
fn compare(probe: &Value, observed: &Observed) -> Result<(), String> {
    let args = strings(&probe["args"]);
    if probe["exit_code"].as_i64() != Some(i64::from(observed.code)) {
        return Err(format!("{args:?}: exit code {}", observed.code));
    }
    if probe["root_dependent"].as_bool() == Some(true) {
        return Ok(());
    }
    let (stdout, stderr) = (
        probe["stdout"].as_str().unwrap(),
        probe["stderr"].as_str().unwrap(),
    );
    if without_runtime_lines(stdout) != without_runtime_lines(&observed.stdout) {
        return Err(format!("{args:?}: stdout differs"));
    }
    if without_runtime_lines(stderr) != without_runtime_lines(&observed.stderr) {
        return Err(format!("{args:?}: stderr differs"));
    }
    Ok(())
}

#[test]
fn every_go_probe_matches_the_native_process() {
    let inventory = fixture();
    let probes = inventory["probes"].as_array().unwrap();
    assert!(
        probes.len() >= 80,
        "inventory shrank to {} probes",
        probes.len()
    );
    let failures: Vec<String> = probes
        .iter()
        .filter_map(|probe| compare(probe, &run(&strings(&probe["args"]))).err())
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn every_inventory_path_is_exercised_and_none_is_unported() {
    let inventory = fixture();
    let probes = inventory["probes"].as_array().unwrap();
    for node in inventory["tree"].as_array().unwrap() {
        let path = strings(&node["path"]);
        let exercised = probes.iter().any(|probe| {
            let args = strings(&probe["args"]);
            let owner = strings(&probe["owner"]);
            owner == path && (args.starts_with(&path) || path.is_empty())
        });
        assert!(exercised, "tree path {path:?} has no probe");
        let own_help = node["help_text"].as_str();
        if let Some(help) = own_help {
            // The recorded help text must also be printed by the native binary.
            let mut args = path.clone();
            args.push("--help".to_owned());
            let observed = run(&args);
            assert_eq!(observed.stdout, help, "help text of {path:?}");
        }
    }
    let rendered = serde_json::to_string(&inventory["probes"]).unwrap();
    for marker in [
        "not yet ported",
        "not ported",
        "unsupported native diagnostic state",
    ] {
        assert!(
            !rendered.contains(marker),
            "Go oracle never prints {marker:?}"
        );
    }
    assert_eq!(inventory["update_check_invoked_by_any_command"], false);
}

#[test]
fn comparison_rejects_altered_expectations() {
    let inventory = fixture();
    let probes = inventory["probes"].as_array().unwrap();
    let scan_help = probes
        .iter()
        .find(|probe| strings(&probe["args"]) == ["scan", "--help"])
        .unwrap();
    let observed = run(&strings(&scan_help["args"]));
    assert!(compare(scan_help, &observed).is_ok());

    let mut altered = scan_help.clone();
    altered["stdout"] = Value::String(observed.stdout.replacen("Discovers", "Finds", 1));
    assert!(compare(&altered, &observed).is_err(), "help drift accepted");

    let mut wrong_exit = scan_help.clone();
    wrong_exit["exit_code"] = Value::from(2);
    assert!(
        compare(&wrong_exit, &observed).is_err(),
        "exit drift accepted"
    );

    // A silent success where the oracle fails closed must be rejected.
    let unknown = probes
        .iter()
        .find(|probe| probe["exit_code"] == 2 && strings(&probe["args"]).len() == 1)
        .unwrap();
    let silent = Observed {
        code: 0,
        stdout: String::new(),
        stderr: String::new(),
    };
    assert!(
        compare(unknown, &silent).is_err(),
        "silent success accepted"
    );
}
