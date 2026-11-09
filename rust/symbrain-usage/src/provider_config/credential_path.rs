// Lexical credential joins follow Go filepath, never filesystem canonicalize.
// Based on Go's BSD-3-Clause filepathlite Clean and filepath Join algorithms;
// see migration/licenses/go-filepath-bsd.txt. Preserve native OS path units.
fn credential_join(base: &Path, child: &str) -> PathBuf {
    let mut path = credential_path_units(base);
    let tail = credential_path_units(Path::new(child));
    if !path.is_empty() {
        path.push(credential_path_sep());
    }
    path.extend(tail);
    if path.is_empty() {
        return PathBuf::new();
    }
    credential_path_from_units(&credential_clean_units(&path))
}

fn credential_path_units(path: &Path) -> Vec<u16> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str()
        .as_bytes()
        .iter()
        .copied()
        .map(u16::from)
        .collect()
}
fn credential_path_from_units(path: &[u16]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(
        path.iter()
            .map(|unit| u8::try_from(*unit).expect("preserved Unix byte"))
            .collect(),
    )
    .into()
}
fn credential_path_sep() -> u16 {
    47
}
fn credential_path_separator(unit: u16) -> bool {
    unit == 47
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
    let mut result = original[..volume].to_vec();
    result.extend(output);
    credential_from_slash(&mut result);
    result
}
fn credential_from_slash(_path: &mut [u16]) {
    }
fn credential_volume_len(_: &[u16]) -> usize {
    0
}
