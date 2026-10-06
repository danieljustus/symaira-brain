//! Native Windows provenance reads preserve Go's read-only directory handles.
#![cfg(windows)]

use symbrain_managed::{format_io_error, read_provenance};

#[test]
fn directory_read_reports_actual_read_failure_and_retains_sidecar() {
    let root = tempfile::tempdir().unwrap();
    let sidecar = root.path().join("symdesk.provenance.json");
    std::fs::create_dir(&sidecar).unwrap();
    let error = read_provenance(root.path(), "symdesk").unwrap_err();
    // ERROR_INVALID_FUNCTION is the real Windows ReadFile error for this
    // successfully opened directory, including the OS-localized diagnostic.
    let expected = format_io_error(&std::io::Error::from_raw_os_error(1));
    assert_eq!(
        error,
        format!(
            "managed: read provenance: read {}: {expected}",
            sidecar.display()
        )
    );
    assert!(sidecar.is_dir());
    assert_eq!(std::fs::read_dir(&sidecar).unwrap().count(), 0);
}

#[test]
fn ordinary_and_absent_records_keep_source_ownership() {
    let root = tempfile::tempdir().unwrap();
    assert!(read_provenance(root.path(), "symdesk").unwrap().is_none());
    let sidecar = root.path().join("symdesk.provenance.json");
    let bytes = br#"{"source":"brain-source","receiver_commit":"retained"}"#;
    std::fs::write(&sidecar, bytes).unwrap();
    let record = read_provenance(root.path(), "symdesk").unwrap().unwrap();
    assert_eq!(record.source, "brain-source");
    assert_eq!(record.receiver_commit, "retained");
    assert_eq!(std::fs::read(sidecar).unwrap(), bytes);
}
