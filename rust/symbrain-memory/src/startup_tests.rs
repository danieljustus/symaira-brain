//! Real private constructor checks; no environment/operator secret resolution.
use std::{fs, path::PathBuf};

use crate::{MemoryRuntime, SecretOptions};

struct Owned(PathBuf);
impl Owned {
    fn new() -> Self {
        let mut entropy = [0; 16];
        getrandom::fill(&mut entropy).unwrap();
        let name: String = entropy.iter().map(|byte| format!("{byte:02x}")).collect();
        let path = std::env::temp_dir().join(format!("memory-startup-test-{name}"));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn owns_private_database_pragmas_and_unique_primary_secrets() {
    let root = Owned::new();
    let mut generated = Vec::new();
    for index in 0..2 {
        let db = root.0.join(format!("db-{index}/default.db"));
        let secret = root.0.join(format!("config-{index}/jwt.secret"));
        let mut warnings = Vec::new();
        let runtime = MemoryRuntime::open(
            &db,
            || {
                Ok(SecretOptions {
                    primary: Vec::new(),
                    path: Ok(secret.clone()),
                })
            },
            &mut warnings,
        )
        .unwrap();
        assert!(warnings.is_empty());
        let bytes = fs::read(&secret).unwrap();
        assert_eq!(bytes.len(), 65);
        assert_eq!(bytes[64], b'\n');
        assert!(
            bytes[..64]
                .iter()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        );
        generated.push(bytes);
        let store = runtime.store();
        let connection = store.lock().unwrap();
        for (pragma, expected) in [
            ("foreign_keys", 1),
            ("busy_timeout", 5_000),
            ("secure_delete", 1),
        ] {
            let actual: i64 = connection
                .query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))
                .unwrap();
            assert_eq!(actual, expected);
        }
        let journal: String = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal, "wal");
        assert!(
            connection
                .execute(
                    "INSERT INTO entities_aliases(entity_id,alias) VALUES ('absent','owned')",
                    []
                )
                .is_err()
        );
        drop(connection);
        drop(store);
        drop(runtime);
        assert!(db.with_file_name("default.db-wal").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [
                &db,
                &secret,
                &db.with_file_name("default.db-wal"),
                &db.with_file_name("default.db-shm"),
            ] {
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
            assert_eq!(
                fs::metadata(secret.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }
    assert_ne!(generated[0], generated[1]);
}

#[test]
fn database_failure_precedes_secret_resolution_without_writes() {
    let root = Owned::new();
    let parent = root.0.join("blocked");
    fs::write(&parent, b"owned original").unwrap();
    let called = std::cell::Cell::new(false);
    let error = MemoryRuntime::open(
        &parent.join("db"),
        || {
            called.set(true);
            unreachable!("secret access before database failure")
        },
        &mut Vec::new(),
    );
    assert!(matches!(error, Err(crate::StartupError::Database(_))));
    assert!(!called.get());
    assert_eq!(fs::read(parent).unwrap(), b"owned original");
}
