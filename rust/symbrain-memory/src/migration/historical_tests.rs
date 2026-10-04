//! Every historical prefix must gain executable effects and retain its rows.

use super::{catalog, configure, test_support::*};
use rusqlite::Connection;

#[test]
fn all_37_historical_prefixes_complete_without_losing_owned_data() {
    for end in 0..catalog::STEPS.len() {
        let conn = Connection::open_in_memory().unwrap();
        prefix(&conn, end, true);
        let store =
            configure(conn).unwrap_or_else(|error| panic!("{}: {error}", catalog::STEPS[end].0));
        let conn = store.lock().unwrap();
        assert_eq!(scalar(&conn, "SELECT count(*) FROM schema_migrations"), 37);
        assert_eq!(
            conn.query_row(
                "SELECT content,created_at,updated_at FROM memories WHERE id='memory'",
                [],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?
                ))
            )
            .unwrap(),
            ("running quickly".into(), CREATED.into(), CREATED.into())
        );
        assert_eq!(
            conn.query_row("SELECT updated_at FROM rules WHERE id='rule'", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
            CREATED
        );
        assert_eq!(
            scalar(
                &conn,
                "SELECT count(*) FROM memories_fts WHERE memories_fts MATCH 'run'"
            ),
            1
        );
        conn.execute(
            "INSERT INTO memories_fts(memories_fts,rank) VALUES('integrity-check',1)",
            [],
        )
        .unwrap();
        assert_eq!(
            scalar(&conn, "SELECT count(*) FROM pragma_foreign_key_check"),
            0
        );
        for projection in [
            "valid_from,valid_until FROM entity_relations",
            "actor,scope,session FROM query_log",
            "target_type,target_id FROM audit_log",
        ] {
            conn.prepare(&format!("SELECT {projection}")).unwrap();
        }
        if end >= 19 {
            let rows = conn.prepare("SELECT id,updated_at,valid_from,valid_until FROM entity_relations ORDER BY from_entity_id,to_entity_id").unwrap()
                .query_map([], |row| Ok((row.get::<_, String>(0)?,row.get::<_, String>(1)?,row.get::<_, Option<String>>(2)?,row.get::<_, Option<String>>(3)?)))
                .unwrap().collect::<Result<Vec<_>, _>>().unwrap();
            assert_eq!(rows.len(), 2);
            assert_ne!(rows[0].0, rows[1].0);
            for (id, updated, from, until) in rows {
                assert!(valid_uuid4(&id), "{} relation {id}", catalog::STEPS[end].0);
                assert_eq!(updated, CREATED);
                assert_eq!((from, until), (None, None));
            }
            assert!(conn.execute("UPDATE entity_relations SET id=(SELECT id FROM entity_relations WHERE from_entity_id='a') WHERE from_entity_id='b'", []).is_err());
        }
    }
}

pub(super) fn valid_uuid4(id: &str) -> bool {
    id.len() == 36
        && [8, 13, 18, 23]
            .iter()
            .all(|index| id.as_bytes()[*index] == b'-')
        && id.as_bytes()[14] == b'4'
        && matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b')
        && id.bytes().enumerate().all(|(index, byte)| {
            [8, 13, 18, 23].contains(&index)
                || byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
        })
}

#[test]
fn historical_unconditional_sync_triggers_are_replaced_with_exclusion() {
    let conn = Connection::open_in_memory().unwrap();
    prefix(&conn, 22, true);
    let store = configure(conn).unwrap();
    let conn = store.lock().unwrap();
    let before = scalar(&conn, "SELECT count(*) FROM sync_oplog");
    conn.execute(
        "UPDATE memories SET metadata='{\"sync_exclude\":\"true\"}' WHERE id='memory'",
        [],
    )
    .unwrap();
    conn.execute("DELETE FROM memories WHERE id='memory'", [])
        .unwrap();
    assert_eq!(scalar(&conn, "SELECT count(*) FROM sync_oplog"), before);
    assert_eq!(
        scalar(
            &conn,
            "SELECT count(*) FROM memories_fts WHERE memories_fts MATCH 'run'"
        ),
        0
    );
}

#[test]
fn healthy_reopen_retains_every_schema_and_fts_shadow_cell() {
    let database = OwnedDatabase::new();
    {
        let conn = Connection::open(database.path()).unwrap();
        prefix(&conn, 36, true);
    }
    let first = {
        let store = crate::Store::open(database.path()).unwrap();
        snapshot(&store.lock().unwrap())
    };
    for _ in 0..3 {
        let store = crate::Store::open(database.path()).unwrap();
        assert_eq!(snapshot(&store.lock().unwrap()), first);
    }
}
