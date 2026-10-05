//! Account for an exact kernel-unavailable owned fixture without claiming parity.
use std::fmt::Write as _;
use std::{fs, io, io::Write, os::unix::ffi::OsStrExt, path::Path};

pub const COMPONENT: &[u8] = b"owned-\xe2\x82<&>";

#[derive(Clone, Copy)]
pub enum Operation {
    ConfigWrite,
    AuditDirectory,
}

impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::ConfigWrite => "fs::write",
            Self::AuditDirectory => "fs::create_dir",
        }
    }
}

fn exact_unavailable(macos: bool, component: &[u8], errno: Option<i32>) -> bool {
    macos && component == COMPONENT && errno == Some(92)
}

pub fn exact_raw_kernel_failure(path: &Path, error: &io::Error) -> bool {
    exact_unavailable(
        cfg!(target_os = "macos"),
        path.file_name().unwrap().as_bytes(),
        error.raw_os_error(),
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut output, value| {
        write!(output, "{value:02x}").unwrap();
        output
    })
}

pub fn entries(parent: &Path) -> Vec<String> {
    let mut entries = fs::read_dir(parent)
        .unwrap()
        .map(|entry| hex(entry.unwrap().file_name().as_bytes()))
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

#[derive(Clone, Copy)]
pub struct Coverage<'a> {
    pub requested: &'a [&'a str],
    pub executed: &'a [&'a str],
    pub unavailable: &'a [&'a str],
}

/// Called only with the error from the immediate exact owned create operation.
pub fn record_failed_creation(
    root: tempfile::TempDir,
    path: &Path,
    operation: Operation,
    error: &io::Error,
    before: &[String],
    coverage: Coverage<'_>,
) {
    let component = path.file_name().unwrap().as_bytes();
    assert_eq!(component, COMPONENT, "unrecognized raw fixture component");
    assert!(path.starts_with(root.path()), "foreign fixture root");
    let after = entries(path.parent().unwrap());
    let allowed = exact_raw_kernel_failure(path, error);
    let owned_root = root.path().to_owned();
    let attempted_path_hex = hex(path.as_os_str().as_bytes());
    let component_hex = hex(component);
    root.close().unwrap();
    let removed = fs::symlink_metadata(&owned_root).unwrap_err().kind() == io::ErrorKind::NotFound;
    let record = serde_json::json!({
        "event": "owned-guard-rust-fixture-admission",
        "platform": std::env::consts::OS,
        "architecture": std::env::consts::ARCH,
        "operation": operation.name(),
        "component_hex": component_hex,
        "attempted_path_bytes_hex": attempted_path_hex,
        "owned_root_bytes_hex": hex(owned_root.as_os_str().as_bytes()),
        "error_debug": format!("{error:?}"),
        "error_display": error.to_string(),
        "error_kind": format!("{:?}", error.kind()),
        "errno": error.raw_os_error(),
        "entries_before_operation": &before,
        "entries_after_operation": &after,
        "owned_root_removed": removed,
        "product_children_started_for_unavailable_cases": 0,
        "requested_case_ids": coverage.requested,
        "executed_case_ids": coverage.executed,
        "unavailable_case_ids": coverage.unavailable,
        "disposition": if allowed { "UNEXECUTED-kernel-unavailable" } else { "FATAL-fixture-error" },
        "parity": false,
        "original_requested_domain_complete": false,
        "platform_admitted_complete": allowed && before == after && removed,
    });
    // Direct I/O remains visible in complete CI stderr even for a passing libtest.
    writeln!(io::stderr().lock(), "{record}").unwrap();
    assert!(allowed, "unexpected owned fixture error: {error}");
    assert_eq!(before, after, "failed fixture operation changed its parent");
    assert!(removed, "owned fixture cleanup not proved");
    assert_accounting(coverage.requested, coverage.executed, coverage.unavailable);
}

pub fn assert_accounting(requested: &[&str], executed: &[&str], unavailable: &[&str]) {
    let mut actual = executed
        .iter()
        .chain(unavailable)
        .copied()
        .collect::<Vec<_>>();
    actual.sort_unstable();
    let mut expected = requested.to_vec();
    expected.sort_unstable();
    assert!(expected.windows(2).all(|pair| pair[0] != pair[1]));
    assert!(actual.windows(2).all(|pair| pair[0] != pair[1]));
    assert_eq!(actual, expected, "unrecorded or duplicate fixture case");
}

#[test]
fn exact_kernel_domain_controls() {
    assert_eq!(Operation::ConfigWrite.name(), "fs::write");
    assert_eq!(Operation::AuditDirectory.name(), "fs::create_dir");
    assert!(exact_unavailable(true, COMPONENT, Some(92)));
    assert!(!exact_unavailable(false, COMPONENT, Some(92)));
    for errno in [None, Some(84), Some(13), Some(2)] {
        assert!(!exact_unavailable(true, COMPONENT, errno));
    }
    for component in [
        b"owned-\xef\xbf\xbd<&>".as_slice(),
        b"other-\xe2\x82<&>",
        b"owned",
    ] {
        assert!(!exact_unavailable(true, component, Some(92)));
    }
    assert_accounting(&["healthy", "raw"], &["healthy"], &["raw"]);
}

#[test]
#[should_panic(expected = "unrecorded or duplicate fixture case")]
fn omitted_case_is_rejected() {
    assert_accounting(&["healthy", "raw"], &["healthy"], &[]);
}
