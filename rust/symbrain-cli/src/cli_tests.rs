use super::*;

fn execute(args: &[&str]) -> (u8, String, String) {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(&args, &mut stdout, &mut stderr);
    (
        code,
        String::from_utf8(stdout).expect("stdout is UTF-8"),
        String::from_utf8(stderr).expect("stderr is UTF-8"),
    )
}

#[test]
fn help_matches_frozen_usage() {
    let (code, stdout, stderr) = execute(&["help"]);
    assert_eq!(code, exit::OK);
    assert_eq!(stdout, USAGE);
    assert!(stderr.is_empty());
}

#[test]
fn missing_command_prints_help_and_exits_usage() {
    let (code, stdout, stderr) = execute(&[]);
    assert_eq!(code, exit::USAGE);
    assert_eq!(stdout, USAGE);
    assert!(stderr.is_empty());
}

#[test]
fn version_json_keeps_schema() {
    let (code, stdout, stderr) = execute(&["version", "--json"]);
    assert_eq!(code, exit::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stdout).expect("valid JSON"),
        serde_json::json!({"tool":"symbrain","version":"dev","schema_version":1})
    );
    assert!(stderr.is_empty());
}

#[test]
fn version_terminator_alone_succeeds() {
    let (code, stdout, stderr) = execute(&["version", "--"]);
    assert_eq!(code, exit::OK);
    assert!(stdout.starts_with("symbrain dev\n"));
    assert!(stderr.is_empty());
}

#[test]
fn version_terminator_with_json_succeeds() {
    let (code, stdout, stderr) = execute(&["version", "--", "--json"]);
    assert_eq!(code, exit::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stdout).expect("valid JSON"),
        serde_json::json!({"tool":"symbrain","version":"dev","schema_version":1})
    );
    assert!(stderr.is_empty());
}

#[test]
fn version_terminator_with_extra_arg_rejected() {
    let (code, stdout, stderr) = execute(&["version", "--", "extra"]);
    assert_eq!(code, exit::USAGE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, "symbrain version: unexpected argument \"extra\"\n");
}

#[test]
fn version_terminator_with_flag_arg_rejected_as_positional() {
    let (code, stdout, stderr) = execute(&["version", "--", "--extra"]);
    assert_eq!(code, exit::USAGE);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        "symbrain version: unexpected argument \"--extra\"\n"
    );
}

#[test]
fn version_bare_hyphen_treated_as_positional() {
    let (code, stdout, stderr) = execute(&["version", "-"]);
    assert_eq!(code, exit::USAGE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, "symbrain version: unexpected argument \"-\"\n");
}

#[test]
fn version_help_flags_print_usage_only() {
    for flag in ["-h", "-help", "--help"] {
        let (code, stdout, stderr) = execute(&["version", flag]);
        assert_eq!(code, exit::USAGE);
        assert!(stdout.is_empty());
        assert_eq!(stderr, "Usage of version:\n");
    }
}

#[test]
fn version_rejects_unknown_flag() {
    let (code, stdout, stderr) = execute(&["version", "--bogus"]);
    assert_eq!(code, exit::USAGE);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        "flag provided but not defined: -bogus\nUsage of version:\n"
    );
}

#[test]
fn version_rejects_unknown_flag_with_value() {
    let (code1, stdout1, stderr1) = execute(&["version", "--bogus=123"]);
    assert_eq!(code1, exit::USAGE);
    assert!(stdout1.is_empty());
    assert_eq!(
        stderr1,
        "flag provided but not defined: -bogus\nUsage of version:\n"
    );

    let (code2, stdout2, stderr2) = execute(&["version", "-bogus=123"]);
    assert_eq!(code2, exit::USAGE);
    assert!(stdout2.is_empty());
    assert_eq!(
        stderr2,
        "flag provided but not defined: -bogus\nUsage of version:\n"
    );
}

#[test]
fn version_version_exits_usage_with_unexpected_argument() {
    let (code, stdout, stderr) = execute(&["version", "version"]);
    assert_eq!(code, exit::USAGE);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        "symbrain version: unexpected argument \"version\"\n"
    );
}

#[test]
fn version_unexpected_argument() {
    let (code, stdout, stderr) = execute(&["version", "extra"]);
    assert_eq!(code, exit::USAGE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, "symbrain version: unexpected argument \"extra\"\n");
}

#[test]
fn json_help_exits_usage_with_unknown_command() {
    let (code, stdout, stderr) = execute(&["--json", "help"]);
    assert_eq!(code, exit::USAGE);
    assert!(stdout.is_empty());
    assert!(stderr.starts_with("symbrain: unknown command \"--json\"\n\n"));
    assert!(stderr.contains(USAGE));
}

#[test]
fn unknown_command_quotes_properly() {
    let (code, stdout, stderr) = execute(&["bogus\"cmd"]);
    assert_eq!(code, exit::USAGE);
    assert!(stdout.is_empty());
    assert!(stderr.starts_with("symbrain: unknown command \"bogus\\\"cmd\"\n\n"));
}

#[test]
fn version_json_order_variants() {
    let (code1, stdout1, stderr1) = execute(&["version", "--json"]);
    assert_eq!(code1, exit::OK);
    assert!(stderr1.is_empty());

    let (code2, stdout2, stderr2) = execute(&["--json", "version"]);
    assert_eq!(code2, exit::OK);
    assert!(stderr2.is_empty());
    assert_eq!(stdout1, stdout2);
}

#[test]
fn config_path_prints_path() {
    let (code, stdout, stderr) = execute(&["config", "path"]);
    assert_eq!(code, exit::OK);
    assert!(stdout.ends_with("config.toml\n"));
    assert!(stderr.is_empty());
}

#[test]
fn config_path_rejects_extra_argument() {
    let (code, stdout, stderr) = execute(&["config", "path", "extra"]);
    assert_eq!(code, exit::USAGE);
    assert_eq!(
        stdout,
        "symbrain config path: unexpected argument \"extra\"\n"
    );
    assert!(stderr.is_empty());
}

#[test]
fn config_path_normalizes_unexpected_flags() {
    let (code1, stdout1, stderr1) = execute(&["config", "path", "--unexpected"]);
    assert_eq!(code1, exit::USAGE);
    assert_eq!(
        stdout1,
        "symbrain config path: unexpected argument \"-unexpected\"\n"
    );
    assert!(stderr1.is_empty());

    let (code2, stdout2, stderr2) = execute(&["config", "path", "-unexpected"]);
    assert_eq!(code2, exit::USAGE);
    assert_eq!(
        stdout2,
        "symbrain config path: unexpected argument \"-unexpected\"\n"
    );
    assert!(stderr2.is_empty());

    let (code3, stdout3, stderr3) = execute(&["config", "path", "--"]);
    assert_eq!(code3, exit::USAGE);
    assert_eq!(
        stdout3,
        "symbrain config path: unexpected argument \"--\"\n"
    );
    assert!(stderr3.is_empty());
}

#[test]
fn every_top_level_command_is_handled_natively() {
    let cases: &[(&[&str], u8)] = &[
        (&["help"], exit::OK),
        (&["doctor", "--help"], exit::USAGE),
        (&["init", "--help"], exit::NO_INPUT),
        (&["harness"], exit::USAGE),
        (&["config", "set"], exit::USAGE),
        (&["config"], exit::NO_INPUT),
        (&["config", "frobnicate"], exit::NO_INPUT),
        (&["doctor", "--fix=false", "--help"], exit::USAGE),
        (&["doctor", "--help", "--", "--force-release"], exit::USAGE),
        // Go consumes `--force-release` as the `-vault-agent` value and stops
        // at `--help`, so this case never consults the machine's profiles.
        (
            &["doctor", "--vault-agent", "--force-release", "--help"],
            exit::USAGE,
        ),
        (&["doctor", "--unknown", "--force-release"], exit::USAGE),
        (&["setup", "--fix=false", "--help"], exit::USAGE),
        (
            &["setup", "--fix=TRUE", "--force-release", "--help"],
            exit::USAGE,
        ),
        (
            &["setup", "--help", "--", "--from-source", "/receiver"],
            exit::USAGE,
        ),
        (
            &["setup", "--help", "--from-source", "/receiver"],
            exit::USAGE,
        ),
        (&["frobnicate"], exit::USAGE),
    ];
    for (case, want) in cases {
        let args = case.iter().map(OsString::from).collect::<Vec<_>>();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(run(&args, &mut stdout, &mut stderr), *want, "{case:?}");
    }
}

#[test]
fn config_get_executes_in_process() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let res = run(
        &[OsString::from("config"), OsString::from("get")],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(res, exit::OK);
}

#[test]
fn config_get_extra_arg_rejected() {
    let (code, stdout, stderr) = execute(&["config", "get", "key", "extra"]);
    assert_eq!(code, exit::USAGE);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        "symbrain config get: unexpected argument \"extra\"\n"
    );
}

#[test]
fn config_get_normalizes_extra_flag() {
    let (code, stdout, stderr) = execute(&["config", "get", "key", "--extra"]);
    assert_eq!(code, exit::USAGE);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        "symbrain config get: unexpected argument \"-extra\"\n"
    );
}

#[test]
fn config_terminator_before_get() {
    let (code, stdout, stderr) = execute(&["config", "--", "get", "nonexistent.key"]);
    assert_eq!(code, exit::USAGE);
    assert!(stdout.is_empty());
    assert!(stderr.starts_with("symbrain config get: key \"nonexistent.key\" is not set in "));
}

#[test]
fn config_set_executes_in_process() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let res = run(
        &[
            OsString::from("config"),
            OsString::from("set"),
            OsString::from("key"),
        ],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(res, exit::USAGE);
    assert_eq!(stderr, b"symbrain config set: want exactly <key> <value>\n");
}

#[test]
fn config_terminator_before_set() {
    let (code, stdout, stderr) = execute(&["config", "--", "set"]);
    assert_eq!(code, exit::USAGE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, "symbrain config set: want exactly <key> <value>\n");
}
