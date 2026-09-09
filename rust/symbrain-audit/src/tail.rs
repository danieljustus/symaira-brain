use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::{Degradation, Entry};

const TAIL_CHUNK_SIZE: usize = 64 * 1024;

/// Reads the newest matching entries from a JSONL file in bounded reverse chunks.
/// Results are returned oldest-first. Chained envelopes intentionally decode as
/// zero-valued entries, matching the current Go reader contract.
///
/// # Errors
/// Returns an error when the file cannot be opened, sought, or read.
pub fn tail_entries_bounded<F>(
    path: &Path,
    limit: usize,
    mut filter: F,
    session_scoped: bool,
) -> io::Result<Vec<Entry>>
where
    F: FnMut(&Entry) -> bool,
{
    let mut file = File::open(path)?;
    let mut offset = file.metadata()?.len();
    if offset == 0 {
        return Ok(Vec::new());
    }

    let mut carry = Vec::new();
    let mut latest_session = String::new();
    let mut results = Vec::new();
    let mut stopped = false;

    while offset > 0 && !stopped {
        let read_size = usize::try_from(offset.min(TAIL_CHUNK_SIZE as u64))
            .map_err(|_| io::Error::other("audit: tail chunk does not fit usize"))?;
        offset -= read_size as u64;
        file.seek(SeekFrom::Start(offset))?;
        let mut chunk = vec![0; read_size];
        file.read_exact(&mut chunk)?;
        chunk.extend_from_slice(&carry);
        carry.clear();

        let mut line_end = chunk.len();
        for index in (0..chunk.len()).rev() {
            if chunk[index] != b'\n' {
                continue;
            }
            if consume_line(
                &chunk[index + 1..line_end],
                limit,
                &mut filter,
                session_scoped,
                &mut latest_session,
                &mut results,
            ) {
                stopped = true;
                break;
            }
            line_end = index;
        }
        if !stopped && line_end > 0 {
            carry.extend_from_slice(&chunk[..line_end]);
        }
    }

    if !stopped && !carry.is_empty() {
        let _ = consume_line(
            &carry,
            limit,
            &mut filter,
            session_scoped,
            &mut latest_session,
            &mut results,
        );
    }
    results.reverse();
    Ok(results)
}

fn consume_line<F>(
    line: &[u8],
    limit: usize,
    filter: &mut F,
    session_scoped: bool,
    latest_session: &mut String,
    results: &mut Vec<Entry>,
) -> bool
where
    F: FnMut(&Entry) -> bool,
{
    if line.is_empty() {
        return false;
    }
    let Ok(entry) = serde_json::from_slice::<Entry>(line) else {
        return false;
    };
    if session_scoped {
        if latest_session.is_empty() {
            if entry.session_id.is_empty() {
                return false;
            }
            latest_session.clone_from(&entry.session_id);
        }
        if entry.session_id != *latest_session {
            return true;
        }
    }
    if filter(&entry) {
        results.push(entry);
        return limit > 0 && results.len() >= limit;
    }
    false
}

/// Returns the paths selected by Go's profile/all-profile discovery rules.
///
/// # Errors
/// Returns an error when all-profile directory discovery fails.
pub fn audit_log_paths(dir: &Path, profile: &str) -> io::Result<Vec<PathBuf>> {
    if !profile.is_empty() {
        return Ok(vec![dir.join(format!("{profile}.jsonl"))]);
    }
    let mut paths = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            (path.is_file() && path.extension().is_some_and(|ext| ext == "jsonl")).then_some(path)
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

/// Reads up to `limit` entries per selected profile log, matching Go behavior.
///
/// # Errors
/// Returns an error when profile-log discovery fails.
pub fn tail_entries_in(dir: &Path, profile: &str, limit: usize) -> io::Result<Vec<Entry>> {
    let mut result = Vec::new();
    for path in audit_log_paths(dir, profile)? {
        let Ok(entries) = tail_entries_bounded(&path, limit, |_| true, false) else {
            continue;
        };
        let profile_name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        result.extend(entries.into_iter().map(|mut entry| {
            entry.profile = profile_name.to_string();
            entry
        }));
    }
    Ok(result)
}

/// Returns degradation records from the latest session in every selected log.
///
/// # Errors
/// Returns an error when discovery or reading an existing selected log fails.
pub fn latest_degradations_in(dir: &Path, profile: &str) -> io::Result<Vec<Degradation>> {
    let paths = if profile.is_empty() {
        if !dir.exists() {
            return Ok(Vec::new());
        }
        audit_log_paths(dir, "")?
    } else {
        vec![dir.join(format!("{profile}.jsonl"))]
    };
    let mut result = Vec::new();
    for path in paths {
        let entries = match tail_entries_bounded(&path, 0, |entry| entry.status == "degraded", true)
        {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        result.extend(entries.into_iter().map(|entry| Degradation {
            session_id: entry.session_id,
            profile: entry.profile,
            server: entry.server,
            reason: entry.reason,
            level: entry.level,
        }));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::tempdir;

    use super::*;

    fn line(session: &str, reason: &str) -> String {
        serde_json::to_string(&Entry {
            session_id: session.to_string(),
            profile: "p".to_string(),
            server: "memory".to_string(),
            status: "degraded".to_string(),
            reason: reason.to_string(),
            ..Entry::default()
        })
        .expect("serialize")
    }

    #[test]
    fn reverse_reader_preserves_chunk_boundaries_and_latest_session() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("p.jsonl");
        let mut file = File::create(&path).expect("create");
        for index in 0..500 {
            writeln!(file, "{}", line("old", &format!("old-{index}"))).expect("write");
        }
        for index in 0..700 {
            writeln!(file, "{}", line("latest", &format!("new-{index}"))).expect("write");
        }
        let entries = tail_entries_bounded(&path, 0, |_| true, true).expect("tail");
        assert_eq!(entries.len(), 700);
        assert_eq!(entries.first().expect("first").reason, "new-0");
        assert_eq!(entries.last().expect("last").reason, "new-699");
    }

    #[test]
    fn limit_returns_most_recent_in_chronological_order() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("p.jsonl");
        let mut content = String::new();
        for index in 0..20 {
            use std::fmt::Write as _;
            writeln!(content, "{}", line("s", &format!("r{index}"))).expect("render");
        }
        fs::write(&path, content).expect("write");
        let entries = tail_entries_bounded(&path, 5, |_| true, true).expect("tail");
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].reason, "r15");
        assert_eq!(entries[4].reason, "r19");
    }
}
