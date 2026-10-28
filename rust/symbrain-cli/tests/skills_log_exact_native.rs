//! CLI byte checks for the native `skills log` slice (SKL-017): the log path
//! under the home directory, symlink and read-failure shapes, line decoding
//! and the shapes that must stay on Go.

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::process::{Command, Output};

use tempfile::TempDir;

fn command(root: &TempDir, args: &[&str]) -> Command {
    let home = root.path().join("home");
    let config = root.path().join("config");
    let project = root.path().join("project");
    for path in [&home, &config, &project] {
        std::fs::create_dir_all(path).unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .envs(coverage::profile_environment())
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_DATA_HOME", root.path().join("data"))
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("TZ", "UTC")
        .current_dir(project)
        .args(args);
    command
}

fn run(root: &TempDir, args: &[&str]) -> Output {
    command(root, args).output().unwrap()
}

fn log_dir(root: &TempDir) -> std::path::PathBuf {
    let dir = root.path().join("home/.local/share/symskills");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_log(root: &TempDir, name: &str, lines: &[&str]) {
    let mut body = lines.join("\n");
    body.push('\n');
    std::fs::write(log_dir(root).join(name), body).unwrap();
}

fn event(ts: &str, skill: &str, target: &str) -> String {
    format!(
        r#"{{"ts":"{ts}","event":"install","skill":"{skill}","target":"{target}","outcome":"ok"}}"#
    )
}

fn assert_go_fallback(output: &Output, label: &str) {
    assert_eq!(output.status.code(), Some(1), "{label}");
    assert!(output.stdout.is_empty(), "{label}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"),
        "{label}: {:?}",
        output.stderr
    );
}

#[test]
fn records_are_newest_first_across_rotated_and_current_segments() {
    let root = TempDir::new().unwrap();
    write_log(
        &root,
        "events.1.jsonl",
        &[&event("2026-01-01T00:00:00Z", "old", "claude")],
    );
    write_log(
        &root,
        "events.jsonl",
        &[
            &event("2026-01-02T00:00:00Z", "b", "opencode"),
            &event("2026-01-03T00:00:00Z", "c", "opencode"),
            &event("2026-01-02T00:00:00Z", "a", "opencode"),
        ],
    );
    let output = run(&root, &["skills", "log"]);
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "WHEN\tEVENT\tSKILL\tTARGET\tOUTCOME\n\
         2026-01-03T00:00:00Z\tinstall\tc\topencode\tok\n\
         2026-01-02T00:00:00Z\tinstall\tb\topencode\tok\n\
         2026-01-02T00:00:00Z\tinstall\ta\topencode\tok\n\
         2026-01-01T00:00:00Z\tinstall\told\tclaude\tok\n"
    );
}

#[test]
fn limit_filters_and_flag_spellings_match_go() {
    let root = TempDir::new().unwrap();
    write_log(
        &root,
        "events.jsonl",
        &[
            &event("2026-01-01T00:00:00Z", "a", "claude"),
            &event("2026-01-02T00:00:00Z", "b", "opencode"),
            &event("2026-01-03T00:00:00Z", "a", "opencode"),
        ],
    );
    let skills = |args: &[&str]| -> Vec<String> {
        let output = run(&root, args);
        assert!(output.status.success(), "{args:?}: {:?}", output.stderr);
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|record| record["ts"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(skills(&["skills", "log", "--json", "-l", "2"]).len(), 2);
    assert_eq!(skills(&["skills", "log", "--json", "--limit=1"]).len(), 1);
    assert_eq!(
        skills(&["skills", "log", "--json", "--limit", "0"]).len(),
        3
    );
    assert_eq!(skills(&["skills", "log", "--json", "--limit=-4"]).len(), 3);
    assert_eq!(
        skills(&["skills", "log", "--json", "--skill", " a "]).len(),
        2
    );
    assert_eq!(
        skills(&["skills", "log", "--json", "-target=opencode"]).len(),
        2
    );
    assert_eq!(
        skills(&["skills", "log", "--json", "--scope", "bogus"]).len(),
        3
    );
    assert_eq!(
        skills(&[
            "skills", "log", "--json", "--skill", "a", "--target", "opencode"
        ]),
        ["2026-01-03T00:00:00Z"]
    );
}

#[test]
fn config_files_and_symskills_variables_cannot_change_the_report() {
    let root = TempDir::new().unwrap();
    write_log(
        &root,
        "events.jsonl",
        &[&event("2026-01-01T00:00:00Z", "a", "claude")],
    );
    let plain = run(&root, &["skills", "log", "--json"]);
    assert!(plain.status.success());
    std::fs::create_dir_all(root.path().join("config/symskills")).unwrap();
    std::fs::write(
        root.path().join("config/symskills/config.toml"),
        "not [valid toml\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("project/.symskills.toml"),
        "vcs = { enabled = false }\n",
    )
    .unwrap();
    let configured = command(&root, &["skills", "log", "--json"])
        .env("SYMSKILLS_LIBRARY_DIR", "relative")
        .env("SYMSKILLS_VCS_ENABLED", "maybe")
        .output()
        .unwrap();
    assert_eq!(configured.stdout, plain.stdout);
    assert!(configured.stderr.is_empty());
}

#[test]
fn json_escapes_html_and_line_separators_like_go() {
    let root = TempDir::new().unwrap();
    write_log(
        &root,
        "events.jsonl",
        &["{\"ts\":\"t\",\"event\":\"<a>&\u{2028}\u{2029}\",\"outcome\":\"ok\"}"],
    );
    let output = run(&root, &["skills", "log", "--json"]);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[{\"ts\":\"t\",\"event\":\"\\u003ca\\u003e\\u0026\\u2028\\u2029\",\"outcome\":\"ok\",\"actor\":\"\"}]\n"
    );
}

#[test]
fn undecidable_lines_and_flags_keep_go_fallback() {
    for (label, line) in [
        ("upper-case key", r#"{"TS":"x","event":"e","outcome":"ok"}"#),
        ("duplicate key", r#"{"ts":"a","ts":"b","outcome":"ok"}"#),
        ("null value", r#"{"ts":"a","event":null}"#),
        ("top-level null", "null"),
        ("lone surrogate", r#"{"ts":"a","event":"\ud800"}"#),
        ("number value", r#"{"ts":"a","event":5}"#),
    ] {
        let root = TempDir::new().unwrap();
        write_log(&root, "events.jsonl", &[line]);
        assert_go_fallback(&run(&root, &["skills", "log", "--json"]), label);
    }
    let root = TempDir::new().unwrap();
    write_log(
        &root,
        "events.jsonl",
        &[&event("2026-01-01T00:00:00Z", "a", "claude")],
    );
    for args in [
        &["skills", "log", "--limit", "010"][..],
        &["skills", "log", "--limit=0x2"],
        &["skills", "log", "--limit", "many"],
        &["skills", "log", "--bogus"],
        &["skills", "log", "extra"],
        &["skills", "log", "-h"],
        &["skills", "log", "--skill"],
        &["skills", "log", "--", "--limit", "1"],
    ] {
        assert_go_fallback(&run(&root, args), &args.join(" "));
    }
}

#[test]
fn skipped_lines_never_reach_the_report() {
    let root = TempDir::new().unwrap();
    write_log(
        &root,
        "events.jsonl",
        &[
            "",
            "   \t ",
            "not json",
            "[1,2]",
            "\"text\"",
            "42",
            r#"{"ts":"x"} trailing"#,
            r#"{"ts":"a","event":"e","outcome":"ok","extra":{"deep":[1,2.5e999,null,true]}}"#,
        ],
    );
    let output = run(&root, &["skills", "log", "--json"]);
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[{\"ts\":\"a\",\"event\":\"e\",\"outcome\":\"ok\",\"actor\":\"\"}]\n"
    );
}

#[test]
fn missing_log_reports_nothing_recorded() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["skills", "log"]);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"No recorded skill operations.\n");
    let json = run(&root, &["skills", "log", "--json"]);
    assert_eq!(json.stdout, b"[]\n");
}

#[cfg(unix)]
mod posix {
    use super::*;
    use std::os::unix::fs::symlink;

    fn entry(root: &TempDir, name: &str) -> std::path::PathBuf {
        log_dir(root).join(name)
    }

    #[test]
    fn symlinked_log_and_ancestor_are_followed() {
        let root = TempDir::new().unwrap();
        let target = root.path().join("elsewhere.jsonl");
        std::fs::write(
            &target,
            event("2026-01-01T00:00:00Z", "linked", "claude") + "\n",
        )
        .unwrap();
        symlink(&target, entry(&root, "events.jsonl")).unwrap();
        let output = run(&root, &["skills", "log"]);
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("linked"),
            "{output:?}"
        );

        let second = TempDir::new().unwrap();
        let real = second.path().join("real-local");
        std::fs::create_dir_all(real.join("share/symskills")).unwrap();
        std::fs::write(
            real.join("share/symskills/events.jsonl"),
            event("2026-01-01T00:00:00Z", "ancestor", "claude") + "\n",
        )
        .unwrap();
        std::fs::create_dir_all(second.path().join("home")).unwrap();
        symlink(&real, second.path().join("home/.local")).unwrap();
        let output = run(&second, &["skills", "log"]);
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("ancestor"),
            "{output:?}"
        );
    }

    #[test]
    fn dangling_links_count_as_absent() {
        let root = TempDir::new().unwrap();
        symlink(root.path().join("nowhere"), entry(&root, "events.jsonl")).unwrap();
        symlink(root.path().join("nowhere"), entry(&root, "events.1.jsonl")).unwrap();
        let output = run(&root, &["skills", "log"]);
        assert_eq!(output.stdout, b"No recorded skill operations.\n");
    }

    fn stderr(root: &TempDir) -> String {
        let output = run(root, &["skills", "log"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        String::from_utf8(output.stderr).unwrap()
    }

    fn home_path(root: &TempDir, relative: &str) -> String {
        root.path()
            .join("home")
            .join(relative)
            .display()
            .to_string()
    }

    #[test]
    fn read_failures_use_go_path_error_text() {
        let root = TempDir::new().unwrap();
        std::fs::create_dir_all(entry(&root, "events.jsonl")).unwrap();
        assert_eq!(
            stderr(&root),
            format!(
                "symbrain skills log: read operation log: read {}: is a directory\n",
                home_path(&root, ".local/share/symskills/events.jsonl")
            )
        );

        let loop_root = TempDir::new().unwrap();
        symlink("events.jsonl", entry(&loop_root, "events.jsonl")).unwrap();
        assert_eq!(
            stderr(&loop_root),
            format!(
                "symbrain skills log: read operation log: open {}: too many levels of symbolic links\n",
                home_path(&loop_root, ".local/share/symskills/events.jsonl")
            )
        );

        let file_root = TempDir::new().unwrap();
        std::fs::create_dir_all(file_root.path().join("home/.local/share")).unwrap();
        std::fs::write(file_root.path().join("home/.local/share/symskills"), b"x").unwrap();
        assert_eq!(
            stderr(&file_root),
            format!(
                "symbrain skills log: read operation log: open {}: not a directory\n",
                home_path(&file_root, ".local/share/symskills/events.1.jsonl")
            )
        );
    }

    #[test]
    fn unclean_home_error_text_stays_on_go() {
        let root = TempDir::new().unwrap();
        std::fs::create_dir_all(entry(&root, "events.jsonl")).unwrap();
        let doubled = root
            .path()
            .join("home")
            .display()
            .to_string()
            .replace("/home", "//home");
        let output = command(&root, &["skills", "log"])
            .env("HOME", doubled)
            .output()
            .unwrap();
        assert_go_fallback(&output, "doubled slash home");
    }

    #[test]
    fn home_with_dot_dot_stays_on_go() {
        let root = TempDir::new().unwrap();
        let output = command(&root, &["skills", "log"])
            .env("HOME", root.path().join("home/../home"))
            .output()
            .unwrap();
        assert_go_fallback(&output, "dot-dot home");
    }
}
