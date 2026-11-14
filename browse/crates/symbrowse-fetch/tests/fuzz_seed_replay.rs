//! Stable-Rust replay of the `fetch-text` fuzz target (`browse/fuzz`): robots.txt
//! and Wayback CDX seeds, prefixes and byte mutations must be handled without
//! panicking, and robots parsing must stay a pure function of its input.
#![deny(unsafe_code)]

use std::path::PathBuf;

use symbrowse_fetch::{
    archive, fuzz_decode_content, fuzz_normalize_charset, parse_retry_after, robots::Robots,
};

fn seeds(target: &str) -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/corpus")
        .join(target);
    let mut seeds: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(&path).unwrap(),
            )
        })
        .collect();
    seeds.sort();
    assert!(!seeds.is_empty(), "no seeds for {target}");
    seeds
}

/// Every seed, every prefix of it, and a deterministic single-byte mutation sweep.
fn sweep(target: &str, exercise: impl Fn(&[u8])) {
    for (name, seed) in seeds(target) {
        exercise(&seed);
        for end in 0..seed.len() {
            exercise(&seed[..end]);
        }
        for index in 0..seed.len().min(256) {
            for replacement in [0x00, b'\n', b'"', b'{', 0x7f, 0xff] {
                let mut mutated = seed.clone();
                mutated[index] = replacement;
                exercise(&mutated);
            }
        }
        eprintln!("replayed {target}/{name}");
    }
}

#[test]
fn fetch_text_seeds_are_total_and_deterministic() {
    sweep("fetch_text", |data| {
        let text = String::from_utf8_lossy(data);
        let robots = Robots::parse(&text);
        assert_eq!(robots, Robots::parse(&text));
        let _ = robots.allows("symbrowse", "/");
        let _ = robots.allows("*", "/a/b?c=d");
        let _ = archive::parse_cdx_response(&text);
        let _ = archive::parse_wayback_url(&text);
        let _ = parse_retry_after(&text);
    });
}

#[test]
fn http_body_decode_seeds_are_bounded_and_total() {
    sweep("http_body_decode", |data| {
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
}
