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
