//! Known FTS owners gain porter/rebuild effects; unknown owners are retained.

use super::{configure, test_support::*};
use rusqlite::Connection;

#[test]
fn old_unicode61_and_missing_fts_gain_existing_row_search_and_maintenance() {
    for end in [13, 14, 15] {
        let conn = Connection::open_in_memory().unwrap();
        prefix(&conn, end, true);
        let store = configure(conn).unwrap();
        let conn = store.lock().unwrap();
        assert_eq!(
            scalar(
                &conn,
                "SELECT count(*) FROM memories_fts WHERE memories_fts MATCH 'run'"
            ),
            1
        );
        conn.execute(
            "UPDATE memories SET content='jumping slowly' WHERE id='memory'",
            [],
        )
        .unwrap();
        assert_eq!(
            scalar(
                &conn,
                "SELECT count(*) FROM memories_fts WHERE memories_fts MATCH 'run'"
            ),
            0
        );
        assert_eq!(
            scalar(
                &conn,
                "SELECT count(*) FROM memories_fts WHERE memories_fts MATCH 'jump'"
            ),
            1
        );
        conn.execute("DELETE FROM memories WHERE id='memory'", [])
            .unwrap();
        assert_eq!(
            scalar(
                &conn,
                "SELECT count(*) FROM memories_fts WHERE memories_fts MATCH 'jump'"
            ),
            0
        );
        conn.execute(
            "INSERT INTO memories_fts(memories_fts,rank) VALUES('integrity-check',1)",
            [],
        )
        .unwrap();
    }
}

#[test]
fn current_porter_missing_old_rows_is_rebuilt_despite_claimed_marker() {
    let conn = Connection::open_in_memory().unwrap();
    prefix(&conn, 36, true);
    conn.execute(
        "INSERT INTO memories_fts(memories_fts) VALUES('delete-all')",
        [],
    )
    .unwrap();
    assert_eq!(
        scalar(
            &conn,
            "SELECT count(*) FROM memories_fts WHERE memories_fts MATCH 'run'"
        ),
        0
    );
    let store = configure(conn).unwrap();
    let conn = store.lock().unwrap();
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
}

#[test]
fn unknown_fts_table_tokenizer_or_trigger_is_not_destructively_repaired() {
    for mutation in [
        "DROP TRIGGER memories_ai; DROP TRIGGER memories_ad; DROP TRIGGER memories_au; DROP TABLE memories_fts; CREATE TABLE memories_fts(owned_payload TEXT); INSERT INTO memories_fts VALUES ('keep me');",
        "DROP TRIGGER memories_ai; DROP TRIGGER memories_ad; DROP TRIGGER memories_au; DROP TABLE memories_fts; CREATE VIRTUAL TABLE memories_fts USING fts5(id UNINDEXED,content,scope,content=memories,content_rowid=rowid,tokenize='ascii');",
        "DROP TRIGGER memories_ai; CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN SELECT 'custom owner'; END;",
    ] {
        let database = OwnedDatabase::new();
        let before = {
            let conn = Connection::open(database.path()).unwrap();
            prefix(&conn, 36, true);
            conn.execute_batch(mutation).unwrap();
            snapshot(&conn)
        };
        assert!(crate::Store::open(database.path()).is_err());
        assert_eq!(
            snapshot(&Connection::open(database.path()).unwrap()),
            before
        );
    }
}
