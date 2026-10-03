//! Stable Go request argument encoding.
use std::fmt::Write as _;

pub(super) fn encode_query_arg(value: &str) -> String {
    value.bytes().fold(String::new(), |mut out, byte| {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else if byte == b' ' {
            // Go's url.QueryEscape (used by url.Values.Encode) renders a space
            // as '+', not %20.
            out.push('+');
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
        out
    })
}

/// Serializes one request argument the way the shipped code's
/// `json.Marshal([]any{arg})` does for the `args` query value and the POST
/// body: compact JSON whose strings carry Go's HTML escapes (`&`, `<`, `>`)
/// and U+2028/U+2029 escapes, which `serde_json` leaves alone. None of those
/// characters can occur in the JSON structure itself, so escaping them after
/// serialization cannot alter non-string content.
pub(super) fn go_json_string_array(value: &str) -> String {
    let mut encoded =
        serde_json::to_string(&[value]).expect("serializing a string slice cannot fail");
    encoded = encoded
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e");
    encoded = encoded
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    encoded
}
