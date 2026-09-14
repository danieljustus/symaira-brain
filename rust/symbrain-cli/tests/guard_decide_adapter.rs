#![deny(unsafe_code)]

use std::io::{self, Read, Write};

use chrono::{DateTime, FixedOffset};
use symbrain_cli::guard_cli::run_at_path;

fn fixed_now() -> DateTime<FixedOffset> {
    "2026-09-14T12:00:00Z".parse().expect("fixed time")
}

#[derive(Debug)]
struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("injected reader failure"))
    }
}

#[derive(Debug)]
struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "injected output failure",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn production_route_bounds_input_audits_and_writes_one_response() {
    let data = tempfile::tempdir().expect("tempdir");
    let mut output = Vec::new();
    let code = run_at_path(
        br#"{"command":"open","risk_class":"low"}"#.as_slice(),
        &mut output,
        data.path().join("symguard/audit.log"),
        fixed_now(),
    );
    assert_eq!(code, 0);
    assert_eq!(
        output,
        br#"{"decision":"allow","reason":"low risk class, no warnings"}
"#
    );
    let audit = std::fs::read_to_string(data.path().join("symguard/audit.log")).expect("audit log");
    assert_eq!(audit.lines().count(), 1);
    assert!(audit.contains("\"risk_class\":\"low\""));
}

#[test]
fn production_route_reader_failure_is_one_audited_deny() {
    let data = tempfile::tempdir().expect("tempdir");
    let mut output = Vec::new();
    let code = run_at_path(
        FailingReader,
        &mut output,
        data.path().join("symguard/audit.log"),
        fixed_now(),
    );
    assert_eq!(code, 0);
    assert!(
        String::from_utf8_lossy(&output).contains("decide: read request: injected reader failure")
    );
    let audit = std::fs::read_to_string(data.path().join("symguard/audit.log")).expect("audit log");
    assert_eq!(audit.lines().count(), 1);
}

#[cfg(unix)]
#[test]
fn production_route_audit_failure_denies_and_output_failure_returns_one() {
    let data = tempfile::tempdir().expect("tempdir");
    let bad_data_home = data.path().join("not-a-directory");
    std::fs::write(&bad_data_home, b"x").expect("seed bad data home");
    let mut output = Vec::new();
    let code = run_at_path(
        br#"{"command":"open","risk_class":"low"}"#.as_slice(),
        &mut output,
        bad_data_home.join("symguard/audit.log"),
        fixed_now(),
    );
    assert_eq!(code, 0);
    assert!(String::from_utf8_lossy(&output).contains("audit: write decision record"));

    let data = tempfile::tempdir().expect("tempdir");
    let code = run_at_path(
        br#"{"command":"open","risk_class":"low"}"#.as_slice(),
        FailingWriter,
        data.path().join("symguard/audit.log"),
        fixed_now(),
    );
    assert_eq!(code, 1);
}

#[test]
fn production_route_oversize_input_is_fail_closed() {
    let data = tempfile::tempdir().expect("tempdir");
    let mut input = vec![b'x'; 64 * 1024 + 1];
    input[0] = b'{';
    let mut output = Vec::new();
    let code = run_at_path(
        input.as_slice(),
        &mut output,
        data.path().join("symguard/audit.log"),
        fixed_now(),
    );
    assert_eq!(code, 0);
    assert!(String::from_utf8_lossy(&output).contains("request exceeds maximum size"));
}
