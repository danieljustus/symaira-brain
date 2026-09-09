#![no_main]

use libfuzzer_sys::fuzz_target;
use serde_json::Value;
use symbrain_mcp::Request;

fuzz_target!(|data: &[u8]| {
    if let Ok(request) = serde_json::from_slice::<Request>(data) {
        assert_eq!(
            request.is_notification(),
            !request.has_id && request.id.is_none()
        );
        if let Some(params) = request.params {
            let _ = serde_json::from_str::<Value>(params.get());
        }
    }
});
