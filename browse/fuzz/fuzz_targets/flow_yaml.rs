#![no_main]

//! Untrusted flow YAML: bounded parse, validation, dry-run planning and the
//! pure-function determinism of the parser.
//! Keep the body in sync with `crates/symbrowse-core/tests/fuzz_seed_replay.rs`.

use libfuzzer_sys::fuzz_target;
use symbrowse_core::flows;

fuzz_target!(|data: &[u8]| {
    let first = flows::parse(data, "fuzz");
    assert_eq!(first, flows::parse(data, "fuzz"));
    if let Ok(flow) = first {
        let _ = flows::dry_run(&flow);
    }
});
