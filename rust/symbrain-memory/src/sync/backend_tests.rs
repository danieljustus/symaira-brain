//! Prepared concrete backend regressions, not executed in source-only stage.

use super::{CryptoEngine, EntropySource, RelayBlob, SyncError, json, remote::Url, wire};
use std::io::{self, Read};

#[test]
fn duplicate_maps_and_hidden_slice_slots_survive_nonempty_shrink() {
    let bytes=br#"{"memories":[{"id":"first","metadata":{"a":"old","b":"kept"},"metadata":{"a":null},"embedding":[1,2],"embedding":[3],"embedding":[null,null]},{"id":"hidden"}],"memories":[{}],"memories":[{},{}]}"#;
    let rows = json::changes(bytes)
        .expect("ordered decode")
        .memories
        .expect("allocated");
    assert_eq!(rows[0].metadata.as_ref().expect("map")["a"], "");
    assert_eq!(rows[0].metadata.as_ref().expect("map")["b"], "kept");
    assert_eq!(rows[0].embedding, Some(vec![3.0, 2.0]));
    assert_eq!(rows[1].id, "hidden");
    let reset =
        json::changes(br#"{"memories":[{"id":"discarded"}],"memories":[],"memories":[{}]}"#)
            .expect("reset");
    assert_eq!(reset.memories.expect("allocated")[0].id, "");
}

#[test]
fn depth_limit_applies_to_ignored_values_without_recursive_stack() {
    for arrays in [9999, 10000] {
        let mut data = b"{\"ignored\":".to_vec();
        data.extend(vec![b'['; arrays]);
        data.extend(vec![b']'; arrays]);
        data.push(b'}');
        let parsed = json::changes(&data);
        if arrays == 9999 {
            assert!(parsed.is_ok());
        } else {
            assert_eq!(
                parsed.expect_err("10001 containers").to_string(),
                "invalid character '[' exceeded max depth"
            );
        }
    }
}

#[test]
fn bad_utf8_and_unpaired_surrogates_follow_go_replacement() {
    let mut data = b"{\"next_cursor\":\"".to_vec();
    data.extend([0xf0, 0x90]);
    data.extend(b"\\uD800\"}");
    assert_eq!(
        json::changes(&data).expect("Go replacement").next_cursor,
        "\u{fffd}\u{fffd}\u{fffd}"
    );
}

#[test]
fn first_response_object_finishes_without_waiting_for_trailing_body() {
    struct Reader(bool);
    impl Read for Reader {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.0 {
                return Err(io::Error::other("must not read trailing network body"));
            }
            self.0 = true;
            out[..2].copy_from_slice(b"{}");
            Ok(2)
        }
    }
    assert_eq!(
        super::http::body::first_json(&mut Reader(false), 1024).expect("first object"),
        b"{}"
    );
    assert_eq!(
        json::apply_result(b"{\"applied\":1} invalid trailing bytes")
            .expect("first JSON")
            .applied,
        1
    );
    assert!(json::relay_payload(b"{} invalid trailing bytes").is_err());
    assert!(json::changes(b"nullinvalid trailing bytes").is_ok());
    assert!(json::relay_payload(b"nullinvalid trailing bytes").is_err());
}

#[test]
fn nil_and_allocated_empty_bytes_remain_distinct_and_keep_hidden_slots() {
    let decoded =
        json::relay_changes(br#"{"blobs":[{"blob":[1,2],"blob":[3],"blob":[null,null]}]}"#)
            .expect("byte slots");
    assert_eq!(decoded.blobs[0].blob, [3, 2]);
    let raw = wire::relay(&[RelayBlob::default()]).expect("nil");
    assert!(
        String::from_utf8(raw)
            .expect("JSON")
            .contains("\"blob\":null")
    );
    let raw = wire::relay(&[RelayBlob {
        blob_present: true,
        ..RelayBlob::default()
    }])
    .expect("empty allocated");
    assert!(
        String::from_utf8(raw)
            .expect("JSON")
            .contains("\"blob\":\"\"")
    );
}

#[test]
fn initial_raw_paths_and_reference_cleaning_have_separate_owners() {
    let initial = Url::parse("http://localhost/a/../b/%2f!").expect("raw Go path");
    assert_eq!(initial.uri().expect("URI"), "http://localhost/a/../b/%2f!");
    assert_eq!(
        initial
            .reference("../final")
            .expect("reference")
            .uri()
            .expect("URI"),
        "http://localhost/final"
    );
    let raw = Url::parse("http://%ff:%80@localhost/x").expect("raw Basic bytes");
    assert_eq!(raw.basic().expect("Basic"), b"Basic /zqA");
}

#[test]
fn cache_nonce_authentication_and_phase_reset_are_real_crypto_boundaries() {
    struct OwnedEntropy(u8);
    impl EntropySource for OwnedEntropy {
        fn fill(&mut self, out: &mut [u8]) -> Result<(), SyncError> {
            for byte in out {
                *byte = self.0;
                self.0 = self.0.wrapping_add(1);
            }
            Ok(())
        }
    }
    let mut engine = CryptoEngine::with_entropy(Box::new(OwnedEntropy(1)));
    let first = engine
        .encrypt_bytes(b"synthetic", "owned passphrase")
        .expect("AES-GCM");
    let second = engine
        .encrypt_bytes(b"synthetic", "owned passphrase")
        .expect("cached PBKDF2");
    assert_eq!(&first[1..17], &second[1..17]);
    assert_ne!(&first[17..29], &second[17..29]);
    assert_eq!(
        engine
            .decrypt_bytes(&first, "owned passphrase")
            .expect("authenticated"),
        b"synthetic"
    );
    let mut corrupted = first.clone();
    *corrupted.last_mut().expect("tag") ^= 1;
    assert_eq!(
        engine
            .decrypt_bytes(&corrupted, "owned passphrase")
            .expect_err("corrupted tag")
            .to_string(),
        "decryption failed: invalid passphrase or corrupted payload"
    );
    super::RelayCodec::reset_phase(&mut engine);
    let third = engine
        .encrypt_bytes(b"synthetic", "owned passphrase")
        .expect("new phase");
    assert_ne!(&first[1..17], &third[1..17]);
}

#[test]
fn credential_remote_identity_is_refused_before_cursor_or_network_work() {
    use super::{HttpSyncTransport, SyncTransport, validate_remote_url};
    let remote = "http://owned-user:owned-password@localhost/sync";
    let client = HttpSyncTransport::new(remote, b"", false).expect("owned public client");
    assert!(
        client
            .validate(remote, false)
            .expect_err("no secret cursor")
            .to_string()
            .contains("not admitted")
    );
    let error = validate_remote_url("http://owned-user:owned-password@example.invalid", false)
        .expect_err("HTTPS guard")
        .to_string();
    assert!(!error.contains("owned-user"));
    assert!(!error.contains("owned-password"));
    let malformed = Url::parse("http://owned-user:owned-password%zz@localhost")
        .err()
        .expect("escape error")
        .to_string();
    assert!(!malformed.contains("owned-password"));
    assert!(malformed.contains("details withheld"));
}
