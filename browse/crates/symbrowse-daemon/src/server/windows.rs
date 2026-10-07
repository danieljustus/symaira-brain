use super::*;
#[cfg(windows)]
pub(super) fn wake_windows_listener(path: &Path) {
    use interprocess::os::windows::named_pipe::{DuplexPipeStream, pipe_mode};

    let _ = DuplexPipeStream::<pipe_mode::Bytes>::connect_by_path_with_wait_mode(
        path.to_string_lossy().as_ref(),
        interprocess::ConnectWaitMode::Timeout(Duration::from_millis(50)),
    );
}

#[cfg(windows)]
pub(super) fn listen_windows(server: &Server) -> Result<(), ServerError> {
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
    let (sender, receiver) = mpsc::sync_channel::<
        interprocess::os::windows::named_pipe::DuplexPipeStream<pipe_mode::Bytes>,
    >(CONNECTION_WORKERS);
    let receiver = Arc::new(std::sync::Mutex::new(receiver));
    let mut workers = Vec::with_capacity(CONNECTION_WORKERS);
    for _ in 0..CONNECTION_WORKERS {
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
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
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
pub(super) fn named_pipe_create_error(error: io::Error) -> ServerError {
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
pub(super) fn serve_connection_windows(
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
#[cfg(windows)]
pub(super) fn read_limited_line_windows<R: io::Read>(
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
