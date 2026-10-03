//! Real child-process vault contracts from the immutable production Go CLI.
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct Fixture {
    go_oracle_ref: String,
    child_sha256: String,
    total: usize,
    results: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    case: Scenario,
    go: Expected,
}
#[derive(Deserialize)]
struct Scenario {
    name: String,
    args: Vec<String>,
    input_hex: String,
    metadata: Option<String>,
    discovery: Option<String>,
    action_exit: Option<i32>,
    get_exit: Option<i32>,
    #[serde(default)]
    passthrough: bool,
    #[serde(default)]
    extra_env: std::collections::BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct Expected {
    exit_code: i32,
    stdout: String,
    stderr: String,
    calls: String,
}

fn unhex(hex: &str) -> Vec<u8> {
    let (pairs, remainder) = hex.as_bytes().as_chunks::<2>();
    assert!(remainder.is_empty());
    pairs
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn vault_administration_matches_real_go_process_contracts() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/vault_admin_go.json")).unwrap();
    assert_eq!(
        fixture.go_oracle_ref,
        "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
    );
    assert_eq!(fixture.total, fixture.results.len());
    assert!(fixture.total > 80, "empty/incomplete corpus");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/vault_child.rs");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&source).unwrap())),
        fixture.child_sha256,
        "child fixture changed without actual Go regeneration"
    );
    let temp = tempfile::tempdir().unwrap();
    let child = temp
        .path()
        .join(format!("fake-vault{}", std::env::consts::EXE_SUFFIX));
    let output = Command::new("rustc")
        .args([
            OsString::from("--edition=2024"),
            source.into_os_string(),
            OsString::from("-o"),
            child.as_os_str().to_owned(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for (index, case) in fixture.results.iter().enumerate() {
        run_case(&child, case, &temp.path().join(format!("case-{index}")));
    }
}

fn case_command(child: &Path, case: &Case, root: &Path) -> Command {
    fs::create_dir_all(root.join("home")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("PATH", root.join("empty-bin"))
        .env("FAKE_VAULT_LOG", root.join("calls"))
        .env(
            "FAKE_VAULT_METADATA",
            case.case.metadata.as_deref().unwrap_or("{}"),
        )
        .env(
            "FAKE_VAULT_ACTION_EXIT",
            case.case.action_exit.unwrap_or(0).to_string(),
        )
        .env(
            "FAKE_VAULT_GET_EXIT",
            case.case.get_exit.unwrap_or(0).to_string(),
        );
    for key in ["SystemRoot", "WINDIR", "COMSPEC", "PATHEXT"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command.envs(&case.case.extra_env);
    if case.case.passthrough {
        command.env("FAKE_VAULT_PASSTHROUGH", "1");
    }
    match case.case.discovery.as_deref().unwrap_or("env") {
        "env" => {
            command.env("SYMBRAIN_SERVERS_VAULT_BINARY_PATH", child);
        }
        "config" => {
            fs::create_dir_all(root.join("config/symbrain")).unwrap();
            fs::write(
                root.join("config/symbrain/config.toml"),
                format!(
                    "[servers.vault]\nbinary_path = {}\n",
                    serde_json::to_string(&child.to_string_lossy()).unwrap()
                ),
            )
            .unwrap();
        }
        "managed" | "path" => {
            let directory = root.join(if case.case.discovery.as_deref() == Some("managed") {
                "home/.symaira/bin"
            } else {
                "bin"
            });
            fs::create_dir_all(&directory).unwrap();
            fs::copy(
                child,
                directory.join(format!("symvault{}", std::env::consts::EXE_SUFFIX)),
            )
            .unwrap();
            if case.case.discovery.as_deref() == Some("path") {
                command.env("PATH", directory);
            }
        }
        other => panic!("nonportable discovery case {other}"),
    }
    command
}

fn run_case(child: &Path, case: &Case, root: &Path) {
    let mut command = case_command(child, case, root);
    // File captures cannot fill a pipe while the parent enforces its deadline.
    let mut stdout = tempfile::tempfile_in(root).unwrap();
    let mut stderr = tempfile::tempfile_in(root).unwrap();
    command
        .arg("vault")
        .args(&case.case.args)
        .stdin(Stdio::piped())
        .stdout(stdout.try_clone().unwrap())
        .stderr(stderr.try_clone().unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut process = command.spawn().unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(&unhex(&case.case.input_hex))
        .unwrap_or_else(|error| {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        });
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = process.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            #[cfg(unix)]
            {
                let _ = Command::new("/bin/kill")
                    .args(["-KILL", "--", &format!("-{}", process.id())])
                    .status();
            }
            let _ = process.kill();
            let _ = process.wait();
            panic!("vault command hung: {}", case.case.name);
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    stdout.rewind().unwrap();
    stderr.rewind().unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    stdout.read_to_end(&mut out).unwrap();
    stderr.read_to_end(&mut err).unwrap();
    assert_eq!(status.code(), Some(case.go.exit_code), "{}", case.case.name);
    assert_eq!(out, case.go.stdout.as_bytes(), "{} stdout", case.case.name);
    assert_eq!(err, case.go.stderr.as_bytes(), "{} stderr", case.case.name);
    let calls = fs::read_to_string(root.join("calls")).unwrap_or_default();
    assert_eq!(calls, case.go.calls, "{} argv/stdin", case.case.name);
}

#[test]
fn vault_validation_stays_native_with_a_fallback_executor() {
    struct NoFallback;
    impl symbrain_cli::FallbackExecutor for NoFallback {
        fn execute(&self, _: &[OsString], _: &mut dyn Write) -> u8 {
            panic!("vault reached Go fallback");
        }
    }
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(
        symbrain_cli::run_with_executor(
            &[OsString::from("vault"), OsString::from("create")],
            &mut out,
            &mut err,
            &NoFallback
        ),
        2
    );
    assert!(out.is_empty());
}
