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

/// True when the generator recorded that this probe's Go bytes are only valid on a Unix host.
fn host_dependent(probe: &Value) -> bool {
    probe["host_dependent_reason"]
        .as_str()
        .is_some_and(|reason| !reason.is_empty())
}

/// Host-dependent probes (Go `doctor`) are recorded on Unix. Off Unix the Go report legitimately
/// differs (a missing client-config parent directory is `ERROR_PATH_NOT_FOUND`, which Go's
/// string-based discovery check reports, exit 1), so only the invariants common to every host are
/// checked here; the Windows doctor bytes are asserted against the host-recorded doctor oracle.
fn compare_host_invariants(probe: &Value, observed: &Observed) -> Result<(), String> {
    let args = strings(&probe["args"]);
    if !matches!(observed.code, 0 | 1) {
        return Err(format!("{args:?}: exit code {} outside 0/1", observed.code));
    }
    if !observed.stdout.starts_with("symguard doctor\n") {
        return Err(format!("{args:?}: not a doctor report"));
    }
    if !observed.stderr.is_empty() {
        return Err(format!("{args:?}: unexpected stderr"));
    }
    Ok(())
}

/// `Err` names the first difference between the recorded Go probe and the observed Rust process.
/// `unix` selects whether host-dependent probes are compared byte-for-byte (Unix capture host).
fn compare_on(probe: &Value, observed: &Observed, unix: bool) -> Result<(), String> {
    if !unix && host_dependent(probe) {
        return compare_host_invariants(probe, observed);
    }
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

fn compare(probe: &Value, observed: &Observed) -> Result<(), String> {
    compare_on(probe, observed, cfg!(unix))
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
    let relaxed = probes.iter().filter(|probe| host_dependent(probe)).count();
    let mut exact = 0;
    let failures: Vec<String> = probes
        .iter()
        .filter_map(|probe| {
            let observed = run(&strings(&probe["args"]));
            if !host_dependent(probe) || cfg!(unix) {
                exact += 1;
            }
            compare(probe, &observed).err()
        })
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
    // Only the generator-recorded host-dependent probes (the doctor paths) may skip the byte-exact
    // comparison, and only off Unix: everything else, and every probe on Unix, is compared exactly.
    assert!(relaxed >= 1, "generator recorded no host-dependent probe");
    assert!(
        probes
            .iter()
            .filter(|probe| host_dependent(probe))
            .all(|probe| strings(&probe["owner"]) == ["doctor"]),
        "only doctor probes may be host-dependent"
    );
    let expected = if cfg!(unix) {
        probes.len()
    } else {
        probes.len() - relaxed
    };
    assert_eq!(exact, expected, "byte-exact probe count");
    assert!(exact >= 73, "byte-exact probes shrank to {exact}");
}

#[test]
fn host_dependence_only_relaxes_doctor_probes_off_unix() {
    let inventory = fixture();
    let probes = inventory["probes"].as_array().unwrap();
    let windows_doctor = Observed {
        code: 1,
        stdout: "symguard doctor\n\n  mcp servers      error: discovery: read failed\n".to_owned(),
        stderr: String::new(),
    };
    let doctor = probes
        .iter()
        .find(|probe| strings(&probe["args"]) == ["doctor"])
        .unwrap();
    // The Unix capture exits 0; a Windows host exits 1. Unix keeps the exact comparison.
    assert!(compare_on(doctor, &windows_doctor, true).is_err());
    assert!(compare_on(doctor, &windows_doctor, false).is_ok());
    // The relaxation still rejects anything that is not a doctor report on any host.
    for broken in [
        Observed {
            code: 2,
            stdout: windows_doctor.stdout.clone(),
            stderr: String::new(),
        },
        Observed {
            code: 1,
            stdout: String::new(),
            stderr: String::new(),
        },
        Observed {
            code: 1,
            stdout: windows_doctor.stdout.clone(),
            stderr: "boom\n".to_owned(),
        },
    ] {
        assert!(
            compare_on(doctor, &broken, false).is_err(),
            "relaxed too far"
        );
    }
    // A non-doctor probe is never relaxed: a drifting exit code fails on every host.
    let scan = probes
        .iter()
        .find(|probe| strings(&probe["args"]) == ["scan", "--help"])
        .unwrap();
    assert!(!host_dependent(scan));
    let drifted = Observed {
        code: 1,
        stdout: String::new(),
        stderr: String::new(),
    };
    assert!(compare_on(scan, &drifted, false).is_err());
    assert!(compare_on(scan, &drifted, true).is_err());
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
