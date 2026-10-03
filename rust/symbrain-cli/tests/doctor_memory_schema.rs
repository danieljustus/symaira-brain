//! Doctor must distinguish missing DDL from integrity and migration bookkeeping.
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

#[path = "../../test-support/coverage.rs"]
mod coverage;

fn doctor(root: &Path, json: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .envs(coverage::profile_environment())
        .env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("home"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("PATH", root.join("absent-bin"))
        .env("SYMBRAIN_GO_BINARY", root.join("absent-go"))
        .current_dir(root)
        .arg("doctor");
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    if json {
        command.arg("--json");
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    output
}

#[test]
fn applied_migrations_with_missing_columns_are_reported_without_modification() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("data/symbrain/memory/default.db");
    let memory_id = {
        let store = symbrain_memory::Store::open(&path).unwrap();
        store
            .set(
                "retained Unicode 世界",
                "global",
                "user",
                serde_json::Map::new(),
                false,
            )
            .unwrap()
            .id
    };
    let missing = [
        "consolidated_into_id",
        "embedding_binary",
        "embedding_dim",
        "embedding_quantization",
        "lsh_hash",
    ];
    let count = {
        let connection = rusqlite::Connection::open(&path).unwrap();
        let indexes = connection
            .prepare(
                "SELECT name,sql FROM sqlite_schema WHERE type='index' AND tbl_name='memories'",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for (name, sql) in indexes {
            if sql.is_some_and(|sql| missing.iter().any(|column| sql.contains(column))) {
                connection
                    .execute_batch(&format!("DROP INDEX \"{}\"", name.replace('"', "\"\"")))
                    .unwrap();
            }
        }
        for column in missing {
            connection
                .execute_batch(&format!("ALTER TABLE memories DROP COLUMN {column}"))
                .unwrap();
        }
        connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let before = fs::read(&path).unwrap();
    let output = doctor(root.path(), true);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["memory_db"]["quick_check"], "ok");
    let diagnostic = format!(
        "missing required columns: {}",
        missing
            .map(|column| format!("memories.{column}"))
            .join(", ")
    );
    assert_eq!(report["memory_db"]["error"], diagnostic);
    assert_eq!(fs::read(&path).unwrap(), before);
    let output = doctor(root.path(), false);
    let text = String::from_utf8(output.stdout).unwrap();
    let line = text
        .lines()
        .find(|line| line.contains("memory db"))
        .unwrap();
    assert!(line.contains('✗') && line.contains(&diagnostic), "{line}");
    assert_eq!(fs::read(&path).unwrap(), before);

    // The separately reviewed open/repair path remains idempotent and retains data.
    for _ in 0..2 {
        let store = symbrain_memory::Store::open(&path).unwrap();
        assert_eq!(
            store.get(&memory_id).unwrap().unwrap().content,
            "retained Unicode 世界"
        );
    }
    let output = doctor(root.path(), true);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["memory_db"].get("error").is_none(), "{report}");
    let connection = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        count
    );
}

#[test]
fn same_column_view_cannot_replace_a_required_writable_table() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("data/symbrain/memory/default.db");
    drop(symbrain_memory::Store::open(&path).unwrap());
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "DROP TABLE sessions; CREATE VIEW sessions AS SELECT '' AS id, '' AS summary, '' AS updated_at;",
        )
        .unwrap();
    }
    let before = fs::read(&path).unwrap();
    let output = doctor(root.path(), true);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["memory_db"]["quick_check"], "ok");
    assert_eq!(
        report["memory_db"]["error"],
        "missing required columns: sessions.id, sessions.summary, sessions.updated_at"
    );
    let output = doctor(root.path(), false);
    let text = String::from_utf8(output.stdout).unwrap();
    let line = text
        .lines()
        .find(|line| line.contains("memory db"))
        .unwrap();
    assert!(line.contains('✗') && line.contains("sessions.id"), "{line}");
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[cfg(windows)]
#[test]
fn windows_directory_attributes_are_reported_without_a_private_acl_claim() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("data/symbrain/memory/default.db");
    fs::create_dir_all(&path).unwrap();
    for (readonly, expected) in [(false, "0777"), (true, "0555")] {
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(readonly);
        fs::set_permissions(&path, permissions).unwrap();
        let output = doctor(root.path(), true);
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["memory_db"]["mode"], expected);
        assert_eq!(report["memory_db"]["mode_ok"], false);
        assert!(report["memory_db"].get("error").is_some());
    }
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).unwrap();
}
