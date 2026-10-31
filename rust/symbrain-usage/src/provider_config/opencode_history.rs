// Recorded hosted rows only. No credential lookup or source SQLite connection.
#[cfg(unix)]
fn opencode_history_rows(
    dir: &Path,
    cancel: &crate::Cancellation,
) -> Result<(Vec<crate::history::Event>, usize), ()> {
    use std::collections::{BTreeMap, BTreeSet};
    let deadline = Instant::now() + Duration::from_secs(2);
    if cancel.is_cancelled() {
        return Err(());
    }
    let root = match opencode_directory(dir) {
        Ok(root) => root,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((vec![], 0)),
        Err(_) => return Err(()),
    };
    let mut names = vec![];
    let mut entries = 0;
    // Read the retained directory capability, not a second path-based traversal.
    let mut directory = rustix::fs::Dir::read_from(&root).map_err(|_| ())?;
    for entry in &mut directory {
        let entry = entry.map_err(|_| ())?;
        entries += 1;
        if entries > 4098 || cancel.is_cancelled() || Instant::now() >= deadline {
            return Err(());
        }
        let name = entry.file_name().to_str().map_err(|_| ())?;
        if name.starts_with("opencode")
            && Path::new(name).extension().is_some_and(|ext| ext == "db")
        {
            names.push(name.to_owned());
        }
    }
    if names.len() > 8 {
        return Err(());
    }
    names.sort();
    let mut unique = BTreeMap::new();
    let mut conflicts = BTreeSet::new();
    let mut excluded = 0;
    let mut count = 0;
    let mut bytes = 0;
    for name in names {
        if cancel.is_cancelled() || Instant::now() >= deadline {
            return Err(());
        }
        with_opencode_database(dir, &name, |db| {
            let cancelled = cancel.clone();
            opencode_sql_budget(db, move || cancelled.is_cancelled() || Instant::now() >= deadline).map_err(|_| OPENCODE_DB_ERROR)?;
            db.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;").map_err(|_| OPENCODE_DB_ERROR)?;
            let integrity: String = db.query_row("PRAGMA quick_check(1)", [], |r| r.get(0)).map_err(|_| OPENCODE_DB_ERROR)?;
            if integrity != "ok" { return Err(OPENCODE_DB_ERROR); }
            for table in ["message", "session_message"] {
                if !opencode_table(db, table).map_err(|()| OPENCODE_DB_ERROR)? { continue; }
                let kind = if table == "message" { "''" } else { "coalesce(type,'')" };
                let sql = format!("SELECT coalesce(CASE WHEN typeof(id)='text' THEN substr(id,1,257) END,''), CASE WHEN typeof(time_created)='integer' THEN time_created ELSE 0 END, {kind}, coalesce(CASE WHEN typeof(data)='text' THEN substr(data,1,262145) END,''), typeof(id), typeof(time_created), typeof(data) FROM {table} LIMIT 10001");
                let mut statement = db.prepare(&sql).map_err(|_| OPENCODE_DB_ERROR)?;
                let mut rows = statement.query([]).map_err(|_| OPENCODE_DB_ERROR)?;
                while let Some(row) = rows.next().map_err(|_| OPENCODE_DB_ERROR)? {
                    count += 1;
                    let id: String = row.get(0).map_err(|_| OPENCODE_DB_ERROR)?;
                    let ms: i64 = row.get(1).map_err(|_| OPENCODE_DB_ERROR)?;
                    let kind: String = row.get(2).map_err(|_| OPENCODE_DB_ERROR)?;
                    let data: String = row.get(3).map_err(|_| OPENCODE_DB_ERROR)?;
                    bytes += data.len();
                    if count > 10_000 || data.len() > 262_144 || bytes > 32 << 20 || cancel.is_cancelled() || Instant::now() >= deadline { return Err(OPENCODE_DB_ERROR); }
                    let types = [row.get::<_,String>(4),row.get(5),row.get(6)];
                    if types != [Ok("text".into()),Ok("integer".into()),Ok("text".into())] { excluded += 1; if !id.is_empty() && id.len() <= 256 { conflicts.insert(id); } continue; }
                    match parse_opencode_history_row(&id, ms, &kind, &data) {
                        Ok(Some((provider,event))) => {
                            if let Some((old_provider,old)) = unique.get(&id) {
                                if old_provider != &provider || old != &event { conflicts.insert(id); }
                            } else { unique.insert(id, (provider,event)); }
                        }
                        Ok(None) => (),
                        Err(()) => { excluded += 1; if !id.is_empty() && id.len() <= 256 { conflicts.insert(id); } },
                    }
                }
            }
            Ok(())
        }).map_err(|_| ())?;
    }
    if cancel.is_cancelled() || Instant::now() >= deadline {
        return Err(());
    }
    let mut events = vec![];
    for (id, (provider, event)) in unique {
        if conflicts.contains(&id) {
            excluded += 1;
        } else if provider == "opencode" || provider == "opencode-go" {
            events.push(event);
        }
    }
    Ok((events, excluded))
}

#[cfg(not(unix))]
fn opencode_history_rows(
    _: &Path,
    _: &crate::Cancellation,
) -> Result<(Vec<crate::history::Event>, usize), ()> {
    Err(())
}

fn parse_opencode_history_row(
    id: &str,
    ms: i64,
    kind: &str,
    data: &str,
) -> Result<Option<(String, crate::history::Event)>, ()> {
    let obj: serde_json::Value = serde_json::from_str(data).map_err(|_| ())?;
    if !obj.is_object() && !obj.is_null() {
        return Err(());
    }
    let kind = if kind.is_empty() {
        obj["role"].as_str().unwrap_or("")
    } else {
        kind
    };
    if kind != "assistant" && !(kind == "compaction" && obj["status"] == "completed") {
        return Ok(None);
    }
    let nested = obj.get("model").filter(|v| !v.is_null());
    let model = nested
        .map_or(&obj["modelID"], |v| &v["id"])
        .as_str()
        .ok_or(())?;
    let provider = nested
        .map_or(&obj["providerID"], |v| &v["providerID"])
        .as_str()
        .ok_or(())?;
    if nested.is_some()
        && ((obj["modelID"]
            .as_str()
            .is_some_and(|m| !m.is_empty() && m != model))
            || (obj["providerID"]
                .as_str()
                .is_some_and(|p| !p.is_empty() && p != provider)))
    {
        return Err(());
    }
    if id.is_empty()
        || id.len() > 256
        || ms <= 0
        || ms >= 253_402_300_800_000
        || provider.is_empty()
        || model.len() > 128
        || model.contains("..")
        || !model
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        || !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
    {
        return Err(());
    }
    let count = |v: &serde_json::Value| v.as_u64().filter(|n| *n <= 1_000_000_000_000_000);
    let t = &obj["tokens"];
    let tokens = if t.get("total").is_some() {
        count(&t["total"])
    } else {
        [
            &t["input"],
            &t["output"],
            &t["reasoning"],
            &t["cache"]["read"],
            &t["cache"]["write"],
        ]
        .into_iter()
        .map(|v| count(v).filter(|n| *n <= 1_000_000_000_000))
        .sum::<Option<u64>>()
    };
    Ok(Some((
        provider.into(),
        crate::history::Event {
            id: id.into(),
            model: model.into(),
            at: chrono::DateTime::from_timestamp_millis(ms).ok_or(())?,
            tokens,
        },
    )))
}

pub(crate) fn scan_opencode_history<T: chrono::TimeZone>(
    dir: &Path,
    now: &chrono::DateTime<T>,
    timezone: &str,
    cancel: &crate::Cancellation,
) -> crate::history::LocalHistory {
    let mut h = crate::history::LocalHistory {
        scope: "local_machine".into(),
        attribution: "unattributed".into(),



        timezone: timezone.into(),
        status: "no_data".into(),
        excluded_events: 0,
        daily: vec![],
        periods: vec![],
    };
    if !dir.is_absolute()
        || dir
            .as_os_str()
            .as_encoded_bytes()
            .split(|b| *b == b'/')
            .any(|c| c == b"." || c == b"..")
    {
        h.status = "unavailable".into();
        return h;
    }
    match opencode_history_rows(dir, cancel) {
        Ok((events, excluded)) => {
            h.excluded_events = excluded;
            crate::history::aggregate(&mut h, events, now);
        }
        Err(()) => h.status = "unavailable".into(),
    }
    h
}

include!("opencode_history_tests.rs");
