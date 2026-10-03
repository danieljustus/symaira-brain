//! Byte-preserving Go encoding/json string values shared by source publication and reports.
/// Encodes valid text normally and every malformed UTF-8 byte as `\\ufffd`.
///
/// # Errors
/// Returns a JSON serialization error if the encoded string cannot be represented.
// Go strings retain malformed tool stdout bytes. encoding/json encodes each
// malformed byte as \ufffd, while valid U+FFFD remains literal UTF-8.
pub fn go_json_string_bytes(
    mut bytes: &[u8],
) -> Result<Box<serde_json::value::RawValue>, serde_json::Error> {
    let mut json = String::from("\"");
    while !bytes.is_empty() {
        let valid = std::str::from_utf8(bytes);
        let count = valid
            .as_ref()
            .map_or_else(std::str::Utf8Error::valid_up_to, |text| text.len());
        let text = std::str::from_utf8(&bytes[..count])
            .map_err(<serde_json::Error as serde::ser::Error>::custom)?;
        let quoted = serde_json::to_string(text)?;
        json.push_str(&quoted[1..quoted.len() - 1]);
        bytes = &bytes[count..];
        if valid.is_err() {
            json.push_str("\\ufffd");
            bytes = &bytes[1..];
        }
    }
    json.push('"');
    serde_json::value::RawValue::from_string(json)
}
