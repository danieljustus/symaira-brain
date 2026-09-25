use crate::{
    DaemonError, Frame, MAX_FRAME_BYTES, Response, Warning, codes, decode_frame, error_response,
    success_response,
};
#[cfg(unix)]
use fs2::FileExt;
use serde_json::Value;
use serde_json::json;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::fs::{File, OpenOptions};
#[cfg(not(windows))]
use std::io::BufRead;
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt};
#[cfg(any(unix, windows))]
use std::sync::mpsc;
use std::{
    io::{self, BufReader, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[cfg(unix)]
use symbrowse_core::state_store::Store;

pub type HandlerResult = Result<(Option<Value>, Vec<Warning>), DaemonError>;
pub type DaemonHandler =
    Arc<dyn Fn(Frame, OperationContext) -> HandlerResult + Send + Sync + 'static>;

// Connections can stay open for several request frames. Bound the number of
// connection threads to a small multiple of available CPU capacity so short-
// lived clients do not wake a large pool for every daemon round trip.
#[cfg(any(unix, windows))]
const MAX_CONNECTION_WORKERS: usize = 32;

#[cfg(any(unix, windows))]
fn bounded_connection_workers(parallelism: Option<usize>) -> usize {
    parallelism
        .unwrap_or(4)
        .saturating_mul(2)
        .clamp(4, MAX_CONNECTION_WORKERS)
}

#[cfg(any(unix, windows))]
fn connection_worker_count() -> usize {
    bounded_connection_workers(
        std::thread::available_parallelism()
            .ok()
            .map(|value| value.get()),
    )
}

#[derive(Clone, Debug)]
pub struct OperationContext {
    cancelled: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    deadline: Instant,
}

impl OperationContext {
    pub(crate) fn for_test() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + Duration::from_secs(60 * 60),
        }
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire) || self.shutdown.load(Ordering::Acquire)
    }

    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct PolicyStatus {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_domains: Vec<String>,
    pub ssrf_enabled: bool,
    pub fetch_ssrf_enabled: bool,
    pub allow_private: bool,
}

#[cfg(unix)]
const STALE_SOCKET_PROBE_TIMEOUT: Duration = Duration::from_millis(100);

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SocketIdentity {
    device: u64,
    inode: u64,
}

#[cfg(unix)]
fn socket_identity(metadata: &fs::Metadata) -> SocketIdentity {
    SocketIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

#[cfg(unix)]
fn socket_identity_at_path(path: &Path) -> io::Result<SocketIdentity> {
    fs::symlink_metadata(path).map(|metadata| socket_identity(&metadata))
}

#[cfg(unix)]
fn remove_owned_socket(path: &Path, owner: SocketIdentity) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() && socket_identity(&metadata) == owner => {
            fs::remove_file(path)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[derive(Clone)]

pub struct ServerOptions {
    pub socket_path: PathBuf,
    pub session: String,
    pub idle_timeout: Option<Duration>,
    pub operation_timeout: Duration,
    pub read_timeout: Duration,
    pub handler: Option<DaemonHandler>,
    pub registry: Option<Arc<crate::SessionRegistry>>,
    pub session_spec: Option<crate::SessionSpec>,
    pub policy: PolicyStatus,
    pub engine: String,
    pub mode: String,
}
impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path("default"),
            session: "default".into(),
            idle_timeout: Some(Duration::from_secs(30 * 60)),
            operation_timeout: Duration::from_millis(crate::DEFAULT_OPERATION_TIMEOUT_MS),
            read_timeout: Duration::from_millis(crate::DEFAULT_READ_TIMEOUT_MS),
            handler: None,
            registry: None,
            session_spec: None,
            policy: PolicyStatus::default(),
            engine: "chrome".into(),
            mode: "browser".into(),
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct DaemonState {
    pub running: bool,
    pub pid: u32,
    pub socket: String,
    pub started_at: String,
    pub last_activity: String,
    pub policy: PolicyStatus,
    pub engine: String,
    pub mode: String,
}

#[derive(Debug)]
pub enum ServerError {
    Io(io::Error),
    AlreadyRunning,
    InvalidSession(String),
    Unsupported,
}
impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::AlreadyRunning => f.write_str("a daemon is already running for this session"),
            Self::InvalidSession(s) => write!(
                f,
                "invalid session {s:?}: use 1-64 letters, digits, '.', '_' or '-'"
            ),
            Self::Unsupported => f.write_str("daemon sockets are not supported on this platform"),
        }
    }
}
impl std::error::Error for ServerError {}
impl From<io::Error> for ServerError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct Server {
    options: ServerOptions,
    stopping: Arc<AtomicBool>,
    started_at: i64,
    last_activity: Arc<AtomicI64>,
    registry: Arc<crate::SessionRegistry>,
    dispatch_gate: Arc<std::sync::Mutex<()>>,
}
impl Server {
    pub fn new(mut options: ServerOptions) -> Result<Self, ServerError> {
        let configured_spec = options.session_spec.clone();
        let spec = configured_spec
            .clone()
            .unwrap_or_else(|| crate::SessionSpec::for_session(options.session.clone()));
        if configured_spec.is_some() {
            options.session = spec.session.clone();
            options.socket_path = spec.socket_path.clone();
            options.operation_timeout = spec.operation_timeout;
            options.read_timeout = spec.read_timeout;
            options.idle_timeout = spec.idle_timeout;
            options.engine = spec.engine.clone();
            options.mode = spec.mode.clone();
            options.policy = PolicyStatus {
                allowed_domains: spec.allowed_domains.clone(),
                ssrf_enabled: spec.ssrf_enabled,
                fetch_ssrf_enabled: spec.ssrf_enabled,
                allow_private: spec.allow_private,
            };
        }
        if !validate_session(&options.session) {
            return Err(ServerError::InvalidSession(crate::redact_str(
                &options.session,
            )));
        }
        if options.socket_path.as_os_str().is_empty() {
            options.socket_path = crate::default_socket_path(&options.session);
        }
        if options.operation_timeout.is_zero() {
            options.operation_timeout = Duration::from_millis(crate::DEFAULT_OPERATION_TIMEOUT_MS);
        }
        if options.read_timeout.is_zero() {
            options.read_timeout = Duration::from_millis(crate::DEFAULT_READ_TIMEOUT_MS);
        }
        let now = unix_nanos();
        let registry = options.registry.take().unwrap_or_else(|| {
            Arc::new(crate::SessionRegistry::new(crate::SessionRegistryOptions {
                user_data_root: if configured_spec.is_some() {
                    spec.state_dir.join("sessions")
                } else {
                    default_user_data_root()
                },
                pid: std::process::id(),
                scope: String::new(),
                origin_path: String::new(),
            }))
        });
        if options.handler.is_none() {
            options.handler = Some(
                crate::runtime::handler(spec)
                    .map_err(|error| ServerError::Io(io::Error::other(error.message)))?,
            );
        }
        Ok(Self {
            options,
            stopping: Arc::new(AtomicBool::new(false)),
            started_at: now,
            last_activity: Arc::new(AtomicI64::new(now)),
            registry,
            dispatch_gate: Arc::new(std::sync::Mutex::new(())),
        })
    }
    pub fn options(&self) -> &ServerOptions {
        &self.options
    }
    pub fn state(&self) -> DaemonState {
        self.status_data()
    }
    pub fn stop(&self) {
        let _gate = self.dispatch_gate.lock().ok();
        self.stopping.store(true, Ordering::Release);
        #[cfg(windows)]
        wake_windows_listener(&self.options.socket_path);
    }

    pub fn listen_and_serve(&self) -> Result<(), ServerError> {
        #[cfg(unix)]
        {
            self.listen_unix()
        }
        #[cfg(windows)]
        {
            listen_windows(self)
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(ServerError::Unsupported)
        }
    }

    #[cfg(unix)]
    fn listen_unix(&self) -> Result<(), ServerError> {
        use std::os::unix::net::UnixListener;
        let parent = self
            .options
            .socket_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        set_mode(parent, 0o700)?;
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

    #[cfg(unix)]
    fn accept_loop(
        &self,
        listener: std::os::unix::net::UnixListener,
        handler: DaemonHandler,
    ) -> Result<(), ServerError> {
        let worker_count = connection_worker_count();
        let (sender, receiver) = mpsc::channel();
        let receiver = Arc::new(std::sync::Mutex::new(receiver));
        for _ in 0..worker_count {
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

    fn status_data(&self) -> DaemonState {
        DaemonState {
            running: true,
            pid: std::process::id(),
            socket: self.options.socket_path.display().to_string(),
            started_at: format_time(self.started_at),
            last_activity: format_time(self.last_activity.load(Ordering::Acquire)),
            policy: self.options.policy.clone(),
            engine: self.options.engine.clone(),
            mode: self.options.mode.clone(),
        }
    }
}

#[cfg(unix)]
fn wait_for_unix_listener(
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

#[cfg(windows)]
fn wake_windows_listener(path: &Path) {
    use interprocess::os::windows::named_pipe::{DuplexPipeStream, pipe_mode};

    let _ = DuplexPipeStream::<pipe_mode::Bytes>::connect_by_path_with_wait_mode(
        path.to_string_lossy().as_ref(),
        interprocess::ConnectWaitMode::Timeout(Duration::from_millis(50)),
    );
}

#[cfg(windows)]
fn listen_windows(server: &Server) -> Result<(), ServerError> {
    use interprocess::os::windows::{
        named_pipe::{PipeListenerOptions, PipeMode, pipe_mode},
        security_descriptor::SecurityDescriptor,
    };
    use widestring::u16cstr;

    // `FILE_FLAG_FIRST_PIPE_INSTANCE`, used by interprocess for the initial
    // listener instance, is the endpoint ownership primitive on Windows. A
    // separate filesystem lock is not equivalent to the configured pipe path
    // and can obscure its native duplicate-owner error.
    let security =
        SecurityDescriptor::deserialize(u16cstr!("D:P(A;;GA;;;OW)")).map_err(ServerError::Io)?;
    let path = server.options.socket_path.to_string_lossy();
    let listener = PipeListenerOptions::new()
        .path(path.as_ref())
        .mode(PipeMode::Bytes)
        .accept_remote(false)
        .nonblocking(true)
        .security_descriptor(Some(security))
        .create_duplex::<pipe_mode::Bytes>()
        .map_err(named_pipe_create_error)?;
    // Keep the listener's native nonblocking mode explicit. The accept loop
    // must observe stop() without waiting indefinitely in accept().
    listener.set_nonblocking(true).map_err(ServerError::Io)?;
    server
        .registry
        .ensure(&server.options.session)
        .map_err(|error| ServerError::Io(io::Error::other(error.to_string())))?;
    let handler = server
        .options
        .handler
        .clone()
        .unwrap_or_else(|| Arc::new(|frame, _| builtin_handler(frame)));
    let worker_count = connection_worker_count();
    let (sender, receiver) = mpsc::sync_channel::<
        interprocess::os::windows::named_pipe::DuplexPipeStream<pipe_mode::Bytes>,
    >(worker_count);
    let receiver = Arc::new(std::sync::Mutex::new(receiver));
    let mut workers = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let receiver = receiver.clone();
        let handler = handler.clone();
        let state = server.last_activity.clone();
        let stopping = server.stopping.clone();
        let options = server.options.clone();
        let registry = server.registry.clone();
        let dispatch_gate = server.dispatch_gate.clone();
        let started_at = server.started_at;
        workers.push(thread::spawn(move || {
            loop {
                let stream = match receiver.lock() {
                    Ok(receiver) => receiver.recv(),
                    Err(_) => return,
                };
                let Ok(stream) = stream else {
                    return;
                };
                // Re-wrap the owned server handle with Tokio's named-pipe
                // registration. This reopens the handle for overlapped I/O;
                // every bounded read/write is therefore cancellable by dropping
                // its future, and the joined worker owns final handle close.
                let Ok(handle) = std::os::windows::io::OwnedHandle::try_from(stream) else {
                    return;
                };
                serve_connection_windows(
                    handle,
                    handler.clone(),
                    options.clone(),
                    state.clone(),
                    stopping.clone(),
                    started_at,
                    registry.clone(),
                    dispatch_gate.clone(),
                );
            }
        }));
    }
    while !server.stopping.load(Ordering::Acquire) {
        let idle_expired = server.options.idle_timeout.is_some_and(|idle| {
            !idle.is_zero() && elapsed_since(server.last_activity.load(Ordering::Acquire)) >= idle
        });
        if idle_expired {
            server.stop();
            break;
        }
        match listener.accept() {
            Ok(stream) => {
                // The listener uses NOWAIT only to make accept interruptible.
                // Clear that pipe-mode flag before ReOpenFile hands the owned
                // handle to Tokio, which supplies the actual overlapped I/O.
                if stream.set_nonblocking(false).is_err() {
                    continue;
                }
                if sender.try_send(stream).is_err() {
                    // A full pool is deliberate backpressure: close this
                    // client instead of creating another unbounded thread.
                    continue;
                }
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut
                    // Win32 ERROR_SEM_TIMEOUT can be surfaced without the
                    // TimedOut kind by the nonblocking named-pipe acceptor.
                    || error.raw_os_error() == Some(121) =>
            {
                // ponytail: 5 ms polling bounds pipe latency; use interruptible accept if idle CPU matters.
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                server.stop();
                drop(sender);
                for worker in workers {
                    let _ = worker.join();
                }
                server.registry.clear();
                return Err(ServerError::Io(error));
            }
        }
    }
    drop(sender);
    // Accepted streams remain owned by workers. Their bounded nonblocking read
    // observes the shared stop token, so joining here drains every worker and
    // closes every native handle before the registry is cleared.
    for worker in workers {
        let _ = worker.join();
    }
    server.registry.clear();
    Ok(())
}
#[cfg(windows)]
fn named_pipe_create_error(error: io::Error) -> ServerError {
    use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;

    // interprocess uses FILE_FLAG_FIRST_PIPE_INSTANCE for the first server
    // instance. Windows reports contention as ERROR_ACCESS_DENIED rather than
    // the advisory-lock WouldBlock result used by Unix socket startup.
    if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
        ServerError::AlreadyRunning
    } else {
        ServerError::Io(error)
    }
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
fn peer_is_current_user(stream: &std::os::unix::net::UnixStream) -> io::Result<bool> {
    let (uid, _) = nix::unistd::getpeereid(stream).map_err(io::Error::other)?;
    Ok(peer_uid_matches(
        uid.as_raw(),
        nix::unistd::Uid::effective().as_raw(),
    ))
}

#[cfg(all(unix, any(target_os = "linux", target_os = "android")))]
fn peer_is_current_user(stream: &std::os::unix::net::UnixStream) -> io::Result<bool> {
    use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
    let credentials = getsockopt(stream, PeerCredentials).map_err(io::Error::other)?;
    Ok(peer_uid_matches(
        credentials.uid(),
        nix::unistd::Uid::effective().as_raw(),
    ))
}

#[cfg(unix)]
fn peer_uid_matches(peer_uid: u32, current_uid: u32) -> bool {
    peer_uid == current_uid
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
fn peer_is_current_user(_stream: &std::os::unix::net::UnixStream) -> io::Result<bool> {
    Ok(false)
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn serve_connection(
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

#[cfg(windows)]
struct OverlappedPipeStream {
    // Tokio's native NamedPipeServer is the sole owner after from_raw_handle
    // consumes the accepted interprocess handle. Unlike interprocess's
    // PipeStream, it has no limbo or blocking FlushFileBuffers task.
    stream: tokio::net::windows::named_pipe::NamedPipeServer,
    runtime: tokio::runtime::Runtime,
    stopping: Arc<AtomicBool>,
}

#[cfg(windows)]
impl OverlappedPipeStream {
    #[allow(unsafe_code)]
    fn new(
        handle: std::os::windows::io::OwnedHandle,
        stopping: Arc<AtomicBool>,
    ) -> io::Result<Self> {
        use std::os::windows::io::IntoRawHandle;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()?;
        // SAFETY: `into_raw_handle` transfers the accepted handle's sole
        // ownership to Tokio. The interprocess stream is consumed before this
        // call, so it cannot close or otherwise access the handle afterwards.
        // Tokio registers the already-overlapped server handle with its IOCP
        // reactor and owns it until this wrapper is dropped.
        let stream = {
            let _entered = runtime.enter();
            let raw = handle.into_raw_handle();
            unsafe { tokio::net::windows::named_pipe::NamedPipeServer::from_raw_handle(raw) }?
        };
        Ok(Self {
            stream,
            runtime,
            stopping,
        })
    }
}

#[cfg(windows)]
impl Drop for OverlappedPipeStream {
    fn drop(&mut self) {
        // An aborted response is explicitly allowed to be truncated. Native
        // disconnect makes that outcome observable to a peer that starts
        // draining only after worker join instead of leaving a live pipe
        // instance behind.
        let _ = self.stream.disconnect();
    }
}

#[cfg(windows)]
impl io::Read for OverlappedPipeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use tokio::io::AsyncReadExt;
        let stopping = self.stopping.clone();
        let result = self.runtime.block_on(async {
            tokio::select! {
                result = tokio::time::timeout(Duration::from_millis(10), self.stream.read(buf)) => Ok(result),
                _ = wait_for_pipe_stop(stopping) => Err(()),
            }
        });
        match result {
            Err(()) => Err(io::Error::from(io::ErrorKind::Interrupted)),
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(io::Error::from(io::ErrorKind::WouldBlock)),
        }
    }
}

#[cfg(windows)]
impl io::Write for OverlappedPipeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        use tokio::io::AsyncWriteExt;
        let stopping = self.stopping.clone();
        let result = self.runtime.block_on(async {
            tokio::select! {
                result = tokio::time::timeout(Duration::from_millis(10), self.stream.write(buf)) => Ok(result),
                _ = wait_for_pipe_stop(stopping) => Err(()),
            }
        });
        match result {
            Err(()) => Err(io::Error::from(io::ErrorKind::Interrupted)),
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(io::Error::from(io::ErrorKind::WouldBlock)),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        use tokio::io::AsyncWriteExt;

        // Native Tokio's named-pipe flush is readiness bookkeeping only; it
        // does not call synchronous FlushFileBuffers or spawn a blocking task.
        // Therefore Runtime::Drop cannot wait for a client drain here. A
        // successful flush means the bytes were accepted by the native async
        // write path, not that the peer has read them.
        self.runtime.block_on(self.stream.flush())
    }
}

#[cfg(windows)]
async fn wait_for_pipe_stop(stopping: Arc<AtomicBool>) {
    while !stopping.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn serve_connection_windows(
    handle: std::os::windows::io::OwnedHandle,
    handler: DaemonHandler,
    options: ServerOptions,
    last_activity: Arc<AtomicI64>,
    stopping: Arc<AtomicBool>,
    started_at: i64,
    registry: Arc<crate::SessionRegistry>,
    dispatch_gate: Arc<std::sync::Mutex<()>>,
) {
    let Ok(stream) = OverlappedPipeStream::new(handle, stopping.clone()) else {
        return;
    };
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

#[allow(clippy::too_many_arguments)]
fn serve_connection_parts<S>(
    stream: S,
    handler: DaemonHandler,
    options: ServerOptions,
    last_activity: Arc<AtomicI64>,
    stopping: Arc<AtomicBool>,
    started_at: i64,
    registry: Arc<crate::SessionRegistry>,
    dispatch_gate: Arc<std::sync::Mutex<()>>,
) where
    S: io::Read + Write + Send + 'static,
{
    let mut reader = BufReader::new(stream);
    loop {
        // Do not start another read after shutdown has won. On Windows a
        // named-pipe read can outlive the listener wake-up even though the
        // worker is otherwise cancellation-aware.
        if stopping.load(Ordering::Acquire) {
            return;
        }
        let mut line = Vec::new();
        #[cfg(windows)]
        let read_result = read_limited_line_windows(
            &mut reader,
            &mut line,
            MAX_FRAME_BYTES,
            options.read_timeout,
            &stopping,
        );
        #[cfg(not(windows))]
        let read_result = read_limited_line(&mut reader, &mut line, MAX_FRAME_BYTES);
        match read_result {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        last_activity.store(unix_nanos(), Ordering::Release);
        let frame = match decode_frame(trim_line_ending(&line)) {
            Ok(mut frame) => {
                if frame.session.is_empty() {
                    frame.session = options.session.clone();
                }
                frame
            }
            Err(error) => {
                if write_response(reader.get_mut(), error_response(error.code, error.message))
                    .is_err()
                {
                    return;
                }
                continue;
            }
        };
        // The gate covers every registry mutation and the dispatch decision.
        // stop() takes the same gate, so no request can start after shutdown
        // has won the transition.
        let _dispatch = match dispatch_gate.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if stopping.load(Ordering::Acquire) {
            return;
        }
        if !validate_session(&frame.session) {
            if write_response(
                reader.get_mut(),
                error_response(
                    codes::INVALID_SESSION,
                    format!("invalid session {}", crate::redact_str(&frame.session)),
                ),
            )
            .is_err()
            {
                return;
            }
            continue;
        }
        if frame.cmd == "session.list" {
            let response = success_response(
                Some(serde_json::to_value(registry.list_data()).unwrap_or(Value::Null)),
                Vec::new(),
            );
            drop(_dispatch);
            if write_response(reader.get_mut(), response).is_err() {
                return;
            }
            continue;
        }
        if frame.cmd == "session.info" {
            let response = match registry.touch(&frame.session) {
                Ok(()) => match registry.get(&frame.session) {
                    Ok(info) => success_response(
                        Some(serde_json::to_value(info).unwrap_or(Value::Null)),
                        Vec::new(),
                    ),
                    Err(error) => session_error_response(error),
                },
                Err(error) => session_error_response(error),
            };
            drop(_dispatch);
            if write_response(reader.get_mut(), response).is_err() {
                return;
            }
            continue;
        }
        if let Err(error) = registry.ensure(&frame.session) {
            if write_response(reader.get_mut(), session_error_response(error)).is_err() {
                return;
            }
            continue;
        }
        let _ = registry.touch(&frame.session);
        let cmd = frame.cmd.clone();
        if cmd == "daemon.status" {
            let data = json!({"running":true,"pid":std::process::id(),"session":options.session,"socket":crate::redact_str(&options.socket_path.to_string_lossy()),"started_at":format_time(started_at),"last_activity":format_time(last_activity.load(Ordering::Acquire)),"policy":options.policy,"engine":options.engine,"mode":options.mode});
            drop(_dispatch);
            if write_response(reader.get_mut(), success_response(Some(data), Vec::new())).is_err() {
                return;
            }
            continue;
        }
        if cmd == "daemon.stop" {
            if write_response(
                reader.get_mut(),
                success_response(Some(json!({"stopping":true})), Vec::new()),
            )
            .is_err()
            {
                return;
            }
            stopping.store(true, Ordering::Release);
            #[cfg(windows)]
            wake_windows_listener(&options.socket_path);
            return;
        }
        if cmd == "daemon.ping" {
            // The gate protects the dispatch decision and registry update
            // against stop(); writing a fast-path reply does not need to hold
            // it. Releasing here avoids serializing unrelated ping clients on
            // response I/O while preserving the stop transition.
            drop(_dispatch);
            if write_response(
                reader.get_mut(),
                success_response(Some(json!({"pong":true})), Vec::new()),
            )
            .is_err()
            {
                return;
            }
            continue;
        }
        drop(_dispatch);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let handler_clone = handler.clone();
        let frame_clone = frame.clone();
        let timeout = options.operation_timeout;
        let operation = OperationContext {
            cancelled: Arc::new(AtomicBool::new(false)),
            shutdown: stopping.clone(),
            deadline: Instant::now() + timeout,
        };
        let handler_operation = operation.clone();
        thread::spawn(move || {
            let _ = tx.send(handler_clone(frame_clone, handler_operation));
        });
        let result = loop {
            match rx.recv_timeout(operation.remaining().min(Duration::from_millis(10))) {
                Ok(result) => break operation_result_response(result, &operation),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break error_response(codes::OPERATION_FAILED, "daemon handler disconnected");
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    if stopping.load(Ordering::Acquire) =>
                {
                    // Shutdown is cooperative: notify the handler through its
                    // context and give it a bounded cancellation window. Do
                    // not wait for the full request deadline during shutdown.
                    operation.cancel();
                    let shutdown_deadline = Instant::now() + Duration::from_millis(100);
                    match rx
                        .recv_timeout(shutdown_deadline.saturating_duration_since(Instant::now()))
                    {
                        Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return,
                    }
                    return;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    if operation.remaining().is_zero() =>
                {
                    operation.cancel();
                    break error_response(
                        codes::OPERATION_TIMEOUT,
                        "daemon operation exceeded its timeout",
                    );
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            }
        };
        if write_response(reader.get_mut(), result).is_err() {
            return;
        }
    }
}

fn trim_line_ending(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn operation_result_response(
    result: Result<(Option<Value>, Vec<Warning>), DaemonError>,
    operation: &OperationContext,
) -> Response {
    if operation.remaining().is_zero() {
        operation.cancel();
        return error_response(
            codes::OPERATION_TIMEOUT,
            "daemon operation exceeded its timeout",
        );
    }
    match result {
        Ok((data, warnings)) => success_response(data, warnings),
        Err(error) => Response {
            success: false,
            data: None,
            error: Some(crate::redaction::redact_error(error)),
            warnings: Vec::new(),
        },
    }
}

fn write_response<W: Write>(stream: &mut W, response: Response) -> io::Result<()> {
    let mut data = serde_json::to_vec(&response).map_err(io::Error::other)?;
    if data.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "daemon response exceeds size limit",
        ));
    }
    data.push(b'\n');
    stream.write_all(&data)?;
    stream.flush()
}

#[cfg(not(windows))]
fn read_limited_line<R: BufRead>(
    reader: &mut R,
    output: &mut Vec<u8>,
    limit: usize,
) -> io::Result<usize> {
    output.clear();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(output.len());
        }
        let take = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |n| n + 1);
        if output.len().saturating_add(take) > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "daemon frame exceeds size limit",
            ));
        }
        output.extend_from_slice(&available[..take]);
        reader.consume(take);
        if output.last() == Some(&b'\n') {
            return Ok(output.len());
        }
    }
}

#[cfg(windows)]
fn read_limited_line_windows<R: io::Read>(
    reader: &mut R,
    output: &mut Vec<u8>,
    limit: usize,
    timeout: Duration,
    stopping: &AtomicBool,
) -> io::Result<usize> {
    output.clear();
    let deadline = Instant::now() + timeout;
    let mut byte = [0_u8; 1];
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "daemon frame read timed out",
            ));
        }
        if stopping.load(Ordering::Acquire) {
            return Ok(0);
        }
        match reader.read(&mut byte) {
            Ok(0) => return Ok(output.len()),
            Ok(read) => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "daemon frame read timed out",
                    ));
                }
                if output.len().saturating_add(read) > limit {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "daemon frame exceeds size limit",
                    ));
                }
                output.extend_from_slice(&byte[..read]);
                if byte[..read].contains(&b'\n') {
                    return Ok(output.len());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "daemon frame read timed out",
                    ));
                }
                thread::sleep(remaining.min(Duration::from_millis(2)));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn acquire_lock(path: &Path) -> Result<File, ServerError> {
    let file = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .mode(0o600)
                .open(path)?
        }
        #[cfg(windows)]
        {
            OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(path)?
        }
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

#[cfg(unix)]
fn prepare_socket(path: &Path) -> Result<(), ServerError> {
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
                // Match Go's socketIsLive: only NotFound and ConnectionRefused
                // prove that the old endpoint is stale. Permission and other
                // ambiguous probe failures must preserve the existing socket.
                Err(_) => {
                    return Err(ServerError::AlreadyRunning);
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

fn session_error_response(error: crate::SessionError) -> Response {
    let (code, message) = match error {
        crate::SessionError::InvalidName(message) => (codes::INVALID_SESSION, message),
        crate::SessionError::NotFound(message) => (codes::SESSION_NOT_FOUND, message),
        crate::SessionError::Io(message) | crate::SessionError::InvalidValue(message) => {
            (codes::OPERATION_FAILED, message)
        }
    };
    error_response(code, message)
}

fn unix_nanos() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos().min(i64::MAX as u128) as i64)
}
fn elapsed_since(start: i64) -> Duration {
    Duration::from_nanos(unix_nanos().saturating_sub(start) as u64)
}
fn format_time(nanos: i64) -> String {
    let Ok(value) = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(nanos)) else {
        return "1970-01-01T00:00:00Z".into();
    };
    value
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

fn default_user_data_root() -> PathBuf {
    if let Ok(path) = std::env::var("SYMBROWSE_USER_DATA_DIR") {
        return PathBuf::from(path);
    }
    if cfg!(target_os = "macos")
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home).join("Library/Caches/symbrowse/sessions");
    }
    if cfg!(windows)
        && let Ok(local_app_data) = std::env::var("LOCALAPPDATA")
    {
        return PathBuf::from(local_app_data).join("symbrowse/sessions");
    }
    if let Ok(path) = std::env::var("XDG_CACHE_HOME") {
        return PathBuf::from(path).join("symbrowse/sessions");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".cache/symbrowse/sessions");
    }
    std::env::temp_dir().join("symbrowse/sessions")
}

pub fn validate_session(session: &str) -> bool {
    !session.is_empty()
        && session.len() <= 64
        && session
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

pub fn socket_path(base: impl AsRef<Path>, session: &str) -> Result<PathBuf, ServerError> {
    if !validate_session(session) {
        return Err(ServerError::InvalidSession(session.into()));
    }
    Ok(base.as_ref().join(format!("{session}.sock")))
}

pub fn default_socket_path(session: &str) -> PathBuf {
    crate::spec::default_socket_path(session)
}

#[cfg(not(unix))]
fn builtin_handler(_frame: Frame) -> HandlerResult {
    Err(DaemonError {
        code: codes::OPERATION_FAILED.into(),
        message: "daemon state operations are unavailable on this platform".into(),
        ..Default::default()
    })
}

#[cfg(unix)]
fn builtin_handler(frame: Frame) -> HandlerResult {
    if !frame.cmd.starts_with("state.") {
        return Err(DaemonError {
            code: codes::UNKNOWN_COMMAND.into(),
            message: "command is not implemented by the daemon".into(),
            ..Default::default()
        });
    }
    if matches!(frame.cmd.as_str(), "state.save" | "state.load") {
        return Err(DaemonError {
            code: codes::OPERATION_FAILED.into(),
            message: "browser-backed state capture and restore are not implemented in Rust".into(),
            hint: "use the Go fallback until the browser state runtime passes parity".into(),
            ..Default::default()
        });
    }
    let store = Store::new(state_root(), time::Duration::days(30), None).map_err(store_error)?;
    let args = frame.args.as_ref().and_then(Value::as_object);
    match frame.cmd.as_str() {
        "state.list" => Ok((
            Some(json!({"schema_version": 1, "states": store.list().map_err(store_error)?})),
            Vec::new(),
        )),

        "state.show" => {
            let name = required_state_name(args)?;
            let metadata = store.metadata(name).map_err(store_error)?;
            Ok((
                Some(serde_json::to_value(metadata).map_err(|e| DaemonError {
                    code: codes::OPERATION_FAILED.into(),
                    message: e.to_string(),
                    ..Default::default()
                })?),
                Vec::new(),
            ))
        }
        "state.clear" => {
            let name = required_state_name(args)?;
            store.remove(name).map_err(store_error)?;
            Ok((Some(json!({"name": name, "cleared": true})), Vec::new()))
        }
        "state.clean" => {
            let removed = if let Some(days) = args
                .and_then(|value| value.get("older_than_days"))
                .and_then(Value::as_i64)
            {
                store
                    .clean_older_than_at(
                        time::Duration::days(days),
                        time::OffsetDateTime::now_utc(),
                    )
                    .map_err(store_error)?
            } else {
                store
                    .clean_at(time::OffsetDateTime::now_utc())
                    .map_err(store_error)?
            };
            Ok((Some(json!({"removed": removed})), Vec::new()))
        }
        _ => Err(DaemonError {
            code: codes::UNKNOWN_COMMAND.into(),
            message: "command is not implemented by the daemon".into(),
            ..Default::default()
        }),
    }
}

#[cfg(unix)]
fn required_state_name(args: Option<&serde_json::Map<String, Value>>) -> Result<&str, DaemonError> {
    args.and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| DaemonError {
            code: codes::MALFORMED_REQUEST.into(),
            message: "state name is required".into(),
            ..Default::default()
        })
}
#[cfg(unix)]
fn store_error(error: impl std::fmt::Display) -> DaemonError {
    DaemonError {
        code: codes::OPERATION_FAILED.into(),
        message: crate::redact_str(&error.to_string()),
        ..Default::default()
    }
}
#[cfg(unix)]
fn state_root() -> PathBuf {
    if let Ok(path) = std::env::var("SYMBROWSE_STATE_DIR") {
        return PathBuf::from(path).join("states");
    }
    if let Ok(path) = std::env::var("XDG_STATE_HOME") {
        return PathBuf::from(path).join("symbrowse/states");
    }
    std::env::var("HOME").map_or_else(
        |_| std::env::temp_dir().join("symbrowse/states"),
        |home| PathBuf::from(home).join(".local/state/symbrowse/states"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};
    use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

    #[test]
    fn connection_worker_pool_scales_with_cpu_and_stays_bounded() {
        assert_eq!(bounded_connection_workers(None), 8);
        assert_eq!(bounded_connection_workers(Some(1)), 4);
        assert_eq!(bounded_connection_workers(Some(2)), 4);
        assert_eq!(bounded_connection_workers(Some(4)), 8);
        assert_eq!(bounded_connection_workers(Some(16)), 32);
        assert_eq!(bounded_connection_workers(Some(128)), 32);
    }

    struct BlockingResponseStream {
        input: Cursor<Vec<u8>>,
        started: Option<SyncSender<()>>,
        release: Receiver<()>,
    }

    impl io::Read for BlockingResponseStream {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.input.read(buffer)
        }
    }

    impl io::Write for BlockingResponseStream {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if let Some(started) = self.started.take() {
                let _ = started.send(());
                self.release.recv().map_err(io::Error::other)?;
            }
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn ping_response_io_does_not_hold_the_stop_dispatch_gate() {
        let temp = tempfile::tempdir().expect("temporary registry root");
        let registry = Arc::new(crate::SessionRegistry::new(crate::SessionRegistryOptions {
            user_data_root: temp.path().to_path_buf(),
            ..Default::default()
        }));
        let dispatch_gate = Arc::new(std::sync::Mutex::new(()));
        let (started_tx, started_rx) = sync_channel(1);
        let (release_tx, release_rx) = sync_channel(1);
        let stream = BlockingResponseStream {
            input: Cursor::new(b"{\"cmd\":\"daemon.ping\"}\n".to_vec()),
            started: Some(started_tx),
            release: release_rx,
        };
        let handler: DaemonHandler = Arc::new(|_, _| Ok((None, Vec::new())));
        let worker_gate = dispatch_gate.clone();
        let worker = thread::spawn(move || {
            let now = unix_nanos();
            serve_connection_parts(
                stream,
                handler,
                ServerOptions::default(),
                Arc::new(AtomicI64::new(now)),
                Arc::new(AtomicBool::new(false)),
                now,
                registry,
                worker_gate,
            );
        });

        started_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ping response started");
        let gate_available = dispatch_gate.try_lock().is_ok();
        release_tx.send(()).expect("release response write");
        worker.join().expect("ping worker exits after input EOF");
        assert!(
            gate_available,
            "ping response I/O held the global dispatch gate"
        );
    }

    fn read_frame_for_test(raw: &[u8]) -> io::Result<Vec<u8>> {
        let mut reader = BufReader::new(Cursor::new(raw));
        let mut line = Vec::new();
        #[cfg(not(windows))]
        read_limited_line(&mut reader, &mut line, MAX_FRAME_BYTES)?;
        #[cfg(windows)]
        read_limited_line_windows(
            &mut reader,
            &mut line,
            MAX_FRAME_BYTES,
            Duration::from_secs(1),
            &AtomicBool::new(false),
        )?;
        Ok(line)
    }

    #[test]
    fn frame_line_ending_and_limit_match_go_scanner() {
        assert_eq!(trim_line_ending(b"{\"cmd\":\"x\"}\n"), b"{\"cmd\":\"x\"}");
        assert_eq!(trim_line_ending(b"{\"cmd\":\"x\"}\r\n"), b"{\"cmd\":\"x\"}");
        assert_eq!(trim_line_ending(b"{\"cmd\":\"x\"}\r"), b"{\"cmd\":\"x\"}");

        let vertical_tab = b"{\"cmd\":\"x\"}\x0b";
        assert_eq!(trim_line_ending(vertical_tab), vertical_tab);
        assert!(decode_frame(trim_line_ending(vertical_tab)).is_err());
        assert_eq!(
            decode_frame(trim_line_ending(b"{\"cmd\":\" \"}\n"))
                .expect("whitespace command must reach command dispatch")
                .cmd,
            " "
        );

        let prefix = br#"{"cmd":"x","args":{"v":""#;
        let suffix = b"\"}}";
        let value_len = MAX_FRAME_BYTES - 1 - prefix.len() - suffix.len();
        let mut boundary = Vec::with_capacity(MAX_FRAME_BYTES);
        boundary.extend_from_slice(prefix);
        boundary.resize(boundary.len() + value_len, b'x');
        boundary.extend_from_slice(suffix);
        boundary.push(b'\n');
        assert_eq!(boundary.len(), MAX_FRAME_BYTES);
        let accepted = read_frame_for_test(&boundary).expect("exact scanner limit accepted");
        assert_eq!(accepted.len(), MAX_FRAME_BYTES);
        assert!(decode_frame(trim_line_ending(&accepted)).is_ok());

        let mut oversized = boundary;
        oversized.insert(oversized.len() - 2, b'x');
        assert_eq!(oversized.len(), MAX_FRAME_BYTES + 1);
        assert_eq!(
            read_frame_for_test(&oversized).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[cfg(unix)]
    #[test]
    fn peer_uid_match_policy_accepts_only_the_current_uid() {
        assert!(peer_uid_matches(501, 501));
        assert!(!peer_uid_matches(502, 501));
    }

    #[cfg(unix)]
    #[test]
    fn socket_probe_errors_other_than_refused_preserve_existing_socket() {
        use std::os::unix::net::UnixListener;

        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("private.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        set_mode(&socket, 0o000).unwrap();

        let result = prepare_socket(&socket);

        if socket.exists() {
            set_mode(&socket, 0o600).unwrap();
        }
        assert!(matches!(result, Err(ServerError::AlreadyRunning)));
        assert!(socket.exists(), "ambiguous probe removed a live socket");
        drop(listener);
    }

    #[test]
    fn status_timestamps_use_rfc3339_nano() {
        assert_eq!(format_time(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_time(1_234_000_000), "1970-01-01T00:00:01.234Z");
    }
    #[test]
    fn sessions_are_path_safe() {
        assert!(validate_session("default-1"));
        assert!(!validate_session("../escape"));
        assert!(!validate_session(""));
    }
    #[test]
    fn paths_use_one_session_component() {
        assert_eq!(
            socket_path("/tmp/run", "x").unwrap(),
            PathBuf::from("/tmp/run/x.sock")
        );
    }

    #[test]
    fn completed_handler_result_after_deadline_is_reported_as_timeout() {
        let operation = OperationContext {
            cancelled: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() - Duration::from_millis(1),
        };
        let response =
            operation_result_response(Ok((Some(json!({"done": true})), vec![])), &operation);
        assert_eq!(
            response.error.expect("timeout error").code,
            codes::OPERATION_TIMEOUT
        );
        assert!(operation.is_cancelled());
    }

    #[cfg(windows)]
    #[test]
    fn first_pipe_instance_contention_is_already_running() {
        let error = io::Error::from_raw_os_error(5); // ERROR_ACCESS_DENIED
        assert!(matches!(
            named_pipe_create_error(error),
            ServerError::AlreadyRunning
        ));
    }

    #[cfg(unix)]
    #[test]
    fn browser_state_save_and_load_fail_before_store_access() {
        for command in ["state.save", "state.load"] {
            let error = builtin_handler(Frame {
                cmd: command.to_owned(),
                args: Some(json!({"name": "never-written"})),
                ..Frame::default()
            })
            .expect_err("browser-backed state operations must be rejected");
            assert_eq!(error.code, codes::OPERATION_FAILED);
            assert!(error.message.contains("not implemented"));
        }
    }
}
