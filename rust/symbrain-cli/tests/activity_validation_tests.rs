//! Complete process comparisons for native activity flags and validation.
#[path = "../../test-support/coverage.rs"]
mod coverage;
use serde::Deserialize;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;

#[derive(Deserialize)]
#[serde(untagged)]
enum Arg {
    Text(String),
    Raw { hex: String },
}
#[derive(Deserialize)]
struct Case {
    name: String,
    args: Vec<Arg>,
    exit: i32,
    stdout_hex: String,
    stderr_hex: String,
}
#[derive(Deserialize)]
struct Fixture {
    go_oracle_ref: String,
    cases: Vec<Case>,
}
fn decode(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0, "odd fixture hex");
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).expect("fixture byte")
        })
        .collect()
}
#[cfg(unix)]
fn raw_arg(hex: &str) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(decode(hex))
}
#[cfg(not(unix))]
fn raw_arg(_hex: &str) -> OsString {
    unreachable!("Unix-only raw argument")
}

#[test]
fn activity_validation_matches_complete_go_process_fixture_without_fallback() {
    let path = std::env::var_os("SYMBRAIN_ACTIVITY_VALIDATION_FIXTURE").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/activity_validation_767.json")
        },
        PathBuf::from,
    );
    let fixture: Fixture =
        serde_json::from_slice(&fs::read(path).expect("required activity validation fixture"))
            .expect("Go process fixture");
    assert_eq!(
        fixture.go_oracle_ref,
        "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
    );
    let mut count = 0;
    let mut names = std::collections::BTreeSet::new();
    for case in fixture.cases {
        if cfg!(windows) && case.args.iter().any(|a| matches!(a, Arg::Raw { .. })) {
            continue;
        }
        assert!(names.insert(case.name.clone()), "duplicate activity case");
        let root = tempdir().expect("isolated activity root");
        let home = root.path().join("home");
        let config = home.join(".config");
        let profiles = config.join("symbrain/profiles");
        fs::create_dir_all(&profiles).expect("profile root");
        fs::write(profiles.join("activity-oracle.toml"),"[profile]\nname=\"activity-oracle\"\n[servers.memory]\nenabled=true\nmode=\"read_only\"\ntools_allow=[\"activity_search\",\"activity_get\",\"activity_status\"]\n").expect("isolated granting profile");
        let args = case.args.into_iter().map(|a| match a {
            Arg::Text(text) => OsString::from(text),
            Arg::Raw { hex } => raw_arg(&hex),
        });
        let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
        command
            .env_clear()
            .envs(coverage::profile_environment())
            .args(args)
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", config)
            .env("XDG_DATA_HOME", root.path().join("data"))
            .env("XDG_CACHE_HOME", root.path().join("cache"))
            .env("XDG_STATE_HOME", root.path().join("state"))
            .env("SYMBRAIN_GO_BINARY", root.path().join("absent-go-fallback"));
        #[cfg(windows)]
        for key in ["SystemRoot", "windir", "ComSpec", "PATHEXT"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let output = command.output().expect("native activity process");
        assert_eq!(output.status.code(), Some(case.exit), "{} exit", case.name);
        assert_eq!(
            output.stdout,
            decode(&case.stdout_hex),
            "{} stdout",
            case.name
        );
        assert_eq!(
            output.stderr,
            decode(&case.stderr_hex),
            "{} stderr",
            case.name
        );
        count += 1;
    }
    assert_eq!(
        count,
        if cfg!(windows) { 173 } else { 185 },
        "complete activity process case count"
    );
}
