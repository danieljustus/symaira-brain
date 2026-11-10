use super::*;
const STALE_SOCKET_PROBE_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SocketIdentity {
    device: u64,
    inode: u64,
}

pub(super) fn socket_identity(metadata: &fs::Metadata) -> SocketIdentity {
    SocketIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

pub(super) fn socket_identity_at_path(path: &Path) -> io::Result<SocketIdentity> {
    fs::symlink_metadata(path).map(|metadata| socket_identity(&metadata))
}

pub(super) fn remove_owned_socket(path: &Path, owner: SocketIdentity) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() && socket_identity(&metadata) == owner => {
            fs::remove_file(path)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

impl Server {
    pub(super) fn listen_unix(&self) -> Result<(), ServerError> {
        use std::os::unix::net::UnixListener;
        let parent = self
            .options
            .socket_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        crate::session::create_directory(parent)?;
        let lock_path = self.options.socket_path.with_extension("sock.lock");
        let lock = acquire_lock(&lock_path)?;
        self.registry
            .ensure(&self.options.session)
            .map_err(|error| io::Error::other(error.to_string()))?;
        prepare_socket(&self.options.socket_path)?;
        // The startup lock is held for the server lifetime. Bind the published
        // path while holding it; a shared temporary suffix plus rename would
        // let concurrent starters race over one staging pathname.
        let listener = match UnixListener::bind(&self.options.socket_path) {
            Ok(listener) => listener,
            Err(error) => {
                drop(lock);
                return Err(error.into());
            }
        };
        let owner = socket_identity_at_path(&self.options.socket_path)?;
        if let Err(error) = set_mode(&self.options.socket_path, 0o600) {
            let _ = remove_owned_socket(&self.options.socket_path, owner);
            drop(lock);
            return Err(error.into());
        }
        listener.set_nonblocking(true)?;
        let handler = self
            .options
            .handler
            .clone()
            .unwrap_or_else(|| Arc::new(|frame, _| builtin_handler(frame)));
        let result = self.accept_loop(listener, handler);
        let cleanup = remove_owned_socket(&self.options.socket_path, owner);
        self.registry.clear();
        drop(lock);
        result.and(cleanup.map_err(ServerError::Io))
    }

    fn accept_loop(
        &self,
        listener: std::os::unix::net::UnixListener,
        handler: DaemonHandler,
    ) -> Result<(), ServerError> {
        let (sender, receiver) = mpsc::channel();
        let receiver = Arc::new(std::sync::Mutex::new(receiver));
        for _ in 0..CONNECTION_WORKERS {
            let receiver = receiver.clone();
            let handler = handler.clone();
            let options = self.options.clone();
            let state = self.last_activity.clone();
            let stopping = self.stopping.clone();
            let registry = self.registry.clone();
            let dispatch_gate = self.dispatch_gate.clone();
            let started_at = self.started_at;
            thread::spawn(move || {
                loop {
                    let stream = match receiver.lock() {
                        Ok(receiver) => receiver.recv(),
                        Err(_) => return,
                    };
                    let Ok(stream) = stream else {
                        return;
                    };
                    serve_connection(
                        stream,
                        handler.clone(),
                        options.clone(),
                        state.clone(),
                        stopping.clone(),
                        started_at,
                        registry.clone(),
                        dispatch_gate.clone(),
                    );
                }
            });
        }
        while !self.stopping.load(Ordering::Acquire) {
            let idle_expired = self.options.idle_timeout.is_some_and(|idle| {
                !idle.is_zero() && elapsed_since(self.last_activity.load(Ordering::Acquire)) >= idle
            });
            if idle_expired {
                self.stop();
                break;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    if !peer_is_current_user(&stream).unwrap_or(false) {
                        continue;
                    }
                    // macOS inherits O_NONBLOCK from the listener. Connection
                    // workers use blocking buffered reads with explicit
                    // deadlines, so clear the inherited flag before queuing.
                    stream.set_nonblocking(false)?;
                    let _ = stream.set_read_timeout(Some(self.options.read_timeout));
                    let _ = stream.set_write_timeout(Some(self.options.read_timeout));
                    sender
                        .send(stream)
                        .map_err(|_| io::Error::other("daemon connection workers stopped"))?;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    wait_for_unix_listener(&listener, Duration::from_millis(25))?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}
pub(super) fn wait_for_unix_listener(
    listener: &std::os::unix::net::UnixListener,
    timeout: Duration,
) -> io::Result<()> {
    use std::os::fd::AsFd;

    let timeout_ms = timeout.as_millis().clamp(1, u16::MAX.into()) as u16;
    let mut descriptors = [nix::poll::PollFd::new(
        listener.as_fd(),
        nix::poll::PollFlags::POLLIN,
    )];
    nix::poll::poll(&mut descriptors, timeout_ms).map_err(io::Error::other)?;
    Ok(())
}

/// A connection is served only when its peer runs as the daemon's own
/// effective user; any other UID, including root, is dropped. Pure so the
/// negative case is testable without a foreign-UID process.
pub(super) fn peer_uid_is_current(peer: u32, effective: u32) -> bool {
    peer == effective
}

#[cfg(all(
    unix,
    any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
pub(super) fn peer_is_current_user(stream: &std::os::unix::net::UnixStream) -> io::Result<bool> {
    let (uid, _) = nix::unistd::getpeereid(stream).map_err(io::Error::other)?;
    Ok(peer_uid_is_current(
        uid.as_raw(),
        nix::unistd::Uid::effective().as_raw(),
    ))
}

#[cfg(all(unix, any(target_os = "linux", target_os = "android")))]
pub(super) fn peer_is_current_user(stream: &std::os::unix::net::UnixStream) -> io::Result<bool> {
    use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
    let credentials = getsockopt(stream, PeerCredentials).map_err(io::Error::other)?;
    Ok(peer_uid_is_current(
        credentials.uid(),
        nix::unistd::Uid::effective().as_raw(),
    ))
}

#[cfg(all(
    unix,
    not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "linux",
        target_os = "android"
    ))
))]
pub(super) fn peer_is_current_user(_stream: &std::os::unix::net::UnixStream) -> io::Result<bool> {
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn serve_connection(
    stream: std::os::unix::net::UnixStream,
    handler: DaemonHandler,
    options: ServerOptions,
    last_activity: Arc<AtomicI64>,
    stopping: Arc<AtomicBool>,
    started_at: i64,
    registry: Arc<crate::SessionRegistry>,
    dispatch_gate: Arc<std::sync::Mutex<()>>,
) {
    serve_connection_parts(
        stream,
        handler,
        options,
        last_activity,
        stopping,
        started_at,
        registry,
        dispatch_gate,
    );
}

pub(super) fn acquire_lock(path: &Path) -> Result<File, ServerError> {
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        // O_NOFOLLOW: a symlink planted at the lock path must fail the open
        // instead of redirecting the lock (and its create/truncate-free open)
        // to a file the attacker chose.
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits())
            .open(path)?
    };
    file.try_lock_exclusive().map_err(|error| {
        if error.kind() == io::ErrorKind::WouldBlock {
            ServerError::AlreadyRunning
        } else {
            ServerError::Io(error)
        }
    })?;
    Ok(file)
}

pub(super) fn prepare_socket(path: &Path) -> Result<(), ServerError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_socket() => {
            return Err(io::Error::other("socket path exists and is not a Unix socket").into());
        }
        Ok(metadata) => {
            let owner = socket_identity(&metadata);
            match crate::connect_unix(path, STALE_SOCKET_PROBE_TIMEOUT) {
                Ok(_) => return Err(ServerError::AlreadyRunning),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    remove_owned_socket(path, owner)?;
                }
                Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                    return Err(ServerError::AlreadyRunning);
                }
                Err(error) => return Err(ServerError::Io(error)),
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub(super) fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_uid_check_accepts_only_the_effective_user() {
        let own = nix::unistd::Uid::effective().as_raw();
        let cases = [
            (own, own, true),
            (own.wrapping_add(1), own, false),
            (0, 501, false),
            (501, 0, false),
            (u32::MAX, 501, false),
        ];
        for (peer, effective, expected) in cases {
            assert_eq!(
                peer_uid_is_current(peer, effective),
                expected,
                "peer={peer} effective={effective}"
            );
        }
    }

    #[test]
    fn lock_file_open_refuses_a_symlink_at_the_lock_path() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("sbl{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let target = root.join("victim");
        fs::write(&target, b"keep").unwrap();
        let lock = root.join("d.sock.lock");
        symlink(&target, &lock).unwrap();
        let error = acquire_lock(&lock).expect_err("symlinked lock path must be refused");
        assert!(matches!(error, ServerError::Io(_)), "{error:?}");
        assert_eq!(fs::read(&target).unwrap(), b"keep");

        // A regular lock file is still acquired exactly once.
        let regular = root.join("r.sock.lock");
        let held = acquire_lock(&regular).expect("regular lock file");
        assert!(matches!(
            acquire_lock(&regular),
            Err(ServerError::AlreadyRunning)
        ));
        drop(held);
        let _ = fs::remove_dir_all(&root);
    }
}
