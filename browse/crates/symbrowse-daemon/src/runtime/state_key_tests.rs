//! Private provider tests never inspect the operator's vault or keychain.
use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use symbrowse_core::{key_resolver::ProbeError, state::State};

pub(crate) struct AbsentSources;
impl KeySources for AbsentSources {
    fn vault(&self, _: &str) -> Result<Option<Vec<u8>>, ProbeError> {
        Ok(None)
    }
    fn keychain(&self, _: &str, _: &str) -> Result<Option<Vec<u8>>, ProbeError> {
        Ok(None)
    }
    fn environment(&self, _: &str) -> Option<String> {
        None
    }
}
struct Sources {
    calls: Arc<AtomicUsize>,
    denied: bool,
}
impl KeySources for Sources {
    fn vault(&self, entry: &str) -> Result<Option<Vec<u8>>, ProbeError> {
        assert_eq!(entry, "symbrowse/encryption-key");
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.denied {
            Err(ProbeError::Failed("exit status 4".into()))
        } else {
            Ok(Some(
                format!("{{\"value\":\"{}\"}}", "ab".repeat(32)).into_bytes(),
            ))
        }
    }
    fn keychain(&self, _: &str, _: &str) -> Result<Option<Vec<u8>>, ProbeError> {
        panic!("vault must take precedence")
    }
    fn environment(&self, _: &str) -> Option<String> {
        panic!("vault must take precedence")
    }
}
fn spec(name: &str) -> SessionSpec {
    let mut spec = SessionSpec::for_session(name);
    let root = std::env::temp_dir().join(format!("key-bridge-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    spec.state_dir = root.join("state");
    spec.cache_dir = root.join("cache");
    spec.engine = "static".into();
    spec
}
fn fixtures() -> (PathBuf, serde_json::Value) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/port/state");
    let manifest = serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
    (root, manifest)
}
#[test]
fn denied_provider_fails_before_creating_store() {
    let spec = spec("denied");
    let calls = Arc::new(AtomicUsize::new(0));
    let error = match initialize_with_sources(
        &spec,
        Sources {
            calls: calls.clone(),
            denied: true,
        },
    ) {
        Ok(_) => panic!("denied provider admitted runtime"),
        Err(error) => error,
    };
    assert_eq!(
        error.message,
        "resolve state encryption key: symvault entry \"symbrowse/encryption-key\": exit status 4"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!spec.state_store_dir().exists());
}
#[test]
fn shared_runtime_decrypts_all_versions_and_encrypts_fresh_capture_once() {
    let spec = spec("shared");
    let calls = Arc::new(AtomicUsize::new(0));
    let store = initialize_with_sources(
        &spec,
        Sources {
            calls: calls.clone(),
            denied: false,
        },
    )
    .unwrap();
    let (root, manifest) = fixtures();
    for fixture in manifest["cases"].as_array().unwrap() {
        let name = fixture["name"].as_str().unwrap();
        let raw = fs::read(root.join(fixture["path"].as_str().unwrap())).unwrap();
        let path = store.dir().join(format!("{name}.json"));
        fs::write(&path, &raw).unwrap();
        for _ in 0..3 {
            assert_eq!(store.load(name).unwrap().name, name);
            assert_eq!(store.metadata(name).unwrap().name, name);
        }
        assert_eq!(fs::read(path).unwrap(), raw);
    }
    let runtime = DispatchRuntime::new_with_store(
        spec.clone(),
        "https://web.archive.org/cdx/search/cdx",
        store,
    )
    .unwrap();
    let mut capture: State =
        serde_json::from_value(manifest["cases"][0]["expected"].clone()).unwrap();
    capture.name = "fresh-capture".into();
    capture.key_source.clear();
    runtime
        .state_store
        .save_at(&mut capture, time::OffsetDateTime::now_utc())
        .unwrap();
    assert_eq!(capture.key_source, "symvault");
    let raw = fs::read(runtime.state_store.dir().join("fresh-capture.json")).unwrap();
    assert!(symbrowse_core::state::decode(&raw, None).is_err());
    assert_eq!(runtime.state_store.load("fresh-capture").unwrap(), capture);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    fs::remove_dir_all(spec.state_dir.parent().unwrap()).unwrap();
}
#[test]
fn same_name_warning_metadata_does_not_change_the_loaded_snapshot_guard() {
    let spec = spec("overwrite");
    let calls = Arc::new(AtomicUsize::new(0));
    let encrypted = initialize_with_sources(
        &spec,
        Sources {
            calls,
            denied: false,
        },
    )
    .unwrap();
    let (_, manifest) = fixtures();
    let mut snapshot: State =
        serde_json::from_value(manifest["cases"][0]["expected"].clone()).unwrap();
    snapshot.name = "same-name".into();
    snapshot.key_source.clear();
    encrypted
        .save_at(&mut snapshot, time::OffsetDateTime::now_utc())
        .unwrap();
    let plain = initialize_with_sources(&spec, AbsentSources).unwrap();
    assert_eq!(
        plain.existing_encrypted_key_source("same-name").as_deref(),
        Some("symvault")
    );
    let original = fs::read(plain.dir().join("same-name.json")).unwrap();
    assert!(
        plain
            .save_at(&mut snapshot.clone(), time::OffsetDateTime::now_utc())
            .is_err()
    );
    assert_eq!(
        fs::read(plain.dir().join("same-name.json")).unwrap(),
        original
    );
    snapshot.key_source.clear();
    plain
        .save_at(&mut snapshot, time::OffsetDateTime::now_utc())
        .unwrap();
    assert_eq!(plain.load("same-name").unwrap().key_source, "none");
    fs::remove_dir_all(spec.state_dir.parent().unwrap()).unwrap();
}
