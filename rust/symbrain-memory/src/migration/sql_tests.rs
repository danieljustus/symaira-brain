//! The owned SQL reader must not confuse comments/literals with DDL effects.

use super::{catalog, sql};

#[test]
fn every_owned_resource_has_only_explicit_supported_actions() {
    for (version, resource) in catalog::STEPS {
        let statements = sql::statements(resource).unwrap();
        assert!(!statements.is_empty(), "{version}");
        for statement in statements {
            assert!(
                [
                    "CREATE TABLE",
                    "CREATE VIRTUAL TABLE",
                    "CREATE INDEX",
                    "CREATE UNIQUE INDEX",
                    "CREATE TRIGGER",
                    "ALTER TABLE",
                    "UPDATE rules",
                    "UPDATE entity_relations",
                    "DROP TRIGGER",
                    "DROP TABLE",
                    "INSERT INTO memories_fts"
                ]
                .iter()
                .any(|prefix| statement.starts_with(prefix)),
                "{version}: {statement}"
            );
        }
    }
}

#[test]
fn object_names_support_both_create_and_drop_qualifiers() {
    for (sql_text, kind, expected) in [
        (
            "CREATE TABLE IF NOT EXISTS rules (id TEXT);",
            "TABLE",
            "rules",
        ),
        (
            "CREATE UNIQUE INDEX idx_id ON relations(id);",
            "INDEX",
            "idx_id",
        ),
        (
            "DROP TRIGGER IF EXISTS memories_ai;",
            "TRIGGER",
            "memories_ai",
        ),
    ] {
        assert_eq!(sql::object_name(sql_text, kind), Some(expected));
    }
}

#[test]
fn check_reader_excludes_mentions_inside_literals_comments_and_identifiers() {
    let definition = "CREATE TABLE example(\"CHECK\" TEXT DEFAULT 'CHECK (x > 0)', x INTEGER /* CHECK (x < 0) */ CHECK (x >= 0 AND x <= 1), y TEXT CHECK(y IN ('CHECK (a)', 'second')), z TEXT -- CHECK (z)\n);";
    let expected = [
        "(x>=0andx<=1)".to_owned(),
        "(yin('CHECK (a)','second'))".to_owned(),
    ]
    .into_iter()
    .collect();
    assert_eq!(sql::checks(definition), expected);
    assert_ne!(
        sql::canonical("WHEN marker='TRUE'"),
        sql::canonical("WHEN marker='true'")
    );
}
