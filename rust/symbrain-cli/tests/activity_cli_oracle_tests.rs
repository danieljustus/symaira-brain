#[path = "../../test-support/coverage.rs"]
mod coverage;

use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;

#[derive(Deserialize)]
struct Oracle {
    id: String,
    args: Vec<String>,
    exit_code: i32,
    stdout: String,
    stderr: String,
}

#[derive(Deserialize)]
struct Suite {
    cases: Vec<Oracle>,
}

fn fixture() -> Suite {
    #[cfg(windows)]
    let path = PathBuf::from(
        std::env::var_os("SYMBRAIN_ACTIVITY_CLI_ORACLE_FIXTURE")
            .expect("Windows CI must provide the freshly generated Go oracle fixture"),
    );
    #[cfg(not(windows))]
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/activity_cli_oracle.json");
    serde_json::from_slice(&fs::read(path).expect("read Go activity CLI oracle"))
        .expect("parse Go activity CLI oracle")
}

#[test]
fn bounded_activity_reads_match_the_pinned_go_commands() {
    for oracle in fixture().cases {
        run_case(&oracle);
    }
}

fn run_case(oracle: &Oracle) {
    let root = tempdir().expect("temporary isolated root");
    let root = root
        .path()
        .canonicalize()
        .unwrap_or_else(|_| root.path().to_path_buf());
    let home = root.join("home");
    let config = home.join(".config");
    let profile_dir = config.join("symbrain/profiles");
    let data = root.join("data");
    fs::create_dir_all(&profile_dir).expect("create isolated profile directory");
    fs::create_dir_all(&data).expect("create isolated database directory");
    fs::write(
        profile_dir.join("activity-oracle.toml"),
        "[profile]\nname = \"activity-oracle\"\n\n[servers.memory]\nenabled = true\nmode = \"read_only\"\ntools_allow = [\"activity_search\", \"activity_get\", \"activity_status\"]\n",
    )
    .expect("write isolated profile");

    let db_path = data.join(format!("{}.db", oracle.id));
    if oracle.id.starts_with("search-") || oracle.id.starts_with("get-") {
        let store =
            symbrain_memory::Store::open(&db_path).expect("initialize isolated memory schema");
        drop(store);
        let connection =
            rusqlite::Connection::open(&db_path).expect("open seeded activity database");
        connection
            .execute(
                "INSERT INTO activity_segments (id,source,granularity,started_at,ended_at,applications,redacted_summary,raw_ref,prior_segment_ids,superseded_by,expires_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![
                    "cli-segment",
                    "symcockpit",
                    "10min",
                    "2026-08-28T12:00:00Z",
                    "2026-08-28T12:10:00Z",
                    "[\"Editor\"]",
                    "edited activity summary",
                    "opaque://oracle-fixture",
                    "[]",
                    "",
                    "2027-08-28T12:10:00Z",
                ],
            )
            .expect("seed synthetic activity segment");
    }

    let args = oracle
        .args
        .iter()
        .map(|arg| arg.replace("<root>", &root.to_string_lossy()))
        .collect::<Vec<_>>();
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .envs(coverage::profile_environment())
        .args(args)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_DATA_HOME", &data)
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_STATE_HOME", root.join("state"));
    #[cfg(windows)]
    for key in ["SystemRoot", "windir", "ComSpec", "PATHEXT"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let output = command.output().expect("run native activity CLI case");
    assert_eq!(
        output.status.code(),
        Some(oracle.exit_code),
        "{}",
        oracle.id
    );
    assert_eq!(output.stdout, oracle.stdout.as_bytes(), "{}", oracle.id);
    assert_eq!(output.stderr, oracle.stderr.as_bytes(), "{}", oracle.id);
}
