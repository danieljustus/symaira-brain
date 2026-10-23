use super::*;

#[test]
fn jsonc_preserves_urls_and_removes_comments() {
    let parsed = parse_config(
        br#"{// comment
          "mcpServers":{"s":{"url":"https://example.test/mcp"}}
        }"#,
        "mcpServers",
    )
    .expect("JSONC");
    assert_eq!(parsed["s"].transport(), "http");
    assert_eq!(parsed["s"].command_or_url(), "https://example.test/mcp");
}

#[test]
fn opencode_environment_is_merged_and_redacted() {
    let parsed = parse_config(br#"{"mcp":{"s":{"type":"local","command":"run","env":{"A":"old","B":"x"},"environment":{"A":"new","C":"y"}}}}"#, "mcp").expect("OpenCode");
    let view = view(Server {
        name: "s".into(),
        client: "opencode".into(),
        command: "run".into(),
        args: vec![],
        env: parsed["s"].merged_env(),
        transport: parsed["s"].transport(),
    });
    assert_eq!(
        view.env,
        BTreeMap::from([
            (String::from("A"), String::from("REDACTED")),
            (String::from("B"), String::from("REDACTED")),
            (String::from("C"), String::from("REDACTED"))
        ])
    );
}

#[test]
fn empty_format_value_resolves_to_default() {
    let args = vec![std::ffi::OsString::from("--format=")];
    assert_eq!(
        parse_flags(&args, &mut Vec::new(), &mut Vec::new()),
        Ok(None)
    );
}

#[test]
fn missing_source_error_classification_matches_go_platform_behavior() {
    let missing_file = io::Error::from(io::ErrorKind::NotFound);
    assert!(missing_source_is_silent(&missing_file));

    #[cfg(windows)]
    {
        assert!(missing_source_is_silent(&io::Error::from_raw_os_error(2)));
        let missing_parent = io::Error::from_raw_os_error(3);
        assert!(!missing_source_is_silent(&missing_parent));
    }
}

#[test]
fn source_paths_clean_parent_components() {
    let raw = PathBuf::from("parent")
        .join("discard")
        .join("..")
        .join("config.json");
    assert_eq!(
        clean_native_path(&raw),
        PathBuf::from("parent").join("config.json")
    );
    assert_eq!(
        clean_native_path(Path::new("foo/../claude/config.json")),
        PathBuf::from("claude").join("config.json")
    );
}

#[cfg(windows)]
#[test]
fn source_paths_clean_slash_form_windows_base() {
    let raw = Path::new("C:/parent/../xdg/claude/config.json");
    assert_eq!(
        clean_native_path(raw).to_string_lossy(),
        r"C:\xdg\claude\config.json"
    );
}

#[test]
fn json_typed_fields_fail_instead_of_being_dropped() {
    for document in [
        br#"{"mcpServers":{"s":{"command":42}}}"#.as_slice(),
        br#"{"mcpServers":{"s":{"args":[42]}}}"#.as_slice(),
        br#"{"mcpServers":{"s":{"env":{"TOKEN":42}}}}"#.as_slice(),
    ] {
        assert!(parse_config(document, "mcpServers").is_err());
    }
}

#[test]
fn yaml_environment_is_not_merged() {
    let parsed = parse_config(
        b"mcpServers:\n  s:\n    command: run\n    environment:\n      TOKEN: ignored\n",
        "mcpServers",
    )
    .expect("YAML");
    assert!(parsed["s"].env.is_empty());
}

#[test]
fn yaml_dependency_preserves_typed_fields_and_aliases() {
    let fields = "    command: '2'\n    args: [text, '2', 2, true, null, yes]\n    env: {QUOTED: '2', INTEGER: 2, BOOLEAN: true, ABSENT: null, WORD: yes}\n";
    for input in [
        format!("mcpServers:\n  s:\n{fields}"),
        format!("defaults: &defaults\n{fields}mcpServers:\n  s: *defaults\n"),
    ] {
        let parsed = parse_config(input.as_bytes(), "mcpServers").expect("typed YAML config");
        assert_eq!(parsed["s"].command, "2");
        assert_eq!(parsed["s"].args, ["text", "2", "yes"]);
        assert_eq!(
            parsed["s"].env,
            BTreeMap::from([
                (String::from("QUOTED"), String::from("2")),
                (String::from("WORD"), String::from("yes")),
            ])
        );
    }
    for malformed in ["mcpServers: [unclosed", "mcpServers: *missing"] {
        let error = parse_config(malformed.as_bytes(), "mcpServers")
            .err()
            .expect("invalid YAML rejected");
        assert!(error.starts_with("parse YAML:"));
    }
}

#[test]
fn json_decode_follows_go_encoding_json() {
    // Go: exact-or-folded field names, duplicate `env` objects merge, `null` clears.
    let parsed = parse_config(
        br#"{"mcpServers":{"s":{"COMMAND":"c","Env":{"A":"1"},"env":{"B":"2"},"arg\u017f":["x"]}}}"#,
        "mcpServers",
    )
    .expect("folded fields");
    assert_eq!(parsed["s"].command, "c");
    assert_eq!(parsed["s"].args, ["x"]);
    assert_eq!(parsed["s"].env.len(), 2);
    let cleared = parse_config(
        br#"{"mcpServers":{"s":{"command":"c","env":{"A":"1"},"env":null}}}"#,
        "mcpServers",
    )
    .expect("null clears");
    assert!(cleared["s"].env.is_empty());

    // The first type error in document order wins, with Go's wording.
    let error = parse_config(
        br#"{"mcpServers":{"z":{"command":1},"a":{"url":2}}}"#,
        "mcpServers",
    )
    .err()
    .expect("type error");
    assert_eq!(
        error,
        "parse \"mcpServers\": json: cannot unmarshal number into Go struct field Entry.command of type string"
    );
    let error = parse_config(br#"{"mcp":[]}"#, "mcp")
        .err()
        .expect("array root");
    assert_eq!(
        error,
        "parse \"mcp\": json: cannot unmarshal array into Go value of type map[string]mcpcfgkit.Entry"
    );
    // Syntax errors beat type errors; trailing garbage is reported after the value.
    let error = parse_config(br#"{"mcpServers":{"s":{"command":1}}} x"#, "mcpServers")
        .err()
        .expect("syntax error");
    assert_eq!(
        error,
        "parse JSON: invalid character 'x' after top-level value"
    );
}

#[test]
fn json_inventory_is_html_escaped_like_go() {
    assert_eq!(
        go_html_escape("a<b>&\u{2028}\u{2029}é"),
        "a\\u003cb\\u003e\\u0026\\u2028\\u2029é"
    );
}

#[cfg(unix)]
#[test]
fn directory_read_failure_is_a_read_error_with_go_errno_text() {
    let dir = std::env::temp_dir();
    let failure = read_source(&dir).expect_err("directory");
    assert_eq!(failure.op, "read");
    assert!(read_error_message(&dir, &failure).ends_with(": is a directory"));
}
