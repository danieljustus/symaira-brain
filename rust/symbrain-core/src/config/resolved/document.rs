//! One locked native parser; syntax/admission priority is a separate acceptance gate.
use crate::GoText;
use toml_edit::DocumentMut;

pub(super) fn parse(bytes: &[u8]) -> Result<DocumentMut, GoText> {
    // BurntSushi1.6 strips either UTF16 marker before lexing even when the
    // remaining file is ordinary UTF8. These accepted inputs are not a
    // malformed-error wording exception. Preserve its single-prefix rule.
    let bytes = if bytes.starts_with(b"\xff\xfe") || bytes.starts_with(b"\xfe\xff") {
        &bytes[2..]
    } else if bytes.starts_with(b"\xef\xbb\xbf") {
        &bytes[3..]
    } else {
        bytes
    };
    let text = std::str::from_utf8(bytes).map_err(|error| -> GoText {
        // Preserve the input byte; a Rust UTF-8 diagnostic is not a Go oracle.
        let byte = bytes[error.valid_up_to()];
        let line = bytes[..error.valid_up_to()]
            .iter()
            .filter(|b| **b == b'\n')
            .count()
            + 1;
        // Grammar-before-invalid-UTF8 priority must be proven against the SDK
        // before native cutover. No parser wording decision waives that gate.
        format!("invalid UTF-8 byte 0x{byte:02x} at line {line}").into()
    })?;
    text.parse::<DocumentMut>().map_err(|error| {
        // Native inner wording is deliberately parser-owned; outer selected
        // file/stage context and ordered typed conversion errors remain strict.
        // Report position/reason, without reproducing the complete input line.
        let offset = error.span().map_or(0, |span| span.start.min(bytes.len()));
        let prefix = &bytes[..offset];
        let line = prefix.iter().filter(|byte| **byte == b'\n').count() + 1;
        let column = offset
            - prefix
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(0, |index| index + 1)
            + 1;
        format!(
            "TOML parse error at line {line}, column {column}: {}",
            error.message()
        )
        .into()
    })
}
