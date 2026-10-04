use super::{CoreResult, SetupReport, finish};
use symbrain_managed::GoText;

#[test]
fn setup_json_preserves_raw_bytes_and_escapes_ordinary_fields_once() {
    let marker = "<>&\u{2028}\u{2029}";
    let raw = GoText::from(b"path\xff\xe2\x82".to_vec()).with_suffix(marker.as_bytes());
    let report = SetupReport {
        bin_dir: raw.clone(),
        results: vec![CoreResult {
            name: marker.into(),
            version: marker.into(),
            status: "error",
            error: Some(raw.clone()),
        }],
        errors: vec![raw],
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    assert_eq!(
        finish(&report, true, "symbrain setup", &mut stdout, &mut stderr),
        1
    );
    assert!(stderr.is_empty());
    let text = String::from_utf8(stdout).unwrap();
    assert!(text.ends_with('\n'));
    assert!(!text.contains(['<', '>', '&', '\u{2028}', '\u{2029}']));
    let escaped = "\\u003c\\u003e\\u0026\\u2028\\u2029";
    assert_eq!(text.matches(escaped).count(), 5);
    assert_eq!(text.matches("path\\ufffd\\ufffd\\ufffd").count(), 3);
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["results"][0]["name"], marker);
    assert_eq!(parsed["results"][0]["version"], marker);
}

#[test]
fn setup_json_write_failure_remains_checked() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "owned failure",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let report = SetupReport {
        bin_dir: "owned".into(),
        results: Vec::new(),
        errors: Vec::new(),
    };
    let mut stderr = Vec::new();
    assert_eq!(
        finish(
            &report,
            true,
            "symbrain setup --fix",
            &mut Broken,
            &mut stderr
        ),
        1
    );
    assert_eq!(stderr, b"symbrain setup --fix: encode JSON\n");
}
