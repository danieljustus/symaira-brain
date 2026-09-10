#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use serde_json::{Value, json};
use symbrain_mcp::{Decoder, Response, write_message};

fuzz_target!(|data: &[u8]| {
    let mut decoder = Decoder::new(Cursor::new(data));
    for _ in 0..16 {
        match decoder.read_request() {
            Ok(Some((request, mode))) => {
                if !request.is_notification() {
                    let response = Response::success(request.id.unwrap_or(Value::Null), json!({}));
                    let mut output = Vec::new();
                    let _ = write_message(&mut output, mode, &response);
                }
            }
            Ok(None) | Err(_) => break,
        }
    }
});
