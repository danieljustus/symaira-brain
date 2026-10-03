#![deny(unsafe_code)]
//! Destructive cleanup authenticates v3 headers before trusting timestamps.
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use symbrowse_core::{
    state::{FILE_MAGIC, encrypt},
    state_store::{KeyMaterial, Store},
};
use time::{Duration, macros::datetime};
static NEXT: AtomicU64 = AtomicU64::new(0);
fn root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "state-clean-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    root
}
fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/port/state")
            .join(format!("{name}.state")),
    )
    .unwrap()
}
fn store(root: &PathBuf) -> Store {
    Store::new(
        root,
        Duration::days(30),
        Some(KeyMaterial::new([0xab; 32], "environment").unwrap()),
    )
    .unwrap()
}
fn forged_none(raw: &[u8]) -> Vec<u8> {
    let data = raw.strip_prefix(FILE_MAGIC).unwrap();
    let split = data.iter().position(|byte| *byte == b'\n').unwrap();
    let mut header: serde_json::Value = serde_json::from_slice(&data[..split]).unwrap();
    header["key_source"] = "none".into();
    let mut forged = FILE_MAGIC.to_vec();
    forged.extend(serde_json::to_vec(&header).unwrap());
    forged.push(b'\n');
    forged.extend(&data[split + 1..]);
    forged
}
#[test]
fn keyed_plaintext_show_is_distinct_from_authenticated_destructive_cleanup() {
    let root = root("plaintext");
    let store = store(&root);
    let raw = fixture("plaintext-v3");
    fs::write(root.join("plain.json"), &raw).unwrap();
    assert!(store.metadata("plain").is_ok());
    let error = store
        .clean_at(datetime!(2026-10-01 00:00 UTC))
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "clean state \"plain\": authenticate state header: decrypt state file: cipher: message authentication failed"
    );
    assert_eq!(fs::read(root.join("plain.json")).unwrap(), raw);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn forged_selector_and_later_corruption_stop_sorted_cleanup_without_touching_later_files() {
    for attack in ["forged-none", "corrupt-body"] {
        let root = root(attack);
        let store = store(&root);
        let valid = fixture("encrypted-v3");
        let mut damaged = valid.clone();
        if attack == "forged-none" {
            damaged = forged_none(&valid);
        } else {
            *damaged.last_mut().unwrap() ^= 1;
        }
        fs::write(root.join("a-valid.json"), &valid).unwrap();
        fs::write(root.join("b-damaged.json"), &damaged).unwrap();
        fs::write(root.join("c-later.json"), &valid).unwrap();
        let error = store
            .clean_at(datetime!(2026-10-01 00:00 UTC))
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "clean state \"b-damaged\": authenticate state header: decrypt state file: cipher: message authentication failed"
        );
        assert!(!root.join("a-valid.json").exists());
        assert_eq!(fs::read(root.join("b-damaged.json")).unwrap(), damaged);
        assert_eq!(fs::read(root.join("c-later.json")).unwrap(), valid);
        fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn authenticated_header_cleanup_does_not_require_payload_json() {
    let root = root("authenticated-nonjson");
    let store = store(&root);
    let valid = fixture("encrypted-v3");
    let data = valid.strip_prefix(FILE_MAGIC).unwrap();
    let split = data.iter().position(|byte| *byte == b'\n').unwrap();
    let header = &data[..split];
    let body = encrypt(b"owned-invalid-json", header, &[0xab; 32]).unwrap();
    let mut raw = FILE_MAGIC.to_vec();
    raw.extend(header);
    raw.push(b'\n');
    raw.extend(body);
    fs::write(root.join("authenticated.json"), raw).unwrap();
    assert!(store.metadata("authenticated").is_err());
    assert_eq!(
        store.clean_at(datetime!(2026-10-01 00:00 UTC)).unwrap(),
        ["authenticated"]
    );
    fs::remove_dir_all(root).unwrap();
}
#[cfg(unix)]
#[test]
fn overwrite_warning_probe_rejects_links_and_never_blocks_on_fifo() {
    use rustix::fs::{CWD, Mode, mkfifoat};
    use std::os::unix::fs::symlink;
    let outside = root("outside");
    let root = root("warning");
    let store = store(&root);
    fs::write(outside.join("external.json"), fixture("encrypted-v3")).unwrap();
    symlink(outside.join("external.json"), root.join("linked.json")).unwrap();
    mkfifoat(
        CWD,
        root.join("pipe.json").as_path(),
        Mode::from_bits_truncate(0o600),
    )
    .unwrap();
    assert_eq!(store.existing_encrypted_key_source("linked"), None);
    assert_eq!(store.existing_encrypted_key_source("pipe"), None);
    assert_eq!(
        fs::read(outside.join("external.json")).unwrap(),
        fixture("encrypted-v3")
    );
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}
