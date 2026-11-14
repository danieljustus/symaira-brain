#![no_main]

//! Untrusted URLs and domain allowlists. The first line is the URL, remaining
//! lines are allowlist patterns. The resolver is deterministic and always
//! answers with a private address, so a URL may only be accepted when it does
//! not depend on name resolution reaching a private host.
//! Keep the body in sync with `crates/symbrowse-core/tests/fuzz_seed_replay.rs`.

use libfuzzer_sys::fuzz_target;
use symbrowse_core::policy::{Allowlist, check_ssrf_with_lookup, policy_host};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let mut lines = text.lines();
    let url = lines.next().unwrap_or_default();
    let patterns: Vec<String> = lines.map(str::to_owned).collect();

    let first = check_ssrf_with_lookup(url, |_| Ok(vec!["10.0.0.1".to_owned()]));
    assert_eq!(
        first,
        check_ssrf_with_lookup(url, |_| Ok(vec!["10.0.0.1".to_owned()]))
    );
    // A hostname that resolves to a private address must never be accepted.
    if first.is_ok() {
        let host = policy_host(url);
        assert!(host.parse::<std::net::IpAddr>().is_ok() || host.is_empty());
    }
    if let Ok(allowlist) = Allowlist::parse(&patterns) {
        let _ = allowlist.allows_url(url);
        let _ = allowlist.allows_host(&policy_host(url));
    }
});
