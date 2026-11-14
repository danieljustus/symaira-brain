#![no_main]

//! Untrusted HTTP response bodies: bounded gzip/brotli/zstd content-decoding
//! (decompression bombs must stay under the limit) and charset
//! detection/transcoding. Keep the body in sync with
//! `crates/symbrowse-fetch/tests/fuzz_seed_replay.rs`.

use libfuzzer_sys::fuzz_target;
use symbrowse_fetch::{fuzz_decode_content, fuzz_normalize_charset};

fuzz_target!(|data: &[u8]| {
    // First byte selects the content-encoding stack, the second a charset label;
    // the remainder is the (untrusted) body.
    const ENCODINGS: [&str; 8] = [
        "", "identity", "gzip", "x-gzip", "br", "zstd", "gzip, br", "bogus",
    ];
    const CHARSETS: [&str; 5] = ["utf-8", "iso-8859-1", "shift_jis", "utf-16le", "bogus"];
    let (Some(&encoding), Some(&charset)) = (data.first(), data.get(1)) else {
        return;
    };
    let body = &data[2..];
    const MAX: usize = 1 << 16;
    if let Ok(decoded) = fuzz_decode_content(body, ENCODINGS[usize::from(encoding) % 8], MAX) {
        assert!(decoded.len() <= MAX);
    }
    let content_type = format!("text/html; charset={}", CHARSETS[usize::from(charset) % 5]);
    let _ = fuzz_normalize_charset(body.to_vec(), Some(&content_type));
    let _ = fuzz_normalize_charset(body.to_vec(), None);
});
