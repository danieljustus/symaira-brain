//! One native accepting parser with ordered byte/syntax error admission.
use crate::GoText;
use toml_edit::{DocumentMut, TomlError};

pub(super) fn parse(bytes: &[u8]) -> Result<DocumentMut, GoText> {
    // BurntSushi1.6 removes exactly one marker before lexing. It does not
    // interpret the rest of an FF FE / FE FF file as UTF16.
    let bytes = if bytes.starts_with(b"\xff\xfe") || bytes.starts_with(b"\xfe\xff") {
        &bytes[2..]
    } else if bytes.starts_with(b"\xef\xbb\xbf") {
        &bytes[3..]
    } else {
        bytes
    };
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(utf8) => {
            let boundary = utf8.valid_up_to();
            // An invalid later byte cannot hide an earlier grammar failure.
            // The replacement view is used only to classify a rejecting
            // parser result; never apply a document built from repaired bytes.
            let view = String::from_utf8_lossy(bytes);
            if let Err(error) = view.parse::<DocumentMut>()
                && error.span().is_some_and(|span| span.start < boundary)
            {
                return Err(parse_error(bytes, &error));
            }
            let line = bytes[..boundary].split(|byte| *byte == b'\n').count();
            return Err(format!(
                "toml: line {line}: invalid UTF-8 byte: 0x{:02x}",
                bytes[boundary]
            )
            .into());
        }
    };
    text.parse::<DocumentMut>()
        .map_err(|error| parse_error(bytes, &error))
}
fn parse_error(bytes: &[u8], error: &TomlError) -> GoText {
    let offset = error.span().map_or(0, |span| span.start.min(bytes.len()));
    if let Some(detail) = super::syntax::bare_value_error(bytes, offset + 1) {
        return detail;
    }
    let prefix = &bytes[..offset];
    let line = prefix.split(|byte| *byte == b'\n').count();
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
}
