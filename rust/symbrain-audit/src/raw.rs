//! Safe append-only writer for the legacy raw JSONL audit format.

#[cfg(unix)]
#[path = "raw_unix.rs"]
mod raw_unix;

#[cfg(windows)]
#[path = "raw_windows.rs"]
mod raw_windows;

use std::io;
use std::path::{Path, PathBuf};

/// Appends already-serialized JSON records as raw JSONL.
#[derive(Debug, Clone)]
pub struct RawJsonlAppender {
    path: PathBuf,
}

impl RawJsonlAppender {
    /// Creates an appender for `path` without touching the filesystem.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the configured target path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
impl RawJsonlAppender {
    /// Appends one raw JSONL record, preserving every input byte.
    ///
    /// The parent is opened component-by-component and retained as a
    /// capability. The target is then opened relative to that retained
    /// directory, so an ancestor cannot be swapped between validation and the
    /// write. A single `write` call is intentional: a short write is reported
    /// rather than silently producing a partial JSONL record.
    ///
    /// # Errors
    /// Returns an error when the record contains a line ending, a path
    /// component is unsafe, a parent cannot be created, or the write is short.
    pub fn append(&self, record: &[u8]) -> io::Result<()> {
        if record.contains(&b'\n') || record.contains(&b'\r') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "audit: raw record must be a single line",
            ));
        }
        let (parent_path, name) = raw_unix::split_target(&self.path)?;
        let parent = raw_unix::open_parent(&parent_path, true)?;
        raw_unix::append_to_parent(&parent, &name, record)
    }
}

#[cfg(windows)]
impl RawJsonlAppender {
    /// Appends one raw JSONL record on Windows, preserving every input byte.
    ///
    /// The parent is opened component-by-component with handle-relative
    /// nofollow traversal and retained as a directory capability. The target
    /// is opened relative to that retained directory with symlink following
    /// disabled. Device namespaces, alternate data streams (ADS), reserved DOS
    /// device names, and reparse points are rejected fail-closed.
    ///
    /// # Errors
    /// Returns an error when the record contains a line ending, a path
    /// component is unsafe, a parent cannot be created, the target is not
    /// a regular file, or the write is short.
    pub fn append(&self, record: &[u8]) -> io::Result<()> {
        if record.contains(&b'\n') || record.contains(&b'\r') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "audit: raw record must be a single line",
            ));
        }
        let (parent_path, name) = raw_windows::split_target(&self.path)?;
        let parent = raw_windows::open_parent(&parent_path, true)?;
        raw_windows::append_to_parent(&parent, &name, record)
    }
}

#[cfg(not(any(unix, windows)))]
impl RawJsonlAppender {
    /// Fails closed on platforms without safe directory capability append support.
    ///
    /// # Errors
    /// Always returns [`io::ErrorKind::Unsupported`].
    pub fn append(&self, _record: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "audit: raw JSONL append is unsupported on this platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use std::thread;
    use tempfile::tempdir;

    #[cfg(any(unix, windows))]
    #[test]
    fn appends_exact_bytes_without_rewriting_fields() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("audit.log");
        let appender = RawJsonlAppender::new(&path);
        let first = br#"{"b":2, "a":"<&\u2028"}"#;
        appender.append(first).expect("first append");
        appender
            .append(br#"{"received":null}"#)
            .expect("second append");
        let mut expected = first.to_vec();
        expected.extend_from_slice(b"\n");
        expected.extend_from_slice(br#"{"received":null}"#);
        expected.extend_from_slice(b"\n");
        assert_eq!(fs::read(&path).expect("read"), expected);
    }

    #[cfg(unix)]
    #[test]
    fn creates_all_new_directories_private_and_preserves_existing_modes() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().expect("tempdir");
        let existing = dir.path().join("existing");
        fs::create_dir(&existing).expect("existing");
        fs::set_permissions(&existing, fs::Permissions::from_mode(0o755)).expect("mode");
        let parent = existing.join("new").join("deeper");
        let path = parent.join("audit.log");
        RawJsonlAppender::new(&path)
            .append(br#"{"ok":true}"#)
            .expect("append");
        assert_eq!(
            fs::metadata(&existing).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(
            fs::metadata(existing.join("new"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&parent).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_synchronized_ancestor_swap() {
        use std::sync::Barrier;
        let root = tempdir().expect("root");
        let outside = tempdir().expect("outside");
        let live = root.path().join("live");
        fs::create_dir(&live).expect("live");
        let target = live.join("audit.log");
        let appender = RawJsonlAppender::new(&target);
        let barrier = Arc::new(Barrier::new(2));
        let swapper = thread::spawn({
            let live = live.clone();
            let outside = outside.path().to_path_buf();
            let barrier = Arc::clone(&barrier);
            move || {
                barrier.wait();
                let moved = root.path().join("moved");
                fs::rename(&live, &moved).expect("detach live");
                std::os::unix::fs::symlink(&outside, &live).expect("swap symlink");
                moved
            }
        });
        barrier.wait();
        swapper.join().expect("swap");
        let _ = appender.append(br#"{"safe":true}"#);
        assert!(!outside.path().join("audit.log").exists());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn bare_relative_path_uses_process_working_directory() {
        if std::env::var_os("SYMBRAIN_RAW_RELATIVE_CHILD").is_some() {
            RawJsonlAppender::new("audit.log")
                .append(br#"{"relative":true}"#)
                .expect("child append");
            return;
        }
        let dir = tempdir().expect("tempdir");
        let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "bare_relative_path_uses_process_working_directory",
                "--nocapture",
            ])
            .current_dir(dir.path())
            .env("SYMBRAIN_RAW_RELATIVE_CHILD", "1")
            .status()
            .expect("child status");
        assert!(status.success());
        assert_eq!(
            fs::read(dir.path().join("audit.log")).unwrap(),
            b"{\"relative\":true}\n"
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn rejects_line_breaks_and_unsafe_paths() {
        let dir = tempdir().expect("tempdir");
        assert!(
            RawJsonlAppender::new(dir.path().join("audit.log"))
                .append(b"{}\n{}")
                .is_err()
        );
        assert!(
            RawJsonlAppender::new(dir.path().join("audit.log"))
                .append(b"{}\r{}")
                .is_err()
        );
        assert!(
            RawJsonlAppender::new(dir.path().join("../audit.log"))
                .append(b"{}")
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_regular_targets_without_blocking() {
        use std::os::unix::net::UnixListener;
        use std::time::{Duration, Instant};

        let dir = tempfile::tempdir_in("/tmp").expect("tempdir");
        let fifo = dir.path().join("audit.fifo");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .current_dir(dir.path())
            .env_clear()
            .status()
            .expect("mkfifo status");
        assert!(status.success());
        let started = Instant::now();
        let fifo_result = RawJsonlAppender::new(&fifo).append(br#"{{"fifo":true}}"#);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(fifo_result.is_err());

        let socket = dir.path().join("audit.sock");
        let _listener = UnixListener::bind(&socket).expect("socket");
        let started = Instant::now();
        let socket_result = RawJsonlAppender::new(&socket).append(br#"{{"socket":true}}"#);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(socket_result.is_err());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn concurrent_appends_are_complete_jsonl_records() {
        let dir = tempdir().expect("tempdir");
        let appender = Arc::new(RawJsonlAppender::new(dir.path().join("audit.log")));
        appender
            .append(br#"{"warmup":true}"#)
            .expect("warmup append");
        let mut workers = Vec::new();
        for index in 0..8 {
            let appender = Arc::clone(&appender);
            workers.push(thread::spawn(move || {
                for sequence in 0..16 {
                    let record = format!(r#"{{"worker":{index},"sequence":{sequence}}}"#);
                    appender.append(record.as_bytes()).expect("append");
                }
            }));
        }
        for worker in workers {
            worker.join().expect("join");
        }
        let data = fs::read_to_string(appender.path()).expect("read");
        assert_eq!(data.lines().count(), 8 * 16 + 1);
        for line in data.lines() {
            serde_json::from_str::<serde_json::Value>(line).expect("json");
        }
    }
}
