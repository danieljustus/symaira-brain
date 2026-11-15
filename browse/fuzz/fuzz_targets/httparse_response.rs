#![no_main]

//! Attacker-controlled HTTP/1 bytes as the pinned `httparse` sees them: hyper
//! (and therefore reqwest and the fetch crate) parses every response head,
//! header block and chunk-size line from a remote server with this crate. The
//! parser must be total, deterministic and never report consuming more bytes
//! than it was given.
//! Keep the body in sync with `crates/symbrowse-fetch/tests/parser_dependency_replay.rs`.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut headers);
    let first = response.parse(data);
    if let Ok(httparse::Status::Complete(used)) = first {
        assert!(used <= data.len());
        assert!(response.headers.iter().all(|header| !header.name.is_empty()));
    }
    let mut again_headers = [httparse::EMPTY_HEADER; 64];
    assert_eq!(first, httparse::Response::new(&mut again_headers).parse(data));

    let mut header_block = [httparse::EMPTY_HEADER; 64];
    if let Ok(httparse::Status::Complete((used, _))) = httparse::parse_headers(data, &mut header_block) {
        assert!(used <= data.len());
    }
    if let Ok(httparse::Status::Complete((used, _))) = httparse::parse_chunk_size(data) {
        assert!(used <= data.len());
    }
    let mut request_headers = [httparse::EMPTY_HEADER; 64];
    let _ = httparse::Request::new(&mut request_headers).parse(data);
});
