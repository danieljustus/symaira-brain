use std::{
    fs,
    io::{BufRead, Read},
    path::{Path, PathBuf},
};

use chrono::{DateTime, FixedOffset, Utc};

use super::{ImportError, model::Time};

pub(super) fn path_bytes(path: &Path) -> Result<Vec<u8>, ImportError> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(path.as_os_str().as_bytes().to_vec())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(ImportError::Unsupported(
            "native non-Unix lexical path proof pending",
        ))
    }
}

pub(super) fn home() -> Result<PathBuf, ImportError> {
    #[cfg(unix)]
    {
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or(ImportError::Message("$HOME is not defined"))
    }
    #[cfg(not(unix))]
    {
        Err(ImportError::Unsupported(
            "native non-Unix HOME contract pending",
        ))
    }
}

pub(super) fn from_bytes(bytes: Vec<u8>) -> Result<PathBuf, ImportError> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(std::ffi::OsString::from_vec(bytes).into())
    }
    #[cfg(not(unix))]
    {
        let _ = bytes;
        Err(ImportError::Unsupported(
            "native non-Unix lexical path proof pending",
        ))
    }
}

/// Unix filepath.Clean, never filesystem canonicalization. A generated join
/// cleans `symlink/..` lexically before the kernel sees the spelling.
pub(super) fn clean(path: &Path) -> Result<PathBuf, ImportError> {
    let bytes = path_bytes(path)?;
    let rooted = bytes.starts_with(b"/");
    let mut parts: Vec<&[u8]> = Vec::new();
    for part in bytes.split(|b| *b == b'/') {
        match part {
            b"" | b"." => (),
            b".." if parts.last().is_some_and(|p| *p != b"..") => {
                parts.pop();
            }
            b".." if !rooted => parts.push(part),
            b".." => (),
            _ => parts.push(part),
        }
    }
    let mut result = if rooted { vec![b'/'] } else { Vec::new() };
    for part in parts {
        if !result.is_empty() && !result.ends_with(b"/") {
            result.push(b'/');
        }
        result.extend(part);
    }
    if result.is_empty() {
        result.push(b'.');
    }
    from_bytes(result)
}

pub(super) fn join(root: &Path, child: &[u8]) -> Result<PathBuf, ImportError> {
    let mut raw = path_bytes(root)?;
    if !raw.is_empty() {
        raw.push(b'/');
    }
    raw.extend(child);
    clean(&from_bytes(raw)?)
}

pub(super) fn relative(root: &Path, path: &Path) -> Result<Vec<u8>, ImportError> {
    let base = path_bytes(&clean(root)?)?;
    let target = path_bytes(&clean(path)?)?;
    if base.starts_with(b"/") != target.starts_with(b"/") {
        return Err(ImportError::Message(
            "relative path requires matching roots",
        ));
    }
    let parts = |b: &[u8]| {
        b.split(|c| *c == b'/')
            .filter(|p| !p.is_empty() && *p != b".")
            .map(<[u8]>::to_vec)
            .collect::<Vec<_>>()
    };
    let a = parts(&base);
    let b = parts(&target);
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if a[common..].iter().any(|p| p == b"..") {
        return Err(ImportError::Message(
            "relative base contains unresolved parent",
        ));
    }
    let mut out = vec![b"..".to_vec(); a.len() - common];
    out.extend_from_slice(&b[common..]);
    Ok(if out.is_empty() {
        b".".to_vec()
    } else {
        out.join(&b'/')
    })
}

pub(super) fn basename(path: &Path) -> Result<Vec<u8>, ImportError> {
    // Go Base removes trailing separators, but never Clean: explicit roots
    // and command tokens such as npm/. must keep their final dot component.
    let bytes = path_bytes(path)?;
    if bytes.is_empty() {
        return Ok(b".".to_vec());
    }
    let end = bytes
        .iter()
        .rposition(|byte| *byte != b'/')
        .map_or(0, |index| index + 1);
    if end == 0 {
        return Ok(b"/".to_vec());
    }
    Ok(bytes[..end]
        .rsplit(|b| *b == b'/')
        .next()
        .unwrap_or_default()
        .to_vec())
}

pub(super) fn modified(metadata: &fs::Metadata) -> Result<Time, ImportError> {
    let timestamp: DateTime<Utc> = metadata.modified()?.into();
    Ok(timestamp.with_timezone(&chrono::Local).fixed_offset())
}

pub(super) fn eligible(timestamp: Time, since: Option<Time>) -> bool {
    since.map_or(timestamp.timestamp() >= -62_135_596_800, |start| {
        timestamp >= start
    })
}

/// filepath.Walk visits names in byte order, Lstats links without descending
/// through them, and these importers' callbacks suppress filesystem errors.
pub(super) fn walk(root: &Path) -> Result<Vec<(PathBuf, fs::Metadata)>, ImportError> {
    path_bytes(root)?;
    let mut pending = vec![root.to_path_buf()];
    let mut result = Vec::new();
    while let Some(path) = pending.pop() {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            if let Ok(directory) = fs::read_dir(&path) {
                let mut names = directory
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name())
                    .collect::<Vec<_>>();
                names.sort();
                for name in names.into_iter().rev() {
                    let child = path_bytes(Path::new(&name))?;
                    pending.push(join(&path, &child)?);
                }
            }
        }
        result.push((path, metadata));
    }
    Ok(result)
}

pub(super) fn markdown(path: &Path) -> Result<Vec<u8>, ImportError> {
    let mut data = Vec::new();
    fs::File::open(path)?
        .take((1 << 20) + 1)
        .read_to_end(&mut data)?;
    data.truncate(1 << 20);
    Ok(data)
}

/// ScanLines with the Scanner buffer ceiling. A full buffer with no newline
/// fails before the next EOF read, even if that line is the final input.
pub(super) fn scan(
    reader: &mut impl BufRead,
    maximum: usize,
    mut line: impl FnMut(&[u8]),
) -> Result<(), ImportError> {
    let mut token = Vec::new();
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(error) => {
                // Scanner invokes ScanLines with atEOF on any reader error,
                // so an already buffered final token can still be emitted.
                if !token.is_empty() {
                    emit(&token, &mut line);
                }
                return Err(error.into());
            }
        };
        if available.is_empty() {
            if !token.is_empty() {
                emit(&token, &mut line);
            }
            return Ok(());
        }
        let newline = available.iter().position(|b| *b == b'\n');
        let length = newline.map_or(available.len(), |index| index + 1);
        if token.len() + length > maximum {
            return Err(ImportError::Message("bufio.Scanner: token too long"));
        }
        token.extend_from_slice(&available[..length]);
        reader.consume(length);
        if newline.is_some() {
            token.pop();
            emit(&token, &mut line);
            token.clear();
        } else if token.len() == maximum {
            return Err(ImportError::Message("bufio.Scanner: token too long"));
        }
    }
}

fn emit(token: &[u8], line: &mut impl FnMut(&[u8])) {
    line(token.strip_suffix(b"\r").unwrap_or(token));
}

pub(super) fn utc_seconds(time: Time) -> Vec<u8> {
    let rendered = time
        .with_timezone(&FixedOffset::east_opt(0).expect("zero offset"))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    // Chrono adds '+' above year 9999; Go Format does not. A valid 9999
    // resource can cross that boundary when its 6h end is computed.
    rendered
        .strip_prefix('+')
        .unwrap_or(&rendered)
        .as_bytes()
        .to_vec()
}
