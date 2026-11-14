#![no_main]

//! Untrusted state-file bytes: magic, versioned JSON header, legacy payloads
//! and authenticated bodies, with and without a key. Inputs are also retried
//! behind a valid magic prefix so the header parsers are reached directly.
//! Keep the body in sync with `crates/symbrowse-core/tests/fuzz_seed_replay.rs`.

use libfuzzer_sys::fuzz_target;
use symbrowse_core::state::{FILE_MAGIC, decode, validate_name};

const KEY: [u8; 32] = [0x5a; 32];

fuzz_target!(|data: &[u8]| {
    let mut prefixed = FILE_MAGIC.to_vec();
    prefixed.extend_from_slice(data);
    for raw in [data, prefixed.as_slice()] {
        let _ = decode(raw, None);
        let _ = decode(raw, Some(&KEY));
    }
    let name = String::from_utf8_lossy(data);
    if validate_name(&name).is_ok() {
        // Contract of `validate_name`: one path component, no ASCII controls.
        // C1 controls (U+0080..U+009F) are accepted by the current contract; they
        // are not path syntax, see docs/rust-port/hardening.md.
        assert!(!name.contains('/'));
        assert!(!name.chars().any(|c| c < '\u{20}' || c == '\u{7f}'));
    }
});
