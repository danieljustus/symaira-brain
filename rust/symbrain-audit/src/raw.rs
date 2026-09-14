//! Safe append-only writer for the legacy raw JSONL audit format.

#[cfg(unix)]
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
#[cfg(unix)]
use cap_std::fs::{Dir, DirBuilder, DirBuilderExt, OpenOptions, OpenOptionsExt};
use std::io;
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::path::Component;
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
    #[cfg(unix)]
    pub fn append(&self, record: &[u8]) -> io::Result<()> {
        if record.contains(&b'\n') || record.contains(&b'\r') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "audit: raw record must be a single line",
            ));
        }
        let (parent_path, name) = split_target(&self.path)?;
        let parent = open_parent(&parent_path, true)?;
        append_to_parent(&parent, &name, record)
    }
}

#[cfg(not(unix))]
impl RawJsonlAppender {
    /// Fails closed because the safe append implementation is Unix-only.
    ///
    /// This returns before inspecting the record or touching the filesystem.
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

impl RawJsonlAppender {
    /// Returns the configured target path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
fn append_to_parent(parent: &Dir, name: &Path, record: &[u8]) -> io::Result<()> {
    let mut data = Vec::with_capacity(record.len() + 1);
    data.extend_from_slice(record);
    data.push(b'\n');
    let mut options = OpenOptions::new();
    options.create(true).append(true).write(true);
    options.follow(FollowSymlinks::No);
    #[cfg(unix)]
    options.mode(0o600);
    options.nonblock(true);
    let mut file = parent.open_with(name, &options)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: target must be a regular file",
        ));
    }
    let written = file.write(&data)?;
    if written != data.len() {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            format!("audit: short append write: {written}/{} bytes", data.len()),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn split_target(path: &Path) -> io::Result<(PathBuf, PathBuf)> {
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: target path must name a file",
        )
    })?;
    if matches!(name.to_str(), Some("." | "..")) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "audit: target path must name a file",
        ));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    Ok((parent.to_path_buf(), PathBuf::from(name)))
}

#[cfg(unix)]
#[allow(clippy::collapsible_if)]
fn open_parent(path: &Path, create: bool) -> io::Result<Dir> {
    let path = physical_path(path);
    let mut current = if path.is_absolute() {
        Dir::open_ambient_dir(Path::new("/"), cap_std::ambient_authority())?
    } else {
        Dir::open_ambient_dir(Path::new("."), cap_std::ambient_authority())?
    };
    for component in path.components() {
        let name = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => name,
            Component::ParentDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "audit: path contains an unsafe component",
                ));
            }
        };
        if create {
            let mut builder = DirBuilder::new();
            builder.mode(0o700);
            match current.create_dir_with(name, &builder) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        if let Ok(metadata) = current.symlink_metadata(name) {
            if metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("audit: symlink path component: {}", name.display()),
                ));
            }
        }
        let next = current.open_dir_nofollow(name)?;
        current = next;
    }
    Ok(current)
}

#[cfg(all(unix, target_os = "macos"))]
fn physical_path(path: &Path) -> PathBuf {
    if path == Path::new("/var") || path.starts_with("/var/") {
        PathBuf::from("/private").join(path.strip_prefix("/").unwrap_or(path))
    } else {
        path.to_path_buf()
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn physical_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use std::thread;
    use tempfile::tempdir;

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

    #[test]
    fn rejects_line_breaks_and_unsafe_paths() {
        let dir = tempdir().expect("tempdir");
        assert!(
            RawJsonlAppender::new(dir.path().join("audit.log"))
                .append(b"{}\n{}")
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

#[cfg(all(test, not(unix)))]
mod non_unix_tests {
    use super::RawJsonlAppender;
    use std::io::ErrorKind;

    #[test]
    fn append_fails_closed_before_filesystem_access() {
        let error = RawJsonlAppender::new("definitely/not/created/audit.log")
            .append(b"invalid\nrecord")
            .expect_err("non-Unix append must be unsupported");
        assert_eq!(error.kind(), ErrorKind::Unsupported);
    }
}
