use std::fs;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use symbrain_activity::{
    ActivityError, ReadItem, SearchOptions, SearchPage, bound_items, validate_search_options,
};
use symbrain_patterns::{Episode, Pattern, Store, name, promote};
use tempfile::tempdir;

#[derive(Deserialize)]
struct Suite {
    patterns: PatternCase,
    activity: ActivityCase,
}

#[derive(Deserialize)]
struct PatternCase {
    episodes: Vec<Episode>,
    threshold: usize,
    patterns: Vec<Pattern>,
    names: Vec<String>,
    store_line: String,
}

#[derive(Deserialize)]
struct ActivityCase {
    validation: Vec<ValidationCase>,
    items: Vec<ReadItem>,
    page: SearchPage,
}

#[derive(Deserialize)]
struct ValidationCase {
    id: String,
    query: String,
    from: String,
    to: String,
    limit: usize,
    max_tokens: usize,
    #[serde(default)]
    error: String,
}

fn suite() -> Suite {
    serde_json::from_slice(include_bytes!("fixtures/oracle_expectations.json"))
        .expect("parse patterns/activity oracle")
}

#[test]
fn pattern_names_promotion_and_store_bytes_match_go() {
    let case = suite().patterns;
    assert_eq!(promote(&case.episodes, case.threshold), case.patterns);
    assert_eq!(
        name(&case.episodes[0].profile, &case.episodes[0].steps),
        case.names[0]
    );
    assert_eq!(
        name(&case.episodes[3].profile, &case.episodes[3].steps),
        case.names[1]
    );

    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("patterns.jsonl");
    Store::new(&path)
        .append(&case.episodes[0])
        .expect("append episode");
    assert_eq!(
        fs::read_to_string(path).expect("read store"),
        case.store_line
    );
}

#[test]
fn activity_validation_messages_match_go() {
    for case in suite().activity.validation {
        let options = SearchOptions {
            query: case.query,
            source: String::new(),
            from: case.from.parse::<DateTime<Utc>>().expect("from"),
            to: case.to.parse::<DateTime<Utc>>().expect("to"),
            limit: case.limit,
            max_tokens: case.max_tokens,
            include_episodes: false,
        };
        match validate_search_options(&options) {
            Ok(()) => assert!(
                case.error.is_empty(),
                "case {} expected {}",
                case.id,
                case.error
            ),
            Err(error) => assert_eq!(error.to_string(), case.error, "case {}", case.id),
        }
    }
}

#[test]
fn activity_order_and_token_budget_match_go_store_search() {
    let case = suite().activity;
    assert_eq!(bound_items(case.items, 2, 5), case.page);
}

#[test]
fn activity_error_type_remains_stable() {
    assert_eq!(
        ActivityError::InvalidTokenBudget.to_string(),
        "activity query max_tokens must be between 1 and 4000"
    );
}
