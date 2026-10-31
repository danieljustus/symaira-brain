include!("opencode_wal.rs");

// Only private copies are opened by SQLite: source WAL/SHM must never be created or checkpointed.
const OPENCODE_DB_ERROR: &str =
    "OpenCode database unreadable or ambiguous; set OPENCODE_GO_API_KEY explicitly";

#[cfg(unix)]
fn opencode_directory(dir: &Path) -> std::io::Result<std::fs::File> {
    use rustix::fs::{Mode, OFlags, open, openat};
    let flags =
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let absolute = std::path::absolute(dir)?;
    let mut root = open("/", flags, Mode::empty())?;
    for part in absolute.components() {
        match part {
            std::path::Component::RootDir => (),
            std::path::Component::Normal(part) => {
                root = openat(&root, part, flags, Mode::empty())?;
            }
            _ => return Err(std::io::Error::other(OPENCODE_DB_ERROR)),
        }
    }
    Ok(root.into())
}

#[cfg(unix)]
fn opencode_database_key(dir: &Path, name: &str) -> Result<(Option<String>, bool), &'static str> {
    with_opencode_database(dir, name, |db| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        opencode_database_sql(db, move || std::time::Instant::now() >= deadline)
    })
}

#[cfg(unix)]
fn with_opencode_database<T>(
    dir: &Path,
    name: &str,
    consume: impl FnOnce(&rusqlite::Connection) -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    use rustix::fs::{Mode, OFlags, openat};
    let root = opencode_directory(dir).map_err(|_| OPENCODE_DB_ERROR)?;
    let read = |name: &str, limit: u64| -> Result<Option<Vec<u8>>, &'static str> {
        let fd = match openat(
            &root,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(_) => return Err(OPENCODE_DB_ERROR),
        };
        let file = std::fs::File::from(fd);
        let metadata = file.metadata().map_err(|_| OPENCODE_DB_ERROR)?;
        if !metadata.is_file() || metadata.len() > limit {
            return Err(OPENCODE_DB_ERROR);
        }
        let mut data = Vec::new();
        file.take(limit + 1)
            .read_to_end(&mut data)
            .map_err(|_| OPENCODE_DB_ERROR)?;
        if data.len() as u64 > limit {
            return Err(OPENCODE_DB_ERROR);
        }
        Ok(Some(data))
    };
    let names = [
        name.to_owned(),
        format!("{name}-wal"),
        format!("{name}-shm"),
        format!("{name}-journal"),
    ];
    let limits = [64 << 20, 16 << 20, 1 << 20, 0];
    let mut inputs = Vec::new();
    for (name, limit) in names.iter().zip(limits) {
        inputs.push(read(name, limit)?);
    }
    if inputs[0].as_ref().is_none_or(Vec::is_empty)
        || inputs[3].is_some()
        || !opencode_valid_wal(
            inputs[0].as_deref().unwrap_or_default(),
            inputs[1].as_deref().unwrap_or_default(),
        )
    {
        return Err(OPENCODE_DB_ERROR);
    }
    for (index, (name, limit)) in names.iter().zip(limits).enumerate() {
        if read(name, limit)? != inputs[index] {
            return Err(OPENCODE_DB_ERROR);
        }
    }
    let tmp = tempfile::tempdir().map_err(|_| OPENCODE_DB_ERROR)?;
    for index in 0..2 {
        if let Some(data) = &inputs[index] {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(tmp.path().join(&names[index]))
                .map_err(|_| OPENCODE_DB_ERROR)?;
            file.write_all(data).map_err(|_| OPENCODE_DB_ERROR)?;
        }
    }
    let db = rusqlite::Connection::open_with_flags(
        tmp.path().join(name),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|_| OPENCODE_DB_ERROR)?;
    // SQL-only budget: admission, regular-file copying and connection open are not timed.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let result = consume(&db);
    if std::time::Instant::now() >= deadline {
        return Err(OPENCODE_DB_ERROR);
    }
    result
}

#[cfg(not(unix))]
fn opencode_database_key(_: &Path, _: &str) -> Result<(Option<String>, bool), &'static str> {
    Err(OPENCODE_DB_ERROR)
}

#[cfg(unix)]
fn opencode_credential_row(db: &rusqlite::Connection) -> Result<(Option<String>, bool), ()> {
    if !opencode_table(db, "migration")? {
        return Err(());
    }
    opencode_schema(
        db,
        "migration",
        &[("id", "TEXT"), ("time_completed", "INTEGER")],
    )?;
    let imported: i64 = db
        .query_row(
            "SELECT count(*) FROM migration WHERE id='20260805200742_import_legacy_credentials'",
            [],
            |r| r.get(0),
        )
        .map_err(|_| ())?;
    if imported > 1 {
        return Err(());
    }
    if !opencode_table(db, "credential")? {
        return if imported == 0 {
            Ok((None, false))
        } else {
            Err(())
        };
    }
    opencode_schema(
        db,
        "credential",
        &[
            ("id", "TEXT"),
            ("integration_id", "TEXT"),
            ("value", "TEXT"),
            ("active", "INTEGER"),
            ("time_updated", "INTEGER"),
        ],
    )?;
    let mut statement = db.prepare("SELECT substr(id,1,1025), active, time_updated, substr(value,1,65537), typeof(value) FROM credential WHERE integration_id='opencode-go' ORDER BY active DESC, time_updated DESC, id DESC LIMIT 257").map_err(|_| ())?;
    let mut rows = statement.query([]).map_err(|_| ())?;
    // The upstream order identifies the current account, not the first usable key.
    let mut selected: Option<String> = None;
    let mut count = 0;
    while let Some(row) = rows.next().map_err(|_| ())? {
        let id: String = row.get(0).map_err(|_| ())?;
        let active: Option<i64> = row.get(1).map_err(|_| ())?;
        let updated: i64 = row.get(2).map_err(|_| ())?;
        let value: String = row.get(3).map_err(|_| ())?;
        let kind: String = row.get(4).map_err(|_| ())?;
        count += 1;
        if count > 256
            || id.is_empty()
            || id.len() > 1024
            || updated < 0
            || !matches!(active, None | Some(0 | 1))
            || kind != "text"
            || value.len() > 65536
        {
            return Err(());
        }
        if active == Some(0) {
            continue;
        }
        if selected.is_none() {
            selected = Some(value);
        }
    }
    if imported == 0 {
        let total: i64 = db
            .query_row("SELECT count(*) FROM credential", [], |r| r.get(0))
            .map_err(|_| ())?;
        return if total == 0 {
            Ok((None, false))
        } else {
            Err(())
        };
    }
    let Some(value) = selected else {
        return Ok((None, true));
    };
    let value: serde_json::Value = serde_json::from_str(&value).map_err(|_| ())?;
    let value = value.as_object().ok_or(())?;
    let Some(key) = value.get("key") else {
        return Ok((None, true));
    };
    let key = key.as_str().ok_or(())?.trim();
    if key.is_empty() {
        return Ok((None, true));
    }
    if !valid_opencode_key(key) {
        return Err(());
    }
    Ok((Some(key.into()), true))
}

#[cfg(unix)]
fn opencode_table(db: &rusqlite::Connection, name: &str) -> Result<bool, ()> {
    let (count, valid): (i64, i64) = db
        .query_row(
            "SELECT count(*), coalesce(sum(type='table'),0) FROM sqlite_schema WHERE name=?",
            [name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| ())?;
    if count != valid {
        return Err(());
    }
    Ok(count == 1)
}

#[cfg(unix)]
fn opencode_schema(
    db: &rusqlite::Connection,
    name: &str,
    columns: &[(&str, &str)],
) -> Result<(), ()> {
    for (column, kind) in columns {
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM pragma_table_info(?) WHERE name=? AND upper(type)=? AND (name!='id' OR pk=1)",
                [name, *column, *kind],
                |r| r.get(0),
            )
            .map_err(|_| ())?;
        if count != 1 {
            return Err(());
        }
    }
    Ok(())
}

#[cfg(unix)]
fn opencode_sql_budget(
    db: &rusqlite::Connection,
    cancelled: impl FnMut() -> bool + Send + 'static,
) -> rusqlite::Result<()> {
    db.progress_handler(1000, Some(cancelled))
}

#[cfg(all(test, unix))]
mod opencode_sql_tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn interrupted(error: &rusqlite::Error) {
        assert!(
            matches!(error, rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::OperationInterrupted)
        );
    }

    #[test]
    fn opencode_sql_cancellation_interrupts_executing_vm() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        opencode_sql_budget(&db, move || observed.fetch_add(1, Ordering::SeqCst) >= 2).unwrap();
        let result = db.query_row("WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<100000) SELECT sum(x) FROM n", [], |r| r.get::<_,i64>(0));
        interrupted(&result.unwrap_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn opencode_sql_cancelled_discovery_fails_closed() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/opencode-v2-synthetic.json"
        ))
        .unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(fixture["schema"].as_str().unwrap())
            .unwrap();
        db.execute_batch("INSERT INTO migration VALUES('20260805200742_import_legacy_credentials',1); CREATE TABLE padding(value TEXT); WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<10000) INSERT INTO padding SELECT printf('%0100d',x) FROM n;").unwrap();
        assert_eq!(opencode_database_sql(&db, || false), Ok((None, true)));
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        assert_eq!(
            opencode_database_sql(&db, move || {
                observed.fetch_add(1, Ordering::SeqCst);
                true
            }),
            Err(OPENCODE_DB_ERROR)
        );
        assert!(calls.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn opencode_sql_deadline_interrupts_integrity_and_ordered_scan() {
        for query in [
            "PRAGMA quick_check(1)",
            "SELECT value FROM padding ORDER BY value DESC LIMIT 257",
        ] {
            let db = rusqlite::Connection::open_in_memory().unwrap();
            db.execute_batch("CREATE TABLE padding(value TEXT); WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<10000) INSERT INTO padding SELECT printf('%0100d',x) FROM n").unwrap();
            let deadline = std::time::Instant::now();
            opencode_sql_budget(&db, move || std::time::Instant::now() >= deadline).unwrap();
            interrupted(
                &db.query_row(query, [], |r| r.get::<_, String>(0))
                    .unwrap_err(),
            );
        }
    }
}

#[cfg(unix)]
fn opencode_database_sql(
    db: &rusqlite::Connection,
    cancelled: impl FnMut() -> bool + Send + 'static,
) -> Result<(Option<String>, bool), &'static str> {
    opencode_sql_budget(db, cancelled).map_err(|_| OPENCODE_DB_ERROR)?;
    db.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")
        .map_err(|_| OPENCODE_DB_ERROR)?;
    let integrity: String = db
        .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
        .map_err(|_| OPENCODE_DB_ERROR)?;
    if integrity != "ok" {
        return Err(OPENCODE_DB_ERROR);
    }
    opencode_credential_row(db).map_err(|()| OPENCODE_DB_ERROR)
}
