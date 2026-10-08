//! Publication ordering and source provenance are persistent repair contracts.
use symbrain_managed::{SourceOrigin, install_source, read_provenance};

#[test]
fn source_publication_identifies_installed_bytes_and_retains_binary_on_sidecar_failure() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("managed");
    let origin = SourceOrigin {
        receiver_commit: "receiver123",
        module_dir: "browse",
        builder: b"native <tool> & identity",
    };
    install_source(&bin, "symbrowse", b"old source payload", origin).unwrap();
    let first = read_provenance(&bin, "symbrowse").unwrap().unwrap();
    assert_eq!(first.source, "brain-source");
    assert_eq!(first.receiver_commit, "receiver123");
    let record = std::fs::read_to_string(bin.join("symbrowse.provenance.json")).unwrap();
    assert!(record.contains("\\u003ctool\\u003e \\u0026"));
    let value: serde_json::Value = serde_json::from_str(&record).unwrap();
    assert_eq!(value["version"], "");
    assert_eq!(value["builder"], "native <tool> & identity");
    assert_eq!(
        value["binary_sha256"],
        symbrain_managed::sha256_file(&bin.join("symbrowse")).unwrap()
    );
    std::fs::remove_file(bin.join("symbrowse.provenance.json")).unwrap();
    std::fs::create_dir(bin.join("symbrowse.provenance.json")).unwrap();
    let failure = install_source(&bin, "symbrowse", b"new source payload", origin).unwrap_err();
    assert!(
        failure
            .to_string()
            .contains("record provenance for symbrowse")
    );
    assert_eq!(
        std::fs::read(bin.join("symbrowse")).unwrap(),
        b"new source payload"
    );
    assert!(bin.join("symbrowse.provenance.json").is_dir());
    assert_eq!(
        std::fs::read_dir(&bin).unwrap().count(),
        2,
        "atomic temporary files must be removed on failure"
    );
}
