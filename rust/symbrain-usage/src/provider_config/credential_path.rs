// Lexical credential joins follow Go filepath, never filesystem canonicalize.
// Based on Go's BSD-3-Clause filepathlite Clean and filepath Join algorithms;
// see migration/licenses/go-filepath-bsd.txt. Preserve native OS path units.
fn credential_join(base: &Path, child: &str) -> PathBuf {
    let mut path = credential_path_units(base);
    let mut tail = credential_path_units(Path::new(child));
    if !path.is_empty() {
        if cfg!(windows) && path.last().copied().is_some_and(credential_path_separator) {
            let start = tail
                .iter()
                .position(|unit| !credential_path_separator(*unit))
                .unwrap_or(tail.len());
            tail.drain(..start);
            if path.len() == 1
                && tail.starts_with(&[63, 63])
                && (tail.len() == 2 || credential_path_separator(tail[2]))
            {
                path.extend([46, 92]);
            }
        } else if !(cfg!(windows) && path.last() == Some(&58)) {
            path.push(credential_path_sep());
        }
    }
    path.extend(tail);
    if path.is_empty() {
        return PathBuf::new();
    }
    credential_path_from_units(&credential_clean_units(&path))
}

#[cfg(unix)]
fn credential_path_units(path: &Path) -> Vec<u16> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str()
        .as_bytes()
        .iter()
        .copied()
        .map(u16::from)
        .collect()
}
#[cfg(unix)]
fn credential_path_from_units(path: &[u16]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(
        path.iter()
            .map(|unit| u8::try_from(*unit).expect("preserved Unix byte"))
            .collect(),
    )
    .into()
}
#[cfg(windows)]
fn credential_path_units(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().collect()
}
#[cfg(windows)]
fn credential_path_from_units(path: &[u16]) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    std::ffi::OsString::from_wide(path).into()
}
fn credential_path_sep() -> u16 {
    if cfg!(windows) { 92 } else { 47 }
}
fn credential_path_separator(unit: u16) -> bool {
    unit == 47 || (cfg!(windows) && unit == 92)
}

fn credential_clean_units(original: &[u16]) -> Vec<u16> {
    let volume = credential_volume_len(original);
    let path = &original[volume..];
    if path.is_empty() {
        let mut result = original.to_vec();
        if !(volume > 1
            && credential_path_separator(original[0])
            && credential_path_separator(original[1]))
        {
            result.push(46);
        }
        credential_from_slash(&mut result);
        return result;
    }
    let rooted = credential_path_separator(path[0]);
    let mut output = Vec::new();
    let mut stop = 0;
    if rooted {
        output.push(credential_path_sep());
        stop = 1;
    }
    let mut index = usize::from(rooted);
    while index < path.len() {
        let start = index;
        while index < path.len() && !credential_path_separator(path[index]) {
            index += 1;
        }
        let part = &path[start..index];
        index += 1;
        if part.is_empty() || part == [46] {
            continue;
        }
        if part == [46, 46] {
            if output.len() > stop {
                output.pop();
                while output.len() > stop
                    && !output
                        .last()
                        .copied()
                        .is_some_and(credential_path_separator)
                {
                    output.pop();
                }
                if output.len() > stop {
                    output.pop();
                }
            } else if !rooted {
                if !output.is_empty() {
                    output.push(credential_path_sep());
                }
                output.extend([46, 46]);
                stop = output.len();
            }
        } else {
            if (rooted && output.len() != 1) || (!rooted && !output.is_empty()) {
                output.push(credential_path_sep());
            }
            output.extend(part);
        }
    }
    if output.is_empty() {
        output.push(46);
    }
    // Go's postClean only applies after rewriting an unvolumed path.
    if cfg!(windows) && volume == 0 && output != path {
        if output
            .iter()
            .take_while(|unit| !credential_path_separator(**unit))
            .any(|unit| *unit == 58)
        {
            output.splice(..0, [46, 92]);
        } else if output.len() >= 3
            && credential_path_separator(output[0])
            && output[1..3] == [63, 63]
        {
            output.splice(..0, [92, 46]);
        }
    }
    let mut result = original[..volume].to_vec();
    result.extend(output);
    credential_from_slash(&mut result);
    result
}
fn credential_from_slash(path: &mut [u16]) {
    if cfg!(windows) {
        for unit in path {
            if *unit == 47 {
                *unit = 92;
            }
        }
    }
}
#[cfg(not(windows))]
fn credential_volume_len(_: &[u16]) -> usize {
    0
}
#[cfg(windows)]
fn credential_volume_len(path: &[u16]) -> usize {
    if path.len() >= 2 && path[0] < 128 && path[1] == 58 {
        return 2;
    }
    if path.is_empty() || !credential_path_separator(path[0]) {
        return 0;
    }
    if credential_path_prefix(path, r"\\.\UNC") {
        return credential_unc_len(path, 8);
    }
    if [r"\\.", r"\\?", r"\??"]
        .iter()
        .any(|prefix| credential_path_prefix(path, prefix))
    {
        if path.len() == 3 {
            return 3;
        }
        return path[4..]
            .iter()
            .position(|unit| credential_path_separator(*unit))
            .map_or(path.len(), |index| index + 4);
    }
    if path.len() >= 2 && credential_path_separator(path[1]) {
        return credential_unc_len(path, 2);
    }
    0
}
#[cfg(windows)]
fn credential_path_prefix(path: &[u16], prefix: &str) -> bool {
    let prefix: Vec<_> = prefix.encode_utf16().collect();
    path.len() >= prefix.len()
        && path.iter().zip(&prefix).all(|(left, right)| {
            if credential_path_separator(*right) {
                credential_path_separator(*left)
            } else {
                credential_ascii_upper(*left) == credential_ascii_upper(*right)
            }
        })
        && (path.len() == prefix.len() || credential_path_separator(path[prefix.len()]))
}
#[cfg(windows)]
fn credential_ascii_upper(unit: u16) -> u16 {
    if (97..=122).contains(&unit) {
        unit - 32
    } else {
        unit
    }
}
#[cfg(windows)]
fn credential_unc_len(path: &[u16], prefix: usize) -> usize {
    path.iter()
        .enumerate()
        .skip(prefix)
        .filter(|(_, unit)| credential_path_separator(**unit))
        .nth(1)
        .map_or(path.len(), |(index, _)| index)
}
