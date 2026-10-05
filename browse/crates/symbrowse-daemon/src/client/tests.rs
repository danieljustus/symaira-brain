use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

const UNIX_SOCKET_PATH_LIMIT: usize = if cfg!(target_os = "macos") { 104 } else { 108 };
static TEST_SOCKET_COUNTER: AtomicU64 = AtomicU64::new(0);

fn test_socket_path() -> PathBuf {
    let counter = TEST_SOCKET_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = PathBuf::from(format!("/tmp/sb-{}-{counter}.sock", std::process::id()));
    let length = path.as_os_str().as_encoded_bytes().len();
    assert!(
        length < UNIX_SOCKET_PATH_LIMIT,
        "test socket path is {length} bytes, must be shorter than {UNIX_SOCKET_PATH_LIMIT}"
    );
    path
}

#[test]
fn unavailable_errors_omit_secret_like_transport_text() {
    let error = unavailable(
        &ClientOptions::default(),
        io::Error::other("password=hidden"),
    );
    assert!(!error.to_string().contains("hidden"));
}

#[cfg(target_os = "linux")]
#[test]
fn unix_connect_deadline_handles_saturated_backlog() {
    use std::os::unix::net::UnixListener;

    let path = test_socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("bind test listener");
    nix::sys::socket::listen(&listener, nix::sys::socket::Backlog::new(0).unwrap())
        .expect("set a zero-length Unix listen backlog");
    // A full Unix-domain listen queue is allowed to accept a second local
    // connection immediately on some kernels and to report EINPROGRESS on
    // others. Both outcomes preserve the client contract; only an
    // unexpected error is a regression.
    let pending = connect_unix(&path, Duration::from_secs(1)).expect("fill listen backlog");
    let timeout = Duration::from_millis(50);
    let started = Instant::now();
    match connect_unix(&path, timeout) {
        Ok(stream) => drop(stream),
        Err(error) => assert_eq!(error.kind(), io::ErrorKind::TimedOut),
    }
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "saturated-backlog connection blocked for {:?}",
        started.elapsed()
    );
    drop(pending);
    drop(listener);
    let _ = std::fs::remove_file(path);
}

#[test]
fn unix_connect_succeeds_for_listening_socket() {
    use std::os::unix::net::UnixListener;

    let path = test_socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("bind test listener");
    let stream = connect_unix(&path, Duration::from_secs(1)).expect("connect to listener");
    drop(stream);
    drop(listener);
    let _ = std::fs::remove_file(path);
}

#[test]
fn terminate_child_reaps_within_bounded_cleanup_window() {
    let mut child = Command::new("/bin/sh")
        .args(["-c", "sleep 10"])
        .spawn()
        .expect("spawn controllable child");
    terminate_child(&mut child);
    assert!(
        child
            .try_wait()
            .expect("check child after bounded cleanup")
            .is_some(),
        "killed child should be reaped by the bounded cleanup loop"
    );
}
