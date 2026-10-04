use super::config::*;
use super::discovery::*;
use super::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
#[cfg(windows)]
use std::{env, path::PathBuf};
use toml_edit::{DocumentMut, Value};

fn parse_and_validate_text(text: &str) -> Option<LoadedConfig> {
    let doc = text.parse::<DocumentMut>().ok()?;
    parse_and_validate(&doc)
}

fn stdio(command: &str, args: &[&str]) -> Discovered {
    Discovered {
        name: "s".to_owned(),
        client: "cursor".to_owned(),
        command: command.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        transport: "stdio".to_owned(),
        env: BTreeMap::new(),
    }
}

fn native_command() -> String {
    #[cfg(windows)]
    {
        let system_root = env::var_os("SystemRoot")
            .or_else(|| env::var_os("windir"))
            .unwrap_or_else(|| r"C:\Windows".into());
        PathBuf::from(system_root)
            .join("System32")
            .join("where.exe")
            .to_string_lossy()
            .into_owned()
    }
    #[cfg(not(windows))]
    {
        "/usr/bin/true".to_owned()
    }
}

fn parent_path_variant(path: &str) -> String {
    let path = Path::new(path);
    let parent = path.parent().expect("absolute path has parent");
    let grandparent = parent.parent().expect("test path has grandparent");
    let parent_name = parent.file_name().expect("parent name");
    let file_name = path.file_name().expect("file name");
    grandparent
        .join(parent_name)
        .join("..")
        .join(parent_name)
        .join(file_name)
        .to_string_lossy()
        .into_owned()
}

#[test]
fn allowlist_is_deny_by_default_and_prefix_matched() {
    let command = native_command();
    let entry = SpawnEntry {
        path: command.clone(),
        argv_prefix: vec!["--once".to_owned()],
    };
    assert!(!allows(&stdio(&command, &["--once"]), &[]));
    assert!(allows(
        &stdio(&command, &["--once", "--extra"]),
        std::slice::from_ref(&entry)
    ));
    assert!(!allows(
        &stdio(&command, &["--twice"]),
        std::slice::from_ref(&entry)
    ));
    // Relative commands can never match an absolute entry.
    assert!(!allows(&stdio("true", &[]), std::slice::from_ref(&entry)));
    // ..-containing paths clean to the same file, as filepath.Clean does.
    assert!(allows(
        &stdio(&parent_path_variant(&command), &["--once"]),
        std::slice::from_ref(&entry)
    ));
    // HTTP servers are not gated at all.
    let mut http = stdio("https://example.test/mcp", &[]);
    http.transport = "http".to_owned();
    assert!(allows(&http, &[]));
}

#[test]
fn secret_heuristic_matches_go() {
    assert!(looks_like_secret("SECRET_KEY", "literal"));
    assert!(looks_like_secret("ANYTHING", "sk-abc"));
    assert!(!looks_like_secret("SECRET_KEY", "${FROM_ENV}"));
    assert!(!looks_like_secret("SECRET_KEY", "$FROM_ENV"));
    assert!(looks_like_secret("SECRET_KEY", "$not-a-reference"));
    assert!(!looks_like_secret("SECRET_KEY", ""));
    assert!(!looks_like_secret("PLAIN", "value"));
}

#[test]
fn config_parses_the_healthy_fixture_and_gates_on_invalid_toml() {
    let command = native_command();
    let command = Value::from(command.as_str());
    let healthy = format!(
        "[defaults]\nshell = \"allow\"\nread_secret = \"deny\"\n\n[[rules]]\nmatch.server = \"symmemory\"\nmatch.tool = \"memory_search\"\ndecision = \"allow\"\n\n[spawn]\n[[spawn.allowlist]]\npath = {command}\n"
    );
    let loaded = parse_and_validate_text(&healthy).expect("healthy config is native");
    assert_eq!(loaded.rules, 1);
    assert_eq!(
        loaded.allowlist,
        vec![SpawnEntry {
            path: native_command(),
            argv_prefix: Vec::new(),
        }]
    );

    // Parsing and validation both fail closed here.
    assert!(parse_and_validate_text("not [valid = toml").is_none());
    // validate() rejections gate too.
    assert!(parse_and_validate_text("[defaults]\nshell = \"nonsense\"\n").is_none());
    assert!(
        parse_and_validate_text("[spawn]\n[[spawn.allowlist]]\npath = \"relative\"\n").is_none()
    );
    assert!(parse_and_validate_text("[[rules]]\ndecision = \"allow\"\n[rules.match]\n").is_none());
    assert!(parse_and_validate_text("[sequence]\nenabled = true\nthreshold = 1\n").is_none());
    assert!(parse_and_validate_text("[sequence]\nenabled = true\n").is_some());
}

#[test]
fn unsupported_missing_equals_offenders_stay_gated() {
    for text in [
        "name \"value\"",
        "name 'value'",
        "name \\ value",
        "name \x01 value",
        "name é value",
        "name",
    ] {
        let error = text
            .parse::<DocumentMut>()
            .expect_err("the fixture must be invalid TOML");
        assert_eq!(
            go_missing_equals_diagnostic(text, &error),
            None,
            "must leave {text:?} to Go"
        );
    }
}

#[test]
fn audit_status_reports_all_three_states() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("audit.log");
    assert_eq!(
        audit_status(&log).as_ref().map(|status| status.0.as_ref()),
        Some("not initialized (created on first 'symguard decide')".as_bytes())
    );
    fs::write(&log, b"{\"entry_id\":\"1\"}\n").expect("write log");
    assert_eq!(
        audit_status(&log).as_ref().map(|status| status.0.as_ref()),
        Some("ok (JSONL, chain anchor pending Phase 3 sink)".as_bytes())
    );
    fs::write(
        dir.path().join("audit.log.anchor"),
        br#"{"last_entry_hash":"abc","entry_count":1,"schema_version":2}"#,
    )
    .expect("write anchor");
    assert_eq!(
        audit_status(&log).as_ref().map(|status| status.0.as_ref()),
        Some("ok (hash-chained, anchor present)".as_bytes())
    );
    // Syntax errors use the shared Go-compatible JSON scanner.
    fs::write(dir.path().join("audit.log.anchor"), b"not json").expect("write anchor");
    let status = audit_status(&log).expect("syntax error is native");
    assert!(status.0.as_ref().ends_with(
        b"auditkit: parse anchor: invalid character 'o' in literal null (expecting 'u')"
    ));
    assert!(status.1);
    // The actual Go oracle's first typed field diagnostic is native.
    fs::write(
        dir.path().join("audit.log.anchor"),
        br#"{"entry_count":"one"}"#,
    )
    .expect("write anchor");
    let status = audit_status(&log).expect("type error is native");
    assert!(status.0.as_ref().ends_with(b"auditkit: parse anchor: json: cannot unmarshal string into Go struct field ChainAnchor.entry_count of type int64"));
    assert!(status.1);
}

#[test]
fn multiple_secret_keys_on_one_server_are_sorted() {
    let mut server = stdio("/usr/bin/env", &[]);
    server.env.insert("SECRET_KEY".to_owned(), "a".to_owned());
    server.env.insert("API_KEY".to_owned(), "b".to_owned());
    let checks = check_servers(vec![server], &[]);
    assert_eq!(checks[0].secrets, ["API_KEY", "SECRET_KEY"]);
}

#[test]
fn equivalent_inline_configuration_keeps_rules_allowlist_and_type_gates() {
    let command = toml_edit::Value::from(native_command());
    let inline = format!(
        "defaults={{read=\"allow\"}}\nrules=[{{decision=\"allow\",match={{server=\"owned\"}}}}]\nspawn={{allowlist=[{{path={command},argv_prefix=[\"owned\"]}}]}}\nremote=[]\naudit={{encrypt=true}}\nsequence={{enabled=true,threshold=0}}\n"
    );
    let loaded = parse_and_validate_text(&inline).expect("typed inline configuration is native");
    assert_eq!(loaded.rules, 1);
    assert_eq!(loaded.allowlist.len(), 1);
    assert_eq!(loaded.allowlist[0].path, native_command());
    assert_eq!(loaded.allowlist[0].argv_prefix, ["owned"]);
    assert!(parse_and_validate_text("rules=[]\nremote=[]\nspawn={allowlist=[]}\n").is_some());
    for text in [
        "rules=[1]\n",
        "remote=[1]\n",
        "spawn={allowlist=[1]}\n",
        "defaults={read=1}\n",
        "rules=[{decision=\"allow\",match={command_contains=[1]}}]\n",
        "proxy={owned=1}\n",
    ] {
        assert!(
            parse_and_validate_text(text).is_none(),
            "must remain gated: {text}"
        );
    }
}
