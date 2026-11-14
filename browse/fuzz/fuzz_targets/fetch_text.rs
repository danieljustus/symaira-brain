#![no_main]

//! Untrusted text from remote servers consumed by the static fetcher:
//! robots.txt, Wayback CDX JSON and Retry-After values. Keep the body in sync with
//! `crates/symbrowse-fetch/tests/fuzz_seed_replay.rs`.

use libfuzzer_sys::fuzz_target;
use symbrowse_fetch::{archive, parse_retry_after, robots::Robots};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let robots = Robots::parse(&text);
    assert_eq!(robots, Robots::parse(&text));
    let _ = robots.allows("symbrowse", "/");
    let _ = robots.allows("*", "/a/b?c=d");
    let _ = archive::parse_cdx_response(&text);
    let _ = archive::parse_wayback_url(&text);
    let _ = parse_retry_after(&text);
});
