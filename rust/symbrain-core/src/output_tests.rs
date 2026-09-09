use super::*;
use std::ffi::OsString;

#[test]
fn resolve_defaults_to_table_and_recognizes_json() {
    assert_eq!(OutputFormat::resolve(""), OutputFormat::Table);
    assert_eq!(OutputFormat::resolve("table"), OutputFormat::Table);
    assert_eq!(OutputFormat::resolve("json"), OutputFormat::Json);
    assert_eq!(OutputFormat::resolve("JSON"), OutputFormat::Json);
    assert_eq!(OutputFormat::resolve("  json  "), OutputFormat::Json);
    assert_eq!(OutputFormat::resolve("future"), OutputFormat::Table);
}

#[test]
fn parse_accepts_empty_and_table_formats() {
    for val in ["", "table", " TABLE ", "Table"] {
        let res = OutputFormat::parse(val);
        assert_eq!(res, Ok(OutputFormat::Table));
    }
}

#[test]
fn parse_accepts_json_formats() {
    for val in ["json", " JSON ", "Json"] {
        let res = OutputFormat::parse(val);
        assert_eq!(res, Ok(OutputFormat::Json));
    }
}

#[test]
fn parse_rejects_unknown_formats() {
    let err = OutputFormat::parse("yaml").unwrap_err();
    assert_eq!(err, OutputError::UnsupportedFormat("yaml".to_string()));
    assert_eq!(
        err.to_string(),
        "unsupported output format \"yaml\" (want table or json)"
    );
}

#[test]
fn extract_accepts_global_flags_anywhere() {
    let args = ["profile", "show", "name", "--json"]
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    let (format, remaining) = extract_format(&args).unwrap();
    assert_eq!(format, OutputFormat::Json);
    let profile_args: Vec<String> = remaining
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert_eq!(profile_args, vec!["profile", "show", "name"]);

    let args2 = ["--output", "json", "version"]
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    let (format2, remaining2) = extract_format(&args2).unwrap();
    assert_eq!(format2, OutputFormat::Json);
    let version_args: Vec<String> = remaining2
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert_eq!(version_args, vec!["version"]);
}

#[test]
fn extract_accepts_all_flag_variants() {
    let variants = [
        ("--json", OutputFormat::Json),
        ("-json", OutputFormat::Json),
        ("--output=table", OutputFormat::Table),
        ("-output=table", OutputFormat::Table),
        ("--output=json", OutputFormat::Json),
        ("-output=json", OutputFormat::Json),
    ];

    for (flag, expected) in variants {
        let args = [OsString::from(flag), OsString::from("version")];
        let (format, remaining) = extract_format(&args).unwrap();
        assert_eq!(format, expected);
        assert_eq!(remaining, vec![OsString::from("version")]);
    }
}

#[test]
fn extract_defaults_to_table_without_flags() {
    let args = ["version"].iter().map(OsString::from).collect::<Vec<_>>();
    let (format, remaining) = extract_format(&args).unwrap();
    assert_eq!(format, OutputFormat::Table);
    assert_eq!(remaining, args);
}

#[test]
fn extract_accepts_flag_after_double_dash() {
    let args = ["version", "--", "--json"]
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    let (format, remaining) = extract_format(&args).unwrap();
    assert_eq!(format, OutputFormat::Json);
    assert_eq!(
        remaining,
        vec![OsString::from("version"), OsString::from("--")]
    );
}

#[test]
fn extract_accepts_duplicate_identical_flags() {
    let args = ["--json", "version", "--json"]
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    let (format, remaining) = extract_format(&args).unwrap();
    assert_eq!(format, OutputFormat::Json);
    let version_args: Vec<String> = remaining
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert_eq!(version_args, vec!["version"]);

    let args2 = ["--output=json", "version", "--output", "json"]
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    let (format2, remaining2) = extract_format(&args2).unwrap();
    assert_eq!(format2, OutputFormat::Json);
    let version_args2: Vec<String> = remaining2
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert_eq!(version_args2, vec!["version"]);
}

#[test]
fn extract_rejects_conflicting_formats() {
    let args = ["--json", "--output", "table"]
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    let err = extract_format(&args).unwrap_err();
    assert_eq!(
        err,
        OutputError::ConflictingFormats {
            current: OutputFormat::Json,
            next: OutputFormat::Table,
        }
    );
    assert_eq!(
        err.to_string(),
        "conflicting output formats \"json\" and \"table\""
    );

    let args2 = ["--output=table", "--json"]
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    let err2 = extract_format(&args2).unwrap_err();
    assert_eq!(
        err2,
        OutputError::ConflictingFormats {
            current: OutputFormat::Table,
            next: OutputFormat::Json,
        }
    );
    assert_eq!(
        err2.to_string(),
        "conflicting output formats \"table\" and \"json\""
    );
}

#[test]
fn render_json_preserves_values_and_appends_newline() {
    #[derive(Serialize)]
    struct TestData<'a> {
        name: &'a str,
        count: u32,
    }

    let mut buf = Vec::new();
    let data = TestData {
        name: "symbrain",
        count: 2,
    };
    render_json(&mut buf, &data).unwrap();

    let output_str = String::from_utf8(buf).unwrap();
    assert_eq!(output_str, "{\"name\":\"symbrain\",\"count\":2}\n");
}

#[test]
fn render_table_uses_table_function() {
    let mut buf = Vec::new();
    let data = "ignored-for-table";
    render(&mut buf, OutputFormat::Table, &data, |w| {
        writeln!(w, "table output")
    })
    .unwrap();

    assert_eq!(String::from_utf8(buf).unwrap(), "table output\n");
}

#[test]
fn render_helper_formats_json_and_table() {
    let mut json_buf = Vec::new();
    render(
        &mut json_buf,
        OutputFormat::Json,
        &serde_json::json!({"status": "ok"}),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(json_buf).unwrap(),
        "{\"status\":\"ok\"}\n"
    );

    let mut table_buf = Vec::new();
    render(
        &mut table_buf,
        OutputFormat::Table,
        &serde_json::json!({"status": "ok"}),
        |w| writeln!(w, "STATUS: OK"),
    )
    .unwrap();
    assert_eq!(String::from_utf8(table_buf).unwrap(), "STATUS: OK\n");
}

#[test]
fn missing_output_value_error_normalizes_to_double_dash() {
    let args1 = [std::ffi::OsString::from("--output")];
    let err1 = extract_format(&args1).unwrap_err();
    assert_eq!(err1.to_string(), "--output requires a value");

    let args2 = [std::ffi::OsString::from("-output")];
    let err2 = extract_format(&args2).unwrap_err();
    assert_eq!(err2.to_string(), "--output requires a value");
}
