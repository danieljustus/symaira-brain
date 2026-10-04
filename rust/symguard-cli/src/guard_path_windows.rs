//! Go 1.26.7 Windows lexical rules, preserving native UTF16 path units.
// Volume/post-clean rules follow internal/filepathlite; no filesystem lookup.
// Source-reference tests do not imply native Windows runtime acceptance.

pub(super) fn join_units(base: &[u16], parts: &[&str]) -> Vec<u16> {
    let mut path = base.to_vec();
    for part in parts {
        let mut part: &[u16] = &part.encode_utf16().collect::<Vec<_>>();
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
        clean_units(&path)
    }
}

pub(super) fn clean_units(path: &[u16]) -> Vec<u16> {
    let normalized: Vec<u16> = path
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
    let rooted = rest.starts_with(&[92]);
    let mut parts: Vec<&[u16]> = Vec::new();
    for part in rest.split(|unit| *unit == 92) {
        match part {
            [] | [46] => {}
            [46, 46] if parts.last().is_some_and(|previous| *previous != [46, 46]) => {
                parts.pop();
            }
            [46, 46] if !rooted => parts.push(part),
            [46, 46] => {}
            _ => parts.push(part),
        }
    }
    let mut result = Vec::new();
    if rooted {
        result.push(92);
    }
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            result.push(92);
        }
        result.extend_from_slice(part);
    }
    if result.is_empty() {
        result.push(46);
    }
    // Do not create drive-relative or Root Local Device paths by dropping dots.
    if volume == 0 && result != normalized {
        if result
            .split(|unit| *unit == 92)
            .next()
            .is_some_and(|first| first.contains(&58))
        {
            result.splice(..0, [46, 92]);
        } else if result.starts_with(&[92, 63, 63]) {
            result.splice(..0, [92, 46]);
        }
    }
    [prefix, result.as_slice()].concat()
}

fn separator(unit: u16) -> bool {
    matches!(unit, 47 | 92)
}

fn volume_len(path: &[u16]) -> usize {
    // Go checks the second UTF8/WTF8 byte; non-ASCII "drive letters" do not qualify.
    if path.first().is_some_and(|unit| *unit < 128) && path.get(1) == Some(&58) {
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

fn prefix_fold(path: &[u16], prefix: &str) -> bool {
    path.len() >= prefix.len()
        && path.iter().zip(prefix.bytes()).all(|(unit, byte)| {
            u8::try_from(*unit).is_ok_and(|unit| unit.eq_ignore_ascii_case(&byte))
        })
        && (path.len() == prefix.len() || path.get(prefix.len()) == Some(&92))
}

fn unc_len(path: &[u16], prefix: usize) -> usize {
    path.iter()
        .enumerate()
        .skip(prefix)
        .filter(|(_, unit)| **unit == 92)
        .nth(1)
        .map_or(path.len(), |(index, _)| index)
}

#[cfg(test)]
#[path = "guard_path_windows_tests.rs"]
mod tests;
