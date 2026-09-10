use super::*;
use crate::config::format::format_go_quoted;
use std::ffi::OsString;
use std::fs;
use tempfile::tempdir;

#[cfg(unix)]
#[test]
fn config_get_quotes_raw_unix_arguments_like_go() {
    use std::os::unix::ffi::OsStringExt;

    let cases = [
        (vec![0xff], r#""\xff""#),
        (b"cafe\xe9".to_vec(), r#""cafe\xe9""#),
        ("\u{0085}".as_bytes().to_vec(), r#""\u0085""#),
        ("\u{00a0}".as_bytes().to_vec(), r#""\u00a0""#),
        ("\u{200b}".as_bytes().to_vec(), r#""\u200b""#),
        ("\u{2028}".as_bytes().to_vec(), r#""\u2028""#),
        ("\u{feff}".as_bytes().to_vec(), r#""\ufeff""#),
        ("\u{fffd}".as_bytes().to_vec(), r#""�""#),
        ("😀東京".as_bytes().to_vec(), r#""😀東京""#),
        (b"quote\"slash\\\x01".to_vec(), r#""quote\"slash\\\x01""#),
        ("café".as_bytes().to_vec(), r#""café""#),
    ];

    for (bytes, expected) in cases {
        let arg = OsString::from_vec(bytes);
        assert_eq!(format_go_quoted(&arg), expected);
    }

    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "default_profile = \"personal\"\n").unwrap();
    for bytes in [
        vec![0xff],
        b"cafe\xe9".to_vec(),
        b"quote\"slash\\\x01".to_vec(),
    ] {
        let key = OsString::from_vec(bytes);
        let quoted = format_go_quoted(&key);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_config_get_with_path(&path, &[key], &mut stdout, &mut stderr);
        assert_eq!(code, crate::exit::USAGE);
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            format!(
                "symbrain config get: key {quoted} is not set in {}\n",
                path.display()
            )
        );
    }

    let extra = OsString::from_vec(b"extra\xff".to_vec());
    let quoted = format_go_quoted(&extra);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_config_get_with_path(
        &path,
        &[OsString::from("default_profile"), extra],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::USAGE);
    assert!(stdout.is_empty());
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        format!("symbrain config get: unexpected argument {quoted}\n")
    );
}

#[cfg(unix)]
#[test]
fn invalid_utf8_key_cannot_match_replacement_character_key() {
    use std::os::unix::ffi::OsStringExt;

    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "\"\\uFFFD\" = \"replacement\"\n").unwrap();

    let key = OsString::from_vec(vec![0xff]);
    let quoted = format_go_quoted(&key);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_config_get_with_path(&path, &[key], &mut stdout, &mut stderr);

    assert_eq!(code, crate::exit::USAGE);
    assert!(stdout.is_empty());
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        format!(
            "symbrain config get: key {quoted} is not set in {}\n",
            path.display()
        )
    );
}
