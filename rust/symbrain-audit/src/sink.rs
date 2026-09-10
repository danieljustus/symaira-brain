use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DEFAULT_MAX_BYTES: u64 = 10 << 20;
const SEGMENT_ANCHOR_SCHEMA: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct ChainedEntry {
    d: String,
    h: String,
}

/// Authenticated metadata linking a rotated segment to the preceding segment.
///
/// The anchor is kept next to the rotated file rather than in the JSONL stream,
/// so existing tail readers continue to see only audit entries. Its byte and
/// chain metadata also lets recovery distinguish a complete rotated segment
/// from a stale or partially committed rotation.
#[derive(Debug, Serialize, Deserialize)]
struct SegmentAnchor {
    schema_version: u32,
    previous_head: String,
    head: String,
    entry_count: u64,
    total_count: u64,
    log_size: u64,
    content_hash: String,
}

#[derive(Debug, Clone)]
struct RecoveredSegment {
    head_hash: String,
    entries: u64,
    valid_len: usize,
}

/// Append-only, hash-chained JSONL sink compatible with corekit/auditkit.
///
/// Rotated segments retain their global chain link in a durable sidecar
/// anchor (`<path>.1.anchor`). Only the current segment and one rotated segment
/// are retained, matching the Go sink's bounded-file contract.
pub struct Sink {
    path: PathBuf,
    writer: Option<BufWriter<File>>,
    count: u64,
    head_hash: String,
    segment_start_hash: String,
    segment_count: u64,
    max_bytes: u64,
    degraded: bool,
}

impl Sink {
    /// Opens a sink and validates/reconstructs its chain state.
    ///
    /// # Errors
    /// Returns an error when the file cannot be opened or its retained chain is invalid.
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        Self::open_with_max_bytes(path, DEFAULT_MAX_BYTES)
    }

    /// Opens a sink with an explicit rotation threshold.
    ///
    /// # Errors
    /// Returns an error when the file cannot be opened or its retained chain is invalid.
    pub fn open_with_max_bytes(path: impl Into<PathBuf>, max_bytes: u64) -> io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            create_private_dir(parent)?;
        }
        let file = open_private_append(&path)?;
        let mut sink = Self {
            path,
            writer: Some(BufWriter::new(file)),
            count: 0,
            head_hash: String::new(),
            segment_start_hash: String::new(),
            segment_count: 0,
            max_bytes,
            degraded: false,
        };
        sink.recover_state()?;
        Ok(sink)
    }

    fn recover_state(&mut self) -> io::Result<()> {
        let rotated = rotated_path(&self.path)?;
        let anchor_path = segment_anchor_path(&rotated)?;
        let temp_anchor_path = segment_anchor_temp_path(&anchor_path)?;
        let (base_hash, base_count) = if rotated.exists() {
            let (anchor, from_temp) =
                recover_rotated_segment(&rotated, &anchor_path, &temp_anchor_path)?;
            if from_temp {
                replace_path(&temp_anchor_path, &anchor_path)?;
                sync_directory(anchor_path.parent())?;
            }
            (anchor.head, anchor.total_count)
        } else {
            if anchor_path.exists() {
                return Err(invalid_data("rotated segment anchor has no segment"));
            }
            if temp_anchor_path.exists() {
                fs::remove_file(&temp_anchor_path)?;
            }
            (String::new(), 0)
        };

        let data = fs::read(&self.path)?;
        let recovered = parse_segment(&data, &base_hash, true)?;
        if recovered.valid_len < data.len() {
            truncate_current(&self.path, recovered.valid_len)?;
        }
        self.segment_start_hash = base_hash;
        self.segment_count = recovered.entries;
        self.count = base_count + recovered.entries;
        self.head_hash = recovered.head_hash;
        Ok(())
    }

    /// Appends one payload line and flushes it to durable storage.
    ///
    /// # Errors
    /// Returns an error for multiline payloads, failed writes, flushes, or rotation.
    pub fn append(&mut self, line: &str) -> io::Result<()> {
        if line.contains(['\n', '\r']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "auditkit: entry must be a single line",
            ));
        }
        if self.degraded {
            return Err(io::Error::other("auditkit: sink degraded"));
        }
        let hash = hash_entry(line, &self.head_hash);
        let envelope = go_json(&ChainedEntry {
            d: line.to_string(),
            h: hash.clone(),
        })?;
        let Some(writer) = self.writer.as_mut() else {
            self.degraded = true;
            return Err(io::Error::other("auditkit: sink closed"));
        };
        if let Err(error) = writeln!(writer, "{envelope}").and_then(|()| writer.flush()) {
            self.degraded = true;
            return Err(error);
        }
        self.count += 1;
        self.segment_count += 1;
        self.head_hash = hash;
        self.maybe_rotate()
    }

    fn maybe_rotate(&mut self) -> io::Result<()> {
        let metadata = match fs::metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.degraded = true;
                return Err(error);
            }
        };
        if metadata.len() < self.max_bytes {
            return Ok(());
        }

        if let Err(error) = self.flush_durable() {
            self.degraded = true;
            return Err(error);
        }
        let data = match fs::read(&self.path) {
            Ok(data) => data,
            Err(error) => {
                self.degraded = true;
                return Err(error);
            }
        };
        let anchor = SegmentAnchor {
            schema_version: SEGMENT_ANCHOR_SCHEMA,
            previous_head: self.segment_start_hash.clone(),
            head: self.head_hash.clone(),
            entry_count: self.segment_count,
            total_count: self.count,
            log_size: data.len() as u64,
            content_hash: hash_bytes(&data),
        };
        let rotated = rotated_path(&self.path)?;
        let anchor_path = segment_anchor_path(&rotated)?;
        let temp_anchor_path = segment_anchor_temp_path(&anchor_path)?;
        if let Err(error) = write_segment_anchor(&temp_anchor_path, &anchor) {
            self.degraded = true;
            return Err(error);
        }

        // The temporary anchor is durable before the log is renamed. If the
        // process dies between these two renames, recovery can complete the
        // pair from the temporary anchor without accepting an invalid chain.
        if let Some(writer) = self.writer.take() {
            drop(writer);
        }
        if let Err(error) =
            replace_path(&self.path, &rotated).and_then(|()| sync_directory(self.path.parent()))
        {
            self.degraded = true;
            return Err(error);
        }
        if let Err(error) = replace_path(&temp_anchor_path, &anchor_path)
            .and_then(|()| sync_directory(anchor_path.parent()))
        {
            self.degraded = true;
            return Err(error);
        }

        self.writer = match open_private_append(&self.path) {
            Ok(file) => Some(BufWriter::new(file)),
            Err(error) => {
                self.degraded = true;
                return Err(error);
            }
        };
        self.segment_start_hash = self.head_hash.clone();
        self.segment_count = 0;
        Ok(())
    }

    fn flush_durable(&mut self) -> io::Result<()> {
        let Some(writer) = self.writer.as_mut() else {
            return Err(io::Error::other("auditkit: sink closed"));
        };
        writer.flush()?;
        writer.get_ref().sync_data()
    }

    /// Flushes and closes the sink. Repeated calls are harmless.
    ///
    /// # Errors
    /// Returns an error when buffered bytes cannot be flushed durably.
    pub fn close(&mut self) -> io::Result<()> {
        if self.writer.is_none() {
            return Ok(());
        }
        if let Err(error) = self.flush_durable() {
            self.degraded = true;
            return Err(error);
        }
        self.writer.take();
        Ok(())
    }

    #[must_use]
    pub const fn count(&self) -> u64 {
        self.count
    }

    #[must_use]
    pub fn head_hash(&self) -> &str {
        &self.head_hash
    }
}

fn recover_rotated_segment(
    rotated: &Path,
    anchor_path: &Path,
    temp_anchor_path: &Path,
) -> io::Result<(SegmentAnchor, bool)> {
    let data = fs::read(rotated)?;
    let mut last_error = None;
    for (candidate, from_temp) in [(anchor_path, false), (temp_anchor_path, true)] {
        if !candidate.exists() {
            continue;
        }
        let anchor = match read_segment_anchor(candidate) {
            Ok(anchor) => anchor,
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        };
        if anchor.schema_version != SEGMENT_ANCHOR_SCHEMA
            || anchor.entry_count == 0
            || anchor.total_count < anchor.entry_count
            || anchor.head.is_empty()
            || anchor.log_size != data.len() as u64
            || anchor.content_hash != hash_bytes(&data)
        {
            last_error = Some(invalid_data(
                "rotated segment anchor does not match segment",
            ));
            continue;
        }
        let recovered = match parse_segment(&data, &anchor.previous_head, false) {
            Ok(recovered) => recovered,
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        };
        if recovered.entries != anchor.entry_count || recovered.head_hash != anchor.head {
            last_error = Some(invalid_data("rotated segment chain does not match anchor"));
            continue;
        }
        return Ok((anchor, from_temp));
    }
    Err(last_error.unwrap_or_else(|| invalid_data("rotated segment anchor is missing")))
}

fn read_segment_anchor(path: &Path) -> io::Result<SegmentAnchor> {
    let data = fs::read(path)?;
    serde_json::from_slice(&data).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("auditkit: parse segment anchor: {error}"),
        )
    })
}

fn write_segment_anchor(path: &Path, anchor: &SegmentAnchor) -> io::Result<()> {
    let mut data = serde_json::to_vec(anchor).map_err(io::Error::other)?;
    data.push(b'\n');
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(&data)?;
    file.sync_all()?;
    Ok(())
}

fn parse_segment(
    data: &[u8],
    initial_hash: &str,
    allow_torn_tail: bool,
) -> io::Result<RecoveredSegment> {
    let mut offset = 0;
    let mut entries = 0;
    let mut head_hash = initial_hash.to_string();
    for part in data.split_inclusive(|byte| *byte == b'\n') {
        let complete = part.last() == Some(&b'\n');
        if !complete {
            if allow_torn_tail {
                break;
            }
            return Err(invalid_data("rotated segment has a truncated final line"));
        }
        let line = &part[..part.len() - 1];
        if line.is_empty() {
            offset += part.len();
            continue;
        }
        let entry: ChainedEntry = serde_json::from_slice(line).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("auditkit: invalid chain entry: {error}"),
            )
        })?;
        if entry.h.is_empty() || hash_entry(&entry.d, &head_hash) != entry.h {
            return Err(invalid_data("auditkit: invalid chain link during recovery"));
        }
        entries += 1;
        head_hash = entry.h;
        offset += part.len();
    }
    Ok(RecoveredSegment {
        head_hash,
        entries,
        valid_len: offset,
    })
}

fn truncate_current(path: &Path, length: usize) -> io::Result<()> {
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_len(length as u64)?;
    file.sync_all()
}

fn rotated_path(path: &Path) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| invalid_data("auditkit: audit path has no file name"))?;
    Ok(path.with_file_name(format!("{}.1", name.to_string_lossy())))
}

fn segment_anchor_path(rotated: &Path) -> io::Result<PathBuf> {
    Ok(rotated.with_file_name(format!(
        "{}.anchor",
        rotated
            .file_name()
            .ok_or_else(|| invalid_data("auditkit: rotated path has no file name"))?
            .to_string_lossy()
    )))
}

fn segment_anchor_temp_path(anchor: &Path) -> io::Result<PathBuf> {
    Ok(anchor.with_file_name(format!(
        "{}.tmp",
        anchor
            .file_name()
            .ok_or_else(|| invalid_data("auditkit: anchor path has no file name"))?
            .to_string_lossy()
    )))
}

fn replace_path(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(to)?;
            fs::rename(from, to)
        }
        Err(error) => Err(error),
    }
}

fn sync_directory(path: Option<&Path>) -> io::Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    #[cfg(not(windows))]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(windows)]
    {
        let _ = path;
        Ok(())
    }
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Computes the SHA-256 chain hash used by corekit/auditkit.
#[must_use]
pub fn hash_entry(data: &str, previous_hash: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(previous_hash.as_bytes());
    hasher.update(data.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

pub(crate) fn go_json<T: Serialize>(value: &T) -> io::Result<String> {
    serde_json::to_string(value)
        .map(|json| {
            json.replace('&', "\\u0026")
                .replace('<', "\\u003c")
                .replace('>', "\\u003e")
                .replace('\u{2028}', "\\u2028")
                .replace('\u{2029}', "\\u2029")
        })
        .map_err(io::Error::other)
}

fn open_private_append(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).write(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    let existed = path.exists();
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !existed && fs::metadata(path)?.permissions().mode() & 0o777 != 0o700 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn append_entries(sink: &mut Sink, count: usize) {
        for index in 0..count {
            sink.append(&format!(
                r#"{{"n":{index},"payload":"{}"}}"#,
                "x".repeat(30)
            ))
            .expect("append");
        }
    }

    fn segment_paths(path: &Path) -> (PathBuf, PathBuf) {
        let rotated = rotated_path(path).expect("rotated path");
        let anchor = segment_anchor_path(&rotated).expect("anchor path");
        (rotated, anchor)
    }

    fn corrupt_first_hash(mut data: Vec<u8>) -> Vec<u8> {
        let marker = b"\"h\":\"";
        let start = data
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("hash marker")
            + marker.len();
        data[start] = if data[start] == b'0' { b'1' } else { b'0' };
        data
    }

    #[test]
    fn hash_matches_known_go_vector() {
        assert_eq!(
            hash_entry("hello", ""),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn sink_recovers_and_extends_chain() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("audit.jsonl");
        let mut sink = Sink::open(&path).expect("open");
        sink.append(r#"{"one":1}"#).expect("append");
        let first_hash = sink.head_hash().to_string();
        sink.close().expect("close");

        let mut reopened = Sink::open(&path).expect("reopen");
        assert_eq!(reopened.count(), 1);
        assert_eq!(reopened.head_hash(), first_hash);
        reopened.append(r#"{"two":2}"#).expect("append");
        assert_eq!(reopened.count(), 2);
    }

    #[test]
    fn single_rotation_from_genesis_reopens() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("audit.jsonl");
        let mut sink = Sink::open_with_max_bytes(&path, 180).expect("open");
        append_entries(&mut sink, 2);
        let count = sink.count();
        let head = sink.head_hash().to_string();
        sink.close().expect("close");

        let reopened = Sink::open_with_max_bytes(&path, 180).expect("reopen");
        assert_eq!(reopened.count(), count);
        assert_eq!(reopened.head_hash(), head);
    }

    #[test]
    fn multiple_rotations_reopen_and_continue_chain() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("audit.jsonl");
        let mut sink = Sink::open_with_max_bytes(&path, 180).expect("open");
        append_entries(&mut sink, 20);
        let count = sink.count();
        let head = sink.head_hash().to_string();
        let (rotated, anchor) = segment_paths(&path);
        assert!(rotated.exists());
        assert!(anchor.exists());
        sink.close().expect("close");

        let mut reopened = Sink::open_with_max_bytes(&path, 180).expect("reopen");
        assert_eq!(reopened.count(), count);
        assert_eq!(reopened.head_hash(), head);
        reopened
            .append(r#"{"after_restart":true}"#)
            .expect("append");
        assert_eq!(reopened.count(), count + 1);
        reopened.close().expect("close");
        let reopened_again = Sink::open_with_max_bytes(&path, 180).expect("reopen again");
        assert_eq!(reopened_again.count(), count + 1);
    }

    #[test]
    fn current_torn_tail_is_removed_before_recovery() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("audit.jsonl");
        let mut sink = Sink::open(&path).expect("open");
        sink.append(r#"{"one":1}"#).expect("append");
        sink.append(r#"{"two":2}"#).expect("append");
        sink.close().expect("close");
        let mut file = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("append torn");
        file.write_all(br#"{"d":"torn""#).expect("write torn");
        file.sync_all().expect("sync torn");

        let mut reopened = Sink::open(&path).expect("recover torn current");
        assert_eq!(reopened.count(), 2);
        reopened
            .append(r#"{"three":3}"#)
            .expect("append after recovery");
        reopened.close().expect("close");
        let data = fs::read(&path).expect("read");
        assert_eq!(data.split(|byte| *byte == b'\n').count() - 1, 3);
    }

    #[test]
    fn rotation_metadata_failure_degrades_sink() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("audit.jsonl");
        let mut sink = Sink::open_with_max_bytes(&path, 1).expect("open");
        fs::remove_file(&path).expect("remove active log");
        let error = sink
            .append(r#"{"event":true}"#)
            .expect_err("missing log accepted");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(sink.degraded);
        assert!(sink.append(r#"{"event":false}"#).is_err());
    }

    #[test]
    fn corruption_in_rotated_segment_fails_closed() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("audit.jsonl");
        let mut sink = Sink::open_with_max_bytes(&path, 180).expect("open");
        append_entries(&mut sink, 8);
        sink.close().expect("close");
        let (rotated, _) = segment_paths(&path);
        let original = fs::read(&rotated).expect("read rotated");
        let mutated = corrupt_first_hash(original.clone());
        assert_ne!(mutated, original);
        fs::write(&rotated, mutated).expect("corrupt rotated");
        let Err(error) = Sink::open_with_max_bytes(&path, 180) else {
            panic!("corrupt segment accepted");
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn corruption_in_current_segment_fails_closed() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("audit.jsonl");
        let mut sink = Sink::open(&path).expect("open");
        sink.append(r#"{"one":1}"#).expect("append");
        sink.append(r#"{"two":2}"#).expect("append");
        sink.close().expect("close");
        let original = fs::read(&path).expect("read current");
        let mutated = corrupt_first_hash(original.clone());
        assert_ne!(mutated, original);
        fs::write(&path, mutated).expect("corrupt current");
        let Err(error) = Sink::open(&path) else {
            panic!("corrupt current accepted");
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[cfg(unix)]
    #[test]
    fn sink_creates_private_directory_and_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().expect("tempdir");
        let audit = dir.path().join("nested").join("audit");
        let path = audit.join("p.jsonl");
        let _sink = Sink::open(&path).expect("open");
        assert_eq!(
            fs::metadata(audit).expect("dir").permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(path).expect("file").permissions().mode() & 0o777,
            0o600
        );
    }
}
