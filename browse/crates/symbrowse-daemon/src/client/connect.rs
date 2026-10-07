use super::*;

#[cfg(unix)]
/// Connect to a Unix daemon endpoint without allowing the kernel connect call
/// to block past `timeout`. The returned stream is restored to blocking mode;
/// subsequent reads and writes use their own socket deadlines.
pub fn connect_unix(path: &Path, timeout: Duration) -> io::Result<UnixStream> {
    use nix::{
        errno::Errno,
        fcntl::{FcntlArg, OFlag, fcntl},
        sys::socket::{
            AddressFamily, SockFlag, SockType, UnixAddr, connect, getsockopt, socket,
            sockopt::SocketError,
        },
    };

    if timeout.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "daemon connect timed out",
        ));
    }
    let socket = socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::empty(),
        None,
    )
    .map_err(nix_io_error)?;
    let current_flags = fcntl(&socket, FcntlArg::F_GETFL).map_err(nix_io_error)?;
    fcntl(
        &socket,
        FcntlArg::F_SETFL(OFlag::from_bits_retain(current_flags) | OFlag::O_NONBLOCK),
    )
    .map_err(nix_io_error)?;
    let address = UnixAddr::new(path).map_err(nix_io_error)?;
    match connect(socket.as_raw_fd(), &address) {
        Ok(()) => {}
        Err(Errno::EINPROGRESS | Errno::EWOULDBLOCK) => {
            wait_for_connect_ready(socket.as_fd(), timeout)?;
            let socket_error = getsockopt(&socket, SocketError).map_err(nix_io_error)?;
            if socket_error != 0 {
                return Err(io::Error::from_raw_os_error(socket_error));
            }
        }
        Err(error) => return Err(nix_io_error(error)),
    }
    let stream = UnixStream::from(socket);
    stream.set_nonblocking(false)?;
    Ok(stream)
}

#[cfg(unix)]
fn wait_for_connect_ready(fd: std::os::fd::BorrowedFd<'_>, timeout: Duration) -> io::Result<()> {
    use nix::poll::{PollFd, PollFlags, poll};

    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "daemon connect timed out",
            ));
        }
        let timeout_ms = remaining.as_millis().clamp(1, u16::MAX.into()) as u16;
        let mut descriptors = [PollFd::new(
            fd,
            PollFlags::POLLOUT | PollFlags::POLLERR | PollFlags::POLLHUP,
        )];
        if poll(&mut descriptors, timeout_ms).map_err(nix_io_error)? == 0 {
            continue;
        }
        return Ok(());
    }
}

#[cfg(unix)]
fn nix_io_error(error: nix::errno::Errno) -> io::Error {
    io::Error::from_raw_os_error(error as i32)
}

#[cfg(unix)]
pub(super) fn connect_error(options: &ClientOptions, error: io::Error) -> ClientError {
    if error.kind() == io::ErrorKind::TimedOut {
        map_io_error(options, error, "connect to daemon")
    } else {
        unavailable(options, error)
    }
}
