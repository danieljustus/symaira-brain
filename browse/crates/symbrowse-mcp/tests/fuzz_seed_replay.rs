//! Stable-Rust replay of the `daemon-mcp-frames` fuzz target (`browse/fuzz`):
//! daemon request frames and raw MCP stdio framing, with a proxy that always
//! refuses so no daemon or browser is reached.
#![deny(unsafe_code)]

use std::path::PathBuf;

use symbrowse_daemon::{MAX_FRAME_BYTES, decode_frame};
use symbrowse_mcp::{NoopProxy, ServeOptions, serve_with_proxy};

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
fn daemon_and_mcp_frame_seeds_are_total() {
    sweep("daemon_mcp_frames", |data| {
        match decode_frame(data) {
            Ok(frame) => {
                assert!(!frame.cmd.is_empty());
                assert!(data.len() <= MAX_FRAME_BYTES);
            }
            Err(error) => assert!(!error.code.is_empty()),
        }
        let mut output = Vec::new();
        let _ = serve_with_proxy(data, &mut output, ServeOptions::default(), &mut NoopProxy);
    });
}
