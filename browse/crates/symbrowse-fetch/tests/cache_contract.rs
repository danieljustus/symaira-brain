//! FETCH-007 cache family: output-cache ids and lifetime, response-cache keys,
//! truncate-and-store output bytes and line ranges. No sealed cache fixture
//! exists (the Go oracle was removed), so these are Rust behavior tests.
use std::{path::PathBuf, time::Duration};

use serde_json::json;
use symbrowse_fetch::cache::{
    CacheError, OutputCache, ResponseCache, StoreOptions, cache_id_from_output, content_key,
    line_range, options_key, truncate_and_store,
};

fn root(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "symbrowse-cache-contract-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn output_cache_round_trips_and_ids_have_a_fixed_shape() {
    let dir = root("roundtrip");
    let cache = OutputCache::new(&dir, None);
    let first = cache.store(b"alpha").unwrap();
    let second = cache.store(b"alpha").unwrap();
    assert_ne!(first, second, "identical bytes still get distinct ids");
    for id in [&first, &second] {
        assert_eq!(id.len(), 16);
        assert!(id.starts_with("out_"));
        assert!(id[4..].bytes().all(|b| b.is_ascii_hexdigit()));
    }
    assert_eq!(cache.load(&first).unwrap(), b"alpha");
    cache.remove(&first).unwrap();
    assert!(matches!(cache.load(&first), Err(CacheError::NotFound(_))));
    assert_eq!(cache.load(&second).unwrap(), b"alpha");
    cache.clear().unwrap();
    assert!(matches!(cache.load(&second), Err(CacheError::NotFound(_))));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn output_cache_rejects_ids_that_could_escape_the_root() {
    let cache = OutputCache::new(root("traversal"), None);
    for id in [
        "",
        "../etc/passwd",
        "out_../../x",
        "out_0123456789ab/",
        "out_0123456789abc",
        "out_0123456789a",
        "OUT_0123456789ab",
        "out_0123456789xy",
    ] {
        assert!(
            matches!(cache.load(id), Err(CacheError::InvalidId(_))),
            "{id:?} must be rejected"
        );
        assert!(matches!(cache.remove(id), Err(CacheError::InvalidId(_))));
    }
}

#[test]
fn output_cache_entries_expire_and_are_removed_on_read() {
    let dir = root("ttl");
    // The TTL must outlast store + first load even on a heavily loaded host.
    let ttl = Duration::from_secs(2);
    let cache = OutputCache::new(&dir, Some(ttl));
    let id = cache.store(b"short lived").unwrap();
    assert_eq!(cache.load(&id).unwrap(), b"short lived");
    std::thread::sleep(ttl + Duration::from_millis(500));
    assert!(matches!(cache.load(&id), Err(CacheError::Expired(_))));
    assert!(matches!(cache.load(&id), Err(CacheError::NotFound(_))));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn response_cache_key_covers_every_rendering_input() {
    let base = ResponseCache::key("https://a.test/", "honest", "markdown", "s1", "mc=1");
    assert_eq!(base.len(), 64);
    assert_eq!(
        base,
        ResponseCache::key("https://a.test/", "honest", "markdown", "s1", "mc=1")
    );
    let variants = [
        ResponseCache::key("https://a.test/x", "honest", "markdown", "s1", "mc=1"),
        ResponseCache::key("https://a.test/", "chrome", "markdown", "s1", "mc=1"),
        ResponseCache::key("https://a.test/", "honest", "json", "s1", "mc=1"),
        ResponseCache::key("https://a.test/", "honest", "markdown", "s2", "mc=1"),
        ResponseCache::key("https://a.test/", "honest", "markdown", "s1", "mc=2"),
        // delimiter shifting must not collide
        ResponseCache::key("https://a.test/", "honest", "markdown|s1", "", "mc=1"),
    ];
    for variant in &variants {
        assert_ne!(&base, variant);
    }
    let unique: std::collections::BTreeSet<_> = variants.iter().collect();
    assert_eq!(unique.len(), variants.len());
}

#[test]
fn response_cache_stores_body_with_metadata_and_expires_by_ttl() {
    let dir = root("response");
    let cache = ResponseCache::new(&dir);
    let key = ResponseCache::key("https://a.test/", "honest", "markdown", "", "k");
    cache
        .put(
            &key,
            b"body",
            &json!({"status": 200, "url": "https://a.test/"}),
        )
        .unwrap();
    let (body, meta) = cache.get(&key).unwrap();
    assert_eq!(body, b"body");
    assert_eq!(meta, json!({"status": 200, "url": "https://a.test/"}));
    assert!(matches!(
        cache.put("../../escape", b"x", &json!({})),
        Err(CacheError::InvalidId(_))
    ));
    assert!(matches!(cache.get("short"), Err(CacheError::InvalidId(_))));

    let short = ResponseCache::new(&dir).with_ttl(Duration::from_millis(20));
    std::thread::sleep(Duration::from_millis(60));
    assert!(matches!(short.get(&key), Err(CacheError::Expired(_))));
    assert!(short.get(&key).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn truncate_and_store_keeps_short_text_and_splits_long_text_eighty_twenty() {
    let dir = root("truncate");
    let cache = OutputCache::new(&dir, None);
    let options = StoreOptions {
        char_limit: 10,
        ..StoreOptions::default()
    };
    let short = truncate_and_store("0123456789", options.clone(), Some(&cache)).unwrap();
    assert_eq!(short.output, "0123456789");
    assert!(!short.stored && short.cache_id.is_none());

    let long = truncate_and_store("0123456789ABCDEF", options, Some(&cache)).unwrap();
    assert!(long.stored);
    let id = long.cache_id.clone().unwrap();
    assert_eq!(
        long.output,
        format!("01234567\n\n...\n\nEF\n\n--- Full text stored: cache_id={id} ---")
    );
    assert_eq!(cache_id_from_output(&long.output), Some(id.as_str()));
    assert_eq!(cache.load(&id).unwrap(), b"0123456789ABCDEF");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn truncate_and_store_counts_characters_and_caps_stored_bytes_on_boundaries() {
    let dir = root("unicode");
    let cache = OutputCache::new(&dir, None);
    let text = "Grüße🌍".repeat(4);
    let result = truncate_and_store(
        &text,
        StoreOptions {
            char_limit: 5,
            max_stored_bytes: 11,
            ..StoreOptions::default()
        },
        Some(&cache),
    )
    .unwrap();
    let tight = truncate_and_store(
        &text,
        StoreOptions {
            char_limit: 5,
            max_stored_bytes: 10,
            ..StoreOptions::default()
        },
        Some(&cache),
    )
    .unwrap();
    assert_eq!(
        cache.load(tight.cache_id.as_deref().unwrap()).unwrap(),
        "Grüße".as_bytes(),
        "a 4-byte code point that does not fit is dropped whole"
    );
    let stored = cache.load(result.cache_id.as_deref().unwrap()).unwrap();
    assert!(stored.len() <= 11);
    assert!(std::str::from_utf8(&stored).is_ok(), "no split code point");
    assert_eq!(stored, "Grüße🌍".as_bytes(), "exactly 11 bytes fit");
    assert!(result.output.starts_with("Grüß"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn truncation_without_a_store_directory_is_an_error() {
    let error = truncate_and_store(
        "abcdef",
        StoreOptions {
            char_limit: 3,
            ..StoreOptions::default()
        },
        None,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "cache I/O: store directory not specified"
    );
}

#[test]
fn cache_id_extraction_requires_a_well_formed_id() {
    assert_eq!(
        cache_id_from_output("--- Full text stored: cache_id=out_0123456789ab ---"),
        Some("out_0123456789ab")
    );
    assert_eq!(cache_id_from_output("cache_id=out_zzzz"), None);
    assert_eq!(cache_id_from_output("no id here"), None);
}

#[test]
fn line_ranges_are_one_based_inclusive_and_open_ended_when_end_is_unset() {
    let text = b"a\nb\nc\nd";
    assert_eq!(line_range(text, 2, 3), "b\nc");
    assert_eq!(line_range(text, 0, 2), "a\nb");
    assert_eq!(line_range(text, 3, 0), "c\nd");
    assert_eq!(line_range(text, 3, 1), "c\nd");
    assert_eq!(line_range(text, 2, 99), "b\nc\nd");
    assert_eq!(line_range(text, 9, 10), "");
}

#[test]
fn option_keys_are_stable_across_insertion_order() {
    assert_eq!(
        content_key(15000, true, 20, 1024),
        "mc=15000 il=true ct=20 mi=1024"
    );
    let mut first = std::collections::BTreeMap::new();
    first.insert("b".to_string(), "2".to_string());
    first.insert("a".to_string(), "1".to_string());
    let mut second = std::collections::BTreeMap::new();
    second.insert("a".to_string(), "1".to_string());
    second.insert("b".to_string(), "2".to_string());
    assert_eq!(options_key(&first), "a=1 b=2");
    assert_eq!(options_key(&first), options_key(&second));
}
