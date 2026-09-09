use super::{
    BEGIN_MARKER, END_MARKER, ESCAPE_SENTINEL, ESCAPED_BEGIN_MARKER, ESCAPED_END_MARKER,
    ESCAPED_ESCAPE_SENTINEL,
};

/// Replace the first managed block in `existing`, or append one if absent.
///
/// This deliberately operates on bytes rather than strings. Go's reference
/// implementation accepts arbitrary string bytes, so invalid UTF-8 must not
/// be rejected or normalized by the port. Delimiters and inserted line feeds
/// are ASCII; every other byte outside the first block is copied verbatim.
/// Literal delimiters in `content` are written using the stable escaped
/// spellings so the generated block remains unambiguous and idempotent.
#[must_use]
pub fn render(existing: &[u8], content: &[u8]) -> Vec<u8> {
    let managed = escape_content(content);
    let Some(begin) = find(existing, BEGIN_MARKER) else {
        let mut result = Vec::with_capacity(existing.len() + managed.len() + 64);
        result.extend_from_slice(existing);
        if !existing.is_empty() && !existing.ends_with(b"\n") {
            result.push(b'\n');
        }
        append_block(&mut result, &managed);
        return result;
    };

    let after_begin = &existing[begin..];
    let Some(relative_end) = find(after_begin, END_MARKER) else {
        let mut result = Vec::with_capacity(begin + managed.len() + 64);
        result.extend_from_slice(&existing[..begin]);
        append_block(&mut result, &managed);
        return result;
    };

    let end = begin + relative_end + END_MARKER.len();
    let suffix = &existing[end..];
    let mut result = Vec::with_capacity(existing.len() + managed.len() + 64);
    result.extend_from_slice(&existing[..begin]);
    result.extend_from_slice(BEGIN_MARKER);
    result.push(b'\n');
    result.extend_from_slice(&managed);
    result.extend_from_slice(END_MARKER);
    if !suffix.is_empty() && !suffix.starts_with(b"\n") && !suffix.starts_with(b"\r\n") {
        result.push(b'\n');
    }
    result.extend_from_slice(suffix);
    result
}

fn append_block(result: &mut Vec<u8>, content: &[u8]) {
    result.extend_from_slice(BEGIN_MARKER);
    result.push(b'\n');
    result.extend_from_slice(content);
    result.extend_from_slice(END_MARKER);
    result.push(b'\n');
}

fn escape_content(content: &[u8]) -> Vec<u8> {
    // Encode the sentinel first. Every emitted code word starts with the
    // sentinel and has a distinct suffix, so the encoding is injective even
    // for content that already contains an escape token.
    let content = replace_bytes(content, ESCAPE_SENTINEL, ESCAPED_ESCAPE_SENTINEL);
    let content = replace_bytes(&content, BEGIN_MARKER, ESCAPED_BEGIN_MARKER);
    replace_bytes(&content, END_MARKER, ESCAPED_END_MARKER)
}

fn replace_bytes(input: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative) = find(&input[cursor..], needle) {
        let position = cursor + relative;
        result.extend_from_slice(&input[cursor..position]);
        result.extend_from_slice(replacement);
        cursor = position + needle.len();
    }
    result.extend_from_slice(&input[cursor..]);
    result
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
