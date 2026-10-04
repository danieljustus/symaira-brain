//! Go encoding/json escapes for the two Setup report consumers.
// Escape the serialized document, including ordinary String fields. Raw GoText
// values already escape these characters; their backslash escapes stay intact.
pub(crate) fn escape(text: &str) -> String {
    text.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}
