//! Encoding modes adapted from Go1.26.7 net/url/gen_encoding_table.go.
// Source-derived compatibility logic: Go1.26.7 SDK, Go Authors, BSD-3-Clause.
// Retained license: scripts/memory-sync-oracle/reference/GO-SDK-LICENSE.

use super::{SyncError, quoted};
#[derive(Clone, Copy)]
pub(super) enum Mode {
    Path,
    Host,
    Zone,
}
pub(super) fn host_allowed(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-_.~!$&'()*+,;=:[]<>\"".contains(&byte)
}
pub(super) fn unescape(raw: &str, mode: Mode) -> Result<Vec<u8>, SyncError> {
    let data = raw.as_bytes();
    let mut out = Vec::with_capacity(data.len());
    let mut at = 0;
    while at < data.len() {
        let byte = data[at];
        if byte == b'%' {
            if at + 2 >= data.len()
                || !data[at + 1].is_ascii_hexdigit()
                || !data[at + 2].is_ascii_hexdigit()
            {
                return Err(SyncError(format!(
                    "invalid URL escape {}",
                    quoted(&String::from_utf8_lossy(&data[at..data.len().min(at + 3)]))
                )));
            }
            let value = hex(data[at + 1]) * 16 + hex(data[at + 2]);
            let percent = &data[at..at + 3];
            if matches!(mode, Mode::Host) && value < 128 && percent != b"%25"
                || matches!(mode, Mode::Zone)
                    && percent != b"%25"
                    && value != b' '
                    && !host_allowed(value)
            {
                return Err(SyncError(format!(
                    "invalid URL escape {}",
                    quoted(std::str::from_utf8(percent).expect("ASCII escape"))
                )));
            }
            out.push(value);
            at += 3;
        } else {
            if matches!(mode, Mode::Host | Mode::Zone) && byte < 128 && !host_allowed(byte) {
                return Err(SyncError(format!(
                    "invalid character {} in host name",
                    quoted(&char::from(byte).to_string())
                )));
            }
            out.push(byte);
            at += 1;
        }
    }
    Ok(out)
}
fn hex(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => byte - b'A' + 10,
    }
}
pub(super) fn query_escape(raw: &str) -> String {
    encode(
        raw.as_bytes(),
        |byte| byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte),
        true,
    )
}
pub(super) fn path(raw: &str) -> String {
    // validEncoded permits RFC sub-delims beyond Go's default escape table.
    // A valid RawPath keeps encoded slash/dot/case identity; an invalid hint
    // re-encodes the whole decoded Path, not just its bad character.
    if raw
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-_.~$&+,/:;=@!()*[]%'".contains(&b))
    {
        return raw.into();
    }
    let bytes = unescape(raw, Mode::Path).expect("parse validated path escapes");
    encode(
        &bytes,
        |b| b.is_ascii_alphanumeric() || b"-_.~$&+,/:;=@".contains(&b),
        false,
    )
}
fn encode(bytes: &[u8], allowed: impl Fn(u8) -> bool, plus: bool) -> String {
    let mut out = String::new();
    for byte in bytes {
        if allowed(*byte) {
            out.push(char::from(*byte));
        } else if plus && *byte == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
