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

/// Go `doctor/command.go` row for an empty discovery result (the recorded Unix capture).
const UNIX_MCP_ROW: &str = "  mcp servers      none discovered\n";
/// Go `doctor/command.go` closing line when `problems == 0` (the recorded Unix capture).
const UNIX_PASS_LINE: &str =
    "All basic checks passed. Run 'symguard scan' after setup for full diagnostics.\n";
/// Go `doctor/command.go` closing line for `problems == 1` (`fmt.Fprintf("%d issue(s) found...")`).
const WINDOWS_ISSUE_LINE: &str = "1 issue(s) found. See details above.\n";
/// Stands for the one row whose OS wording is free; see [`error_row_matches`].
const ERROR_ROW_PLACEHOLDER: &str = "  mcp servers      error: <discovery error>\n";

/// Derives the stdout Go's `doctor` prints on Windows for the replay's empty HOME from the
/// recorded Unix stdout, by exactly the two rewrites its source dictates: the `mcp servers` row
/// becomes the `error:` row (`report("mcp servers", "error: ...", true)`) and, because that counts
/// one problem, the closing line becomes `1 issue(s) found. See details above.`.
/// The error row is returned as [`ERROR_ROW_PLACEHOLDER`] because its OS text is host-specific.
fn windows_expected_stdout(recorded_unix: &str) -> Result<String, String> {
    if recorded_unix.matches(UNIX_MCP_ROW).count() != 1 {
        return Err("recorded stdout lacks exactly one `mcp servers none discovered` row".into());
    }
    if recorded_unix.matches(UNIX_PASS_LINE).count() != 1
        || !recorded_unix.ends_with(UNIX_PASS_LINE)
    {
        return Err(
            "recorded stdout does not end with the single `All basic checks passed` line".into(),
        );
    }
    let rewritten = recorded_unix.replacen(UNIX_MCP_ROW, ERROR_ROW_PLACEHOLDER, 1);
    let body = rewritten.strip_suffix(UNIX_PASS_LINE).unwrap_or(&rewritten);
    Ok(format!("{body}{WINDOWS_ISSUE_LINE}"))
}

/// Hand-written equivalent of the regex
/// `^  mcp servers      error: discovery: hermes \(<root>[\\/]home[\\/]\.config[\\/]hermes[\\/]config\.json\): \[unsupported\] read failed: open .+: .+$`
/// applied to one line without its newline. Everything after `open ` (the path and the Windows
/// `ERROR_PATH_NOT_FOUND` text) is free wording, as documented at `guard_scan::io_detail`.
fn error_row_matches(line: &str) -> bool {
    let Some(mut rest) = line.strip_prefix("  mcp servers      error: discovery: hermes (<root>")
    else {
        return false;
    };
    for part in ["home", ".config", "hermes", "config.json"] {
        let Some(after) = rest
            .strip_prefix(['\\', '/'])
            .and_then(|after| after.strip_prefix(part))
        else {
            return false;
        };
        rest = after;
    }
    let Some(tail) = rest.strip_prefix("): [unsupported] read failed: open ") else {
        return false;
    };
    !tail.contains(['\n', '\r'])
        && tail
            .match_indices(": ")
            .any(|(at, _)| at >= 1 && at + 2 < tail.len())
}

/// Masked byte comparison of a Windows `doctor` run: exit 1, empty stderr, and stdout equal to the
/// Go-derived [`windows_expected_stdout`] (both sides through [`without_runtime_lines`]) except the
/// error row, which must satisfy [`error_row_matches`]. Pure, so it is unit-tested on every host.
fn compare_windows_doctor(recorded_unix: &str, observed: &Observed) -> Result<(), String> {
    if observed.code != 1 {
        return Err(format!("exit code {} (Go exits 1)", observed.code));
    }
    if !observed.stderr.is_empty() {
        return Err("unexpected stderr".into());
    }
    let expected = without_runtime_lines(&windows_expected_stdout(recorded_unix)?);
    let actual = without_runtime_lines(&observed.stdout);
    let (expected, actual): (Vec<&str>, Vec<&str>) = (
        expected.split_inclusive('\n').collect(),
        actual.split_inclusive('\n').collect(),
    );
    if expected.len() != actual.len() {
        return Err(format!(
            "stdout has {} lines, expected {}",
            actual.len(),
            expected.len()
        ));
    }
    for (want, got) in expected.iter().zip(&actual) {
        let same = if *want == ERROR_ROW_PLACEHOLDER {
            got.strip_suffix('\n').is_some_and(error_row_matches)
        } else {
            want == got
        };
        if !same {
            return Err(format!("stdout line {got:?} differs from {want:?}"));
        }
    }
    Ok(())
}

/// Host-dependent probes (Go `doctor`) are recorded on Unix. Off Unix Go itself reports a missing
/// client-config parent directory as `ERROR_PATH_NOT_FOUND`, which its string-based discovery check
/// does not skip, so the empty-HOME probe exits 1 with an `mcp servers error` row. That output is
/// derived from the Go source (`doctor/command.go`, `discovery.containsNotExist`) and is NOT covered
/// by a recorded Windows capture of this replay (the Windows CI leg of the `missing_client_parents`
/// doctor-oracle case records Go's real bytes for the shared handler, pending that run). Off Unix
/// the recorded Unix bytes are therefore rewritten by exactly the two source-derived edits and
/// compared byte-for-byte, see [`compare_windows_doctor`].
fn compare_host_invariants(probe: &Value, observed: &Observed) -> Result<(), String> {
    let args = strings(&probe["args"]);
    if probe["exit_code"].as_i64() != Some(0) {
        return Err(format!("{args:?}: recorded Unix capture is not exit 0"));
    }
    compare_windows_doctor(probe["stdout"].as_str().unwrap(), observed)
        .map_err(|error| format!("{args:?}: {error}"))
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
    let recorded: Vec<&Value> = probes
        .iter()
        .filter(|probe| host_dependent(probe))
        .collect();
    assert_eq!(
        recorded.len(),
        7,
        "generator-recorded host-dependent probes"
    );
    // Only the doctor paths may skip the byte-exact comparison, and only off Unix.
    assert!(
        recorded
            .iter()
            .all(|probe| strings(&probe["owner"]) == ["doctor"]),
        "only doctor probes may be host-dependent"
    );
    let failures: Vec<String> = probes
        .iter()
        .filter_map(|probe| compare(probe, &run(&strings(&probe["args"]))).err())
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
    let relaxed = if cfg!(unix) { 0 } else { recorded.len() };
    let exact = probes.len() - relaxed;
    let (want_exact, want_relaxed) = if cfg!(unix) { (80, 0) } else { (73, 7) };
    assert_eq!(relaxed, want_relaxed, "relaxed probe count");
    assert_eq!(exact, want_exact, "byte-exact probe count");
}

/// The error row Go prints on Windows for the replay's empty HOME (OS text from the Windows SDK).
const WINDOWS_ERROR_ROW: &str = "  mcp servers      error: discovery: hermes (<root>\\home\\.config\\hermes\\config.json): [unsupported] read failed: open <root>\\home\\.config\\hermes\\config.json: The system cannot find the path specified.\n";

/// A Windows `doctor` stdout built independently of [`windows_expected_stdout`] from Go's source.
fn simulated_windows_stdout(recorded_unix: &str) -> String {
    recorded_unix
        .replace(UNIX_MCP_ROW, WINDOWS_ERROR_ROW)
        .replace(UNIX_PASS_LINE, WINDOWS_ISSUE_LINE)
        .replace("Toolchain: <actual>", "Toolchain: rustc 9.9.9")
        .replace("OS/Arch:   <os-arch>", "OS/Arch:   windows/amd64")
}

fn observed(code: i32, stdout: impl AsRef<str>, stderr: &str) -> Observed {
    Observed {
        code,
        stdout: stdout.as_ref().to_owned(),
        stderr: stderr.to_owned(),
    }
}

/// Deviations from the Go-derived Windows `doctor` output; each must be rejected off Unix.
fn negative_controls(windows: &str) -> Vec<(&'static str, Observed)> {
    vec![
        ("exit 0", observed(0, windows, "")),
        ("exit 2", observed(2, windows, "")),
        ("extra stderr", observed(1, windows, "boom\n")),
        ("empty stdout", observed(1, "", "")),
        (
            "changed config row",
            observed(
                1,
                windows.replace(
                    "config           not configured (no config file found)",
                    "config           ok",
                ),
                "",
            ),
        ),
        (
            "missing issue line",
            observed(1, windows.replace(WINDOWS_ISSUE_LINE, ""), ""),
        ),
        (
            "pass line kept",
            observed(1, windows.replace(WINDOWS_ISSUE_LINE, UNIX_PASS_LINE), ""),
        ),
        (
            "wrong issue count",
            observed(
                1,
                windows.replace(WINDOWS_ISSUE_LINE, "2 issue(s) found. See details above.\n"),
                "",
            ),
        ),
        (
            "wrong client name",
            observed(
                1,
                windows.replace("discovery: hermes (", "discovery: cursor ("),
                "",
            ),
        ),
        (
            "unrecognised path",
            observed(1, windows.replace("config.json)", "mcp.json)"), ""),
        ),
        (
            "no error row",
            observed(1, windows.replace(WINDOWS_ERROR_ROW, UNIX_MCP_ROW), ""),
        ),
        (
            "empty OS wording",
            observed(
                1,
                windows.replace(
                    "config.json: The system cannot find the path specified.",
                    "config.json: ",
                ),
                "",
            ),
        ),
        (
            "extra row",
            observed(
                1,
                windows.replace(
                    WINDOWS_ERROR_ROW,
                    &format!("{WINDOWS_ERROR_ROW}  extra           x\n"),
                ),
                "",
            ),
        ),
    ]
}

#[test]
fn host_dependence_only_relaxes_doctor_probes_off_unix() {
    let inventory = fixture();
    let probes = inventory["probes"].as_array().unwrap();
    let doctor = probes
        .iter()
        .find(|probe| strings(&probe["args"]) == ["doctor"])
        .unwrap();
    let recorded = doctor["stdout"].as_str().unwrap();
    let windows = simulated_windows_stdout(recorded);
    assert_ne!(windows, recorded);
    let good = observed(1, &windows, "");

    // Every host-dependent probe accepts the Go-derived Windows output off Unix; Unix keeps the
    // byte-exact comparison and rejects it.
    for probe in probes.iter().filter(|probe| host_dependent(probe)) {
        let stdout = simulated_windows_stdout(probe["stdout"].as_str().unwrap());
        let windows_run = observed(1, &stdout, "");
        assert_eq!(compare_on(probe, &windows_run, false), Ok(()));
        assert!(compare_on(probe, &windows_run, true).is_err());
    }
    // Forward slashes and other OS wording in the free part are accepted.
    let slashes = windows.replace(
        WINDOWS_ERROR_ROW,
        "  mcp servers      error: discovery: hermes (<root>/home/.config/hermes/config.json): [unsupported] read failed: open <root>/home/.config/hermes/config.json: No such file or directory\n",
    );
    assert_eq!(
        compare_on(doctor, &observed(1, &slashes, ""), false),
        Ok(())
    );

    // Negative controls: each deviation fails off Unix.
    let broken = negative_controls(&windows);
    for (name, run) in &broken {
        assert!(
            compare_on(doctor, run, false).is_err(),
            "{name}: relaxed too far"
        );
    }

    // The rewrite is a pure function of the recorded bytes and refuses anything else.
    assert_eq!(
        windows_expected_stdout(recorded)
            .unwrap()
            .matches(ERROR_ROW_PLACEHOLDER)
            .count(),
        1
    );
    assert!(windows_expected_stdout(&recorded.replace(UNIX_MCP_ROW, "")).is_err());
    assert!(windows_expected_stdout(&recorded.replace(UNIX_PASS_LINE, "")).is_err());
    assert!(compare_windows_doctor("not a doctor report", &good).is_err());

    // A non-doctor probe is never relaxed: a drifting exit code fails on every host.
    let scan = probes
        .iter()
        .find(|probe| strings(&probe["args"]) == ["scan", "--help"])
        .unwrap();
    assert!(!host_dependent(scan));
    let drifted = observed(1, "", "");
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
