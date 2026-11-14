#![no_main]

//! Untrusted daemon request frames and raw MCP stdio framing. The daemon decoder
//! sees the input verbatim; the MCP adapter reads it as a client byte stream
//! through a proxy that always refuses, so no daemon or browser is reached.
//! Keep the body in sync with `crates/symbrowse-mcp/tests/fuzz_seed_replay.rs`.

use libfuzzer_sys::fuzz_target;
use symbrowse_daemon::{MAX_FRAME_BYTES, decode_frame};
use symbrowse_mcp::{NoopProxy, ServeOptions, serve_with_proxy};

fuzz_target!(|data: &[u8]| {
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
