//! Go 1.26.7 Windows lexical rules, preserving native UTF16 path units.
// Derived from Go Authors' BSD-3-Clause Windows filepath rules (2024).
// The source license is retained in scripts/guard-standalone-oracle/windows-sdk-LICENSE.txt.
// Volume/post-clean rules follow internal/filepathlite; no filesystem lookup.
// Source-reference tests do not imply native Windows runtime acceptance.

pub(super) fn clean_units(path: &[u16]) -> Vec<u16> {
    decode_wtf8(&clean_bytes(&encode_wtf8(path)))
}

pub(super) fn join_units(base: &[u16], parts: &[&str]) -> Vec<u16> {
    decode_wtf8(&join_bytes(&encode_wtf8(base), parts))
}

fn join_bytes(base: &[u8], parts: &[&str]) -> Vec<u8> {
    let mut path = base.to_vec();
    for part in parts {
        let mut part = part.as_bytes();
        if path.last().is_some_and(|unit| separator(*unit)) {
            while part.first().is_some_and(|unit| separator(*unit)) {
                part = &part[1..];
            }
            if path.len() == 1
                && part.starts_with(&[63, 63])
                && (part.len() == 2 || part.get(2).is_some_and(|unit| separator(*unit)))
            {
                path.extend([46, 92]);
            }
        } else if !path.is_empty() && path.last() != Some(&58) {
            path.push(92);
        }
        path.extend(part);
    }
    if path.is_empty() {
        path
    } else {
        clean_bytes(&path)
    }
}

fn clean_bytes(path: &[u8]) -> Vec<u8> {
    let normalized: Vec<u8> = path
        .iter()
        .map(|unit| if *unit == 47 { 92 } else { *unit })
        .collect();
    let volume = volume_len(&normalized);
    let (prefix, rest) = normalized.split_at(volume);
    if rest.is_empty() {
        let mut out = prefix.to_vec();
        if !prefix.starts_with(&[92, 92]) {
            out.push(46);
        }
        return out;
    }
    let original = &path[volume..];
    let rooted = separator(original[0]);
    let mut result = Vec::new();
    let mut buffer: Option<Vec<u8>> = None;
    let mut read = 0;
    let mut boundary = 0;
    if rooted {
        append_unit(&mut result, 92, original, &mut buffer);
        read = 1;
        boundary = 1;
    }
    while read < original.len() {
        let unit = original[read];
        if separator(unit) {
            read += 1;
        } else if unit == 46 && (read + 1 == original.len() || separator(original[read + 1])) {
            read += 1;
        } else if unit == 46
            && original.get(read + 1) == Some(&46)
            && (read + 2 == original.len() || separator(original[read + 2]))
        {
            read += 2;
            if result.len() > boundary {
                result.pop();
                while result.len() > boundary && !result.last().is_some_and(|unit| separator(*unit))
                {
                    result.pop();
                }
                if result.len() > boundary {
                    result.pop();
                }
            } else if !rooted {
                if !result.is_empty() {
                    append_unit(&mut result, 92, original, &mut buffer);
                }
                append_unit(&mut result, 46, original, &mut buffer);
                append_unit(&mut result, 46, original, &mut buffer);
                boundary = result.len();
            }
        } else {
            if (rooted && result.len() != 1) || (!rooted && !result.is_empty()) {
                append_unit(&mut result, 92, original, &mut buffer);
            }
            while read < original.len() && !separator(original[read]) {
                append_unit(&mut result, original[read], original, &mut buffer);
                read += 1;
            }
        }
    }
    if result.is_empty() {
        append_unit(&mut result, 46, original, &mut buffer);
    }
    // Go postClean only acts after an actual buffer rewrite. Merely trimming
    // trailing separators or backtracking an unchanged prefix must not add .\.
    if volume == 0
        && let Some(buffer) = &buffer
    {
        // Go scans the complete allocated buffer, including hidden backing slots.
        if buffer
            .split(|unit| *unit == 92)
            .next()
            .is_some_and(|first| first.contains(&58))
        {
            result.splice(..0, [46, 92]);
        } else if buffer.starts_with(&[92, 63, 63]) {
            result.splice(..0, [92, 46]);
        }
    }
    [prefix, result.as_slice()].concat()
}

fn append_unit(output: &mut Vec<u8>, unit: u8, original: &[u8], buffer: &mut Option<Vec<u8>>) {
    if buffer.is_none() && original.get(output.len()) != Some(&unit) {
        let mut slots = vec![0; original.len()];
        slots[..output.len()].copy_from_slice(output);
        *buffer = Some(slots);
    }
    if let Some(slots) = buffer {
        slots[output.len()] = unit;
    }
    output.push(unit);
}

fn separator(unit: u8) -> bool {
    matches!(unit, 47 | 92)
}

fn volume_len(path: &[u8]) -> usize {
    if path.get(1) == Some(&58) {
        return 2;
    }
    if !path.starts_with(&[92]) {
        return 0;
    }
    if prefix_fold(path, r"\\.\UNC") {
        return unc_len(path, 8);
    }
    if [r"\\.", r"\\?", r"\??"]
        .iter()
        .any(|prefix| prefix_fold(path, prefix))
    {
        if path.len() == 3 {
            return 3;
        }
        return path[4..]
            .iter()
            .position(|unit| *unit == 92)
            .map_or(path.len(), |index| index + 4);
    }
    if path.starts_with(&[92, 92]) {
        return unc_len(path, 2);
    }
    0
}

fn prefix_fold(path: &[u8], prefix: &str) -> bool {
    path.len() >= prefix.len()
        && path
            .iter()
            .zip(prefix.bytes())
            .all(|(unit, byte)| unit.eq_ignore_ascii_case(&byte))
        && (path.len() == prefix.len() || path.get(prefix.len()) == Some(&92))
}

fn unc_len(path: &[u8], prefix: usize) -> usize {
    path.iter()
        .enumerate()
        .skip(prefix)
        .filter(|(_, unit)| **unit == 92)
        .nth(1)
        .map_or(path.len(), |(index, _)| index)
}

// Go's hidden lazy-buffer slots are byte-addressed. Encode WTF8 before cleaning
// and reconstruct original native UTF16 afterwards; a lossy Unicode projection
// or unit-addressed backing buffer would select/format different relative paths.
fn encode_wtf8(units: &[u16]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut index = 0;
    while index < units.len() {
        let mut point = u32::from(units[index]);
        if (0xd800..=0xdbff).contains(&point)
            && units
                .get(index + 1)
                .is_some_and(|unit| (0xdc00..=0xdfff).contains(unit))
        {
            point = 0x10000 + ((point - 0xd800) << 10) + u32::from(units[index + 1] - 0xdc00);
            index += 1;
        }
        let low = |value: u32| value.to_le_bytes()[0];
        match point {
            0..=0x7f => output.push(low(point)),
            0x80..=0x7ff => output.extend([0xc0 | low(point >> 6), 0x80 | low(point & 63)]),
            0x800..=0xffff => output.extend([
                0xe0 | low(point >> 12),
                0x80 | low((point >> 6) & 63),
                0x80 | low(point & 63),
            ]),
            _ => output.extend([
                0xf0 | low(point >> 18),
                0x80 | low((point >> 12) & 63),
                0x80 | low((point >> 6) & 63),
                0x80 | low(point & 63),
            ]),
        }
        index += 1;
    }
    output
}

fn decode_wtf8(bytes: &[u8]) -> Vec<u16> {
    let mut output = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let first = bytes[index];
        let width = match first {
            0..=0x7f => 1,
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            _ => 4,
        };
        let mask = match width {
            1 => 0x7f,
            2 => 31,
            3 => 15,
            _ => 7,
        };
        let mut point = u32::from(first & mask);
        for byte in &bytes[index + 1..index + width] {
            point = (point << 6) | u32::from(byte & 63);
        }
        let low = |value: u32| {
            let raw = value.to_le_bytes();
            u16::from_le_bytes([raw[0], raw[1]])
        };
        if point > 0xffff {
            point -= 0x10000;
            output.extend([0xd800 | low(point >> 10), 0xdc00 | low(point & 1023)]);
        } else {
            output.push(low(point));
        }
        index += width;
    }
    output
}

#[cfg(test)]
#[path = "guard_path_windows_tests.rs"]
mod tests;
