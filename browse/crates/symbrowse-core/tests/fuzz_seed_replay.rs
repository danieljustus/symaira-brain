//! Stable-Rust replay of the `flow-yaml`, `state-headers` and `urls` fuzz
//! targets (`browse/fuzz`): the tracked seeds, their prefixes and a
//! deterministic byte-mutation sweep must be handled without panicking and the
//! parsers must stay pure functions of their input.
#![deny(unsafe_code)]

use std::path::PathBuf;

use symbrowse_core::{
    flows,
    policy::{Allowlist, check_ssrf_with_lookup, policy_host},
    state::{FILE_MAGIC, decode, validate_name},
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
fn flow_yaml_seeds_are_total_and_deterministic() {
    sweep("flow_yaml", |data| {
        let first = flows::parse(data, "fuzz");
        assert_eq!(first, flows::parse(data, "fuzz"));
        if let Ok(flow) = first {
            let _ = flows::dry_run(&flow);
        }
    });
}

#[test]
fn state_header_seeds_are_total() {
    let key = [0x5a_u8; 32];
    sweep("state_headers", |data| {
        let mut prefixed = FILE_MAGIC.to_vec();
        prefixed.extend_from_slice(data);
        for raw in [data, prefixed.as_slice()] {
            let _ = decode(raw, None);
            let _ = decode(raw, Some(&key));
        }
        let name = String::from_utf8_lossy(data);
        if validate_name(&name).is_ok() {
            assert!(!name.contains('/'));
            assert!(!name.chars().any(char::is_control));
        }
    });
}

#[test]
fn url_and_allowlist_seeds_are_total() {
    sweep("urls", |data| {
        let text = String::from_utf8_lossy(data);
        let mut lines = text.lines();
        let url = lines.next().unwrap_or_default();
        let patterns: Vec<String> = lines.map(str::to_owned).collect();
        let first = check_ssrf_with_lookup(url, |_| Ok(vec!["10.0.0.1".to_owned()]));
        assert_eq!(
            first,
            check_ssrf_with_lookup(url, |_| Ok(vec!["10.0.0.1".to_owned()]))
        );
        if first.is_ok() {
            let host = policy_host(url);
            assert!(host.parse::<std::net::IpAddr>().is_ok() || host.is_empty());
        }
        if let Ok(allowlist) = Allowlist::parse(&patterns) {
            let _ = allowlist.allows_url(url);
            let _ = allowlist.allows_host(&policy_host(url));
        }
    });
}
