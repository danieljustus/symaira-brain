//! Expected strings come from the real Go loader (internal/config.Load on
//! corekit v0.18.2) run in disposable roots; they are not derived from Rust.
use super::*;

fn apply(text: &str) -> Result<[bool; 3], String> {
    let mut modules = [false; 3];
    apply_into(&mut modules, text).map(|()| modules)
}

fn apply_into(modules: &mut [bool; 3], text: &str) -> Result<(), String> {
    let document = toml_syntax::parse(text.as_bytes()).expect("valid TOML");
    apply_document(modules, &document)
}

#[test]
fn present_false_clears_a_previous_true_like_corekit_v0_18() {
    let mut modules = [false; 3];
    apply_into(&mut modules, "[modules]\nbrowse = true\nscope = \"T\"\n").unwrap();
    assert_eq!(modules, [true, false, true]);
    apply_into(&mut modules, "[modules]\nbrowse = false\n").unwrap();
    assert_eq!(modules, [false, false, true]);
}

#[test]
fn zero_values_are_converted_not_skipped() {
    assert_eq!(
        apply("[modules]\nbrowse = 0\n").unwrap_err(),
        "field \"modules\": field \"browse\": cannot convert int64 to bool"
    );
    assert_eq!(
        apply("[modules]\nbrowse = \"\"\n").unwrap_err(),
        "field \"modules\": field \"browse\": cannot parse \"\" as bool: strconv.ParseBool: parsing \"\": invalid syntax"
    );
    assert_eq!(
        apply("default_profile = 0\n").unwrap_err(),
        "field \"default_profile\": cannot convert int64 to string"
    );
    assert_eq!(apply("default_profile = \"\"\n"), Ok([false; 3]));
}

#[test]
fn go_type_names_cover_every_toml_kind() {
    for (text, expected) in [
        ("[audit]\nenabled = []\n", "[]interface {}"),
        ("[audit]\nenabled = 1.5\n", "float64"),
        ("[audit]\nenabled = 1979-05-27\n", "time.Time"),
        ("[audit]\nenabled = 1979-05-27T07:32:00Z\n", "time.Time"),
        ("[audit.enabled]\nx = 1\n", "map[string]interface {}"),
        ("[audit]\nenabled = { x = 1 }\n", "map[string]interface {}"),
        ("[[audit.enabled]]\nx = 1\n", "[]map[string]interface {}"),
    ] {
        assert_eq!(
            apply(text).unwrap_err(),
            format!("field \"audit\": field \"enabled\": cannot convert {expected} to bool"),
            "{text}"
        );
    }
}

#[test]
fn sections_that_are_not_maps_are_ignored_and_order_is_struct_order() {
    assert_eq!(apply("audit = 5\nmodules = [1]\n"), Ok([false; 3]));
    assert_eq!(
        apply("[patterns]\npromotion_threshold = \"a\"\n[audit]\nenabled = 3\n").unwrap_err(),
        "field \"audit\": field \"enabled\": cannot convert int64 to bool"
    );
    assert_eq!(
        apply("[servers.scope]\nbinary_path = [1]\n").unwrap_err(),
        "field \"servers\": field \"scope\": field \"binary_path\": cannot convert []interface {} to string"
    );
}

#[test]
fn integer_fields_follow_configkit_conversions() {
    assert_eq!(
        apply("[patterns]\npromotion_threshold = 2.7\n"),
        Ok([false; 3])
    );
    assert_eq!(
        apply("[patterns]\npromotion_threshold = -4\n"),
        Ok([false; 3])
    );
    assert_eq!(
        apply("[patterns]\npromotion_threshold = \"+5\"\n"),
        Ok([false; 3])
    );
    assert_eq!(
        apply("[patterns]\npromotion_threshold = \"99999999999999999999\"\n").unwrap_err(),
        "field \"patterns\": field \"promotion_threshold\": cannot parse \"99999999999999999999\" as int: strconv.ParseInt: parsing \"99999999999999999999\": value out of range"
    );
    assert_eq!(
        apply("[patterns]\npromotion_threshold = true\n").unwrap_err(),
        "field \"patterns\": field \"promotion_threshold\": cannot convert bool to int"
    );
}

#[test]
fn go_bool_and_int_spellings() {
    for value in ["1", "t", "T", "TRUE", "True", "true"] {
        assert_eq!(parse_go_bool(value), Ok(true), "{value}");
    }
    for value in ["0", "f", "F", "FALSE", "False", "false"] {
        assert_eq!(parse_go_bool(value), Ok(false), "{value}");
    }
    for value in ["", "yes", "on", "2", "tRuE"] {
        assert_eq!(parse_go_bool(value), Err(()), "{value}");
    }
    assert!(parse_int(b"-9223372036854775808").is_ok());
    assert!(parse_int(b"9223372036854775808").is_err());
    assert!(parse_int(b"1_0").is_err());
    assert!(parse_int(b"").is_err());
    assert_eq!(
        parse_bool(b"x").unwrap_err(),
        "cannot parse \"x\" as bool: strconv.ParseBool: parsing \"x\": invalid syntax"
    );
}

/// Go `os.ReadFile` on a directory: Unix opens it and the read fails with
/// `EISDIR`; Windows (`FILE_FLAG_BACKUP_SEMANTICS`) opens it and the read fails
/// with `ERROR_INVALID_FUNCTION`, "Incorrect function.". The wording is taken
/// from the OS error code so the check also holds on a localized Windows.
#[test]
fn directory_config_reports_the_read_failure_like_go() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config.toml");
    std::fs::create_dir(&config).unwrap();
    let shown = config.display().to_string();
    let detail = if cfg!(windows) {
        symbrain_managed::format_io_error(&std::io::Error::from_raw_os_error(1))
    } else {
        "is a directory".to_owned()
    };
    let mut modules = [false; 3];
    let error = merge_file(&mut modules, &config, shown.as_bytes()).unwrap_err();
    assert_eq!(
        String::from_utf8_lossy(&error),
        format!("failed to parse {shown}: read {shown}: {detail}")
    );
    assert!(config.is_dir());
}
