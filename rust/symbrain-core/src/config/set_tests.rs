use std::ffi::OsString;
use std::fs;
use tempfile::tempdir;

use super::*;

#[test]
fn set_requires_exactly_two_arguments_and_nonempty_key() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    for args in [vec![], vec!["key"], vec!["key", "value", "extra"]] {
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(
            run_config_set_with_path(
                &path,
                &args.into_iter().map(OsString::from).collect::<Vec<_>>(),
                &mut out,
                &mut err
            ),
            crate::exit::USAGE
        );
        assert_eq!(out, b"");
        assert_eq!(err, b"symbrain config set: want exactly <key> <value>\n");
    }
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(
        run_config_set_with_path(
            &path,
            &[OsString::new(), OsString::from("x")],
            &mut out,
            &mut err
        ),
        crate::exit::USAGE
    );
    assert_eq!(err, b"symbrain config set: key must not be empty\n");
}

#[test]
fn set_infers_go_bool_and_int64_spellings_then_string() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    for (i, value) in [
        "1",
        "t",
        "T",
        "TRUE",
        "true",
        "True",
        "0",
        "f",
        "F",
        "FALSE",
        "false",
        "False",
        "9223372036854775807",
        "9223372036854775808",
        "text",
    ]
    .iter()
    .enumerate()
    {
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(
            run_config_set_with_path(
                &path,
                &[OsString::from(format!("k{i}")), OsString::from(*value)],
                &mut out,
                &mut err
            ),
            crate::exit::OK
        );
        assert!(err.is_empty());
    }
    let content = fs::read_to_string(path).unwrap();
    for key in [
        "k0 = true",
        "k6 = false",
        "k12 = 9223372036854775807",
        "k13 = \"9223372036854775808\"",
        "k14 = \"text\"",
    ] {
        assert!(content.contains(key), "missing {key} in {content}");
    }
}

#[test]
fn set_creates_nested_tables_and_matches_burnt_sushi_ordering() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nested").join("config.toml");
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(
        run_config_set_with_path(
            &path,
            &[OsString::from("z.deep"), OsString::from("v")],
            &mut out,
            &mut err
        ),
        crate::exit::OK
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "[z]\n  deep = \"v\"\n");
    assert_eq!(
        out,
        format!("set z.deep = v in {}\n", path.display()).as_bytes()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn set_rewrites_comments_and_preserves_existing_values() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        "# removed\nz = 1\na = 2\n\n[beta]\ny = 3\n\n[alpha]\nx = 4\n",
    )
    .unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(
        run_config_set_with_path(
            &path,
            &[OsString::from("beta.new"), OsString::from("true")],
            &mut out,
            &mut err
        ),
        crate::exit::OK
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "a = 2\nz = 1\n\n[alpha]\n  x = 4\n\n[beta]\n  new = true\n  y = 3\n"
    );
}

#[test]
fn set_rejects_intermediate_scalar_without_writing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "default_profile = \"personal\"\n").unwrap();
    let before = fs::read(&path).unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(
        run_config_set_with_path(
            &path,
            &[OsString::from("default_profile.x"), OsString::from("y")],
            &mut out,
            &mut err
        ),
        crate::exit::USAGE
    );
    assert_eq!(err, b"symbrain config set: cannot set \"default_profile.x\": \"default_profile\" is not a table\n");
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn set_handles_relative_path_without_parent() {
    let filename = format!("temp_set_test_{}.toml", std::process::id());
    let path = std::path::Path::new(&filename);
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_config_set_with_path(
        path,
        &[OsString::from("key"), OsString::from("val")],
        &mut out,
        &mut err,
    );
    let _ = fs::remove_file(path);
    assert_eq!(code, crate::exit::OK);
}

#[test]
fn set_preview_is_non_mutating_and_does_not_reveal_value() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "default_profile = \"personal\"\n").unwrap();
    let before = fs::read(&path).unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(
        run_config_set_with_path(
            &path,
            &[
                "--preview".into(),
                "default_profile".into(),
                "secret-value".into()
            ],
            &mut out,
            &mut err
        ),
        crate::exit::OK
    );
    assert!(String::from_utf8_lossy(&out).contains("preview config set default_profile: changed"));
    assert!(!String::from_utf8_lossy(&out).contains("secret-value"));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn set_writes_backup_and_preserves_original_when_backup_fails() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "default_profile = \"personal\"\n").unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(
        run_config_set_with_path(
            &path,
            &["default_profile".into(), "restricted".into()],
            &mut out,
            &mut err
        ),
        crate::exit::OK
    );
    assert_eq!(
        fs::read(path.with_file_name("config.toml.bak")).unwrap(),
        b"default_profile = \"personal\"\n"
    );
    fs::remove_file(path.with_file_name("config.toml.bak")).unwrap();
    fs::create_dir(path.with_file_name("config.toml.bak")).unwrap();
    let before = fs::read(&path).unwrap();
    out.clear();
    err.clear();
    assert_eq!(
        run_config_set_with_path(
            &path,
            &["default_profile".into(), "blocked".into()],
            &mut out,
            &mut err
        ),
        crate::exit::GENERIC
    );
    assert_eq!(fs::read(path).unwrap(), before);
}
