use super::{run_get, run_search, run_status};
use std::ffi::OsString;
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use tempfile::tempdir;

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn invalid_get_and_status_budgets_match_usage_before_opening_database() {
    let usage = [
        (
            "get",
            "usage: symbrain activity get <id> --profile <name> --max-tokens <N> [--db <path>]\n",
        ),
        (
            "status",
            "usage: symbrain activity status --profile <name> --max-tokens <N> [--db <path>]\n",
        ),
    ];
    for budget in [None, Some("--max-tokens=0"), Some("--max-tokens=4001")] {
        for (command, expected) in usage {
            let mut argv = vec!["--profile=default", "--db=/no/such/database"];
            if let Some(value) = budget {
                argv.push(value);
            }
            if command == "get" {
                argv.push("item-id");
            }
            let argv = args(&argv);
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let code = if command == "get" {
                run_get(&argv, &mut stdout, &mut stderr, OutputFormat::Table)
            } else {
                run_status(&argv, &mut stdout, &mut stderr, OutputFormat::Table)
            };
            assert_eq!(code, exit::USAGE);
            assert!(stdout.is_empty());
            assert_eq!(stderr, expected.as_bytes());
        }
    }
}

#[test]
fn db_override_is_used_by_each_activity_read_command() {
    let directory = tempdir().expect("temporary directory");
    let db_path = directory.path().join("invalid.db");
    std::fs::write(&db_path, b"not a sqlite database").expect("invalid database fixture");
    let db_path = db_path.to_string_lossy().into_owned();

    for command in ["search", "get", "status"] {
        let mut argv = vec![OsString::from("--profile=default")];
        if command == "get" {
            argv.push(OsString::from(format!("--db={db_path}")));
        } else {
            argv.extend([OsString::from("--db"), OsString::from(&db_path)]);
        }
        argv.push(OsString::from("--max-tokens=100"));
        match command {
            "search" => argv.extend([
                OsString::from("--from=2026-09-01T00:00:00Z"),
                OsString::from("--to=2026-09-02T00:00:00Z"),
                OsString::from("--limit=1"),
                OsString::from("query"),
            ]),
            "get" => argv.push(OsString::from("item-id")),
            "status" => {}
            _ => unreachable!(),
        }

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = match command {
            "search" => run_search(&argv, &mut stdout, &mut stderr, OutputFormat::Table),
            "get" => run_get(&argv, &mut stdout, &mut stderr, OutputFormat::Table),
            "status" => run_status(&argv, &mut stdout, &mut stderr, OutputFormat::Table),
            _ => unreachable!(),
        };
        assert_eq!(code, exit::GENERIC, "{command}");
        assert!(stdout.is_empty(), "{command}");
        assert!(
            String::from_utf8(stderr)
                .expect("UTF-8 error")
                .starts_with(&format!("symbrain activity {command}: open database:")),
            "{command}"
        );
    }
}
