//! Go strings.TrimSpace on byte-valued module selections.
fn character(bytes: &[u8]) -> Option<(char, usize)> {
    let first = *bytes.first()?;
    let count = if first.is_ascii() {
        1
    } else {
        first.leading_ones() as usize
    };
    if !(1..=4).contains(&count) {
        return None;
    }
    let text = std::str::from_utf8(bytes.get(..count)?).ok()?;
    text.chars().next().map(|ch| (ch, count))
}
pub(super) fn trim_space(mut bytes: &[u8]) -> &[u8] {
    while let Some((ch, count)) = character(bytes) {
        if !ch.is_whitespace() {
            break;
        }
        bytes = &bytes[count..];
    }
    while !bytes.is_empty() {
        let last = (1..=4).find_map(|count| {
            let tail = bytes.get(bytes.len().checked_sub(count)?..)?;
            character(tail).filter(|(_, width)| *width == count)
        });
        let Some((ch, count)) = last else {
            break;
        };
        if !ch.is_whitespace() {
            break;
        }
        bytes = &bytes[..bytes.len() - count];
    }
    bytes
}
