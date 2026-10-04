use super::*;

impl Client {
    #[cfg(unix)]
    pub(super) fn request_once(&self, frame: &Frame) -> Result<Response, ClientError> {
        let mut stream = connect_unix(&self.options.socket_path, self.options.read_timeout)
            .map_err(|error| connect_error(&self.options, error))?;
        stream
            .set_read_timeout(Some(self.options.read_timeout))
            .map_err(|error| map_io_error(&self.options, error, "set daemon read deadline"))?;
        stream
            .set_write_timeout(Some(self.options.read_timeout))
            .map_err(|error| map_io_error(&self.options, error, "set daemon write deadline"))?;
        let mut payload = serde_json::to_vec(frame).map_err(io::Error::other)?;
        if payload.len() >= MAX_FRAME_BYTES {
            return Err(ClientError::Transport(DaemonError {
                code: codes::MALFORMED_REQUEST.into(),
                message: "daemon frame exceeds size limit".into(),
                ..Default::default()
            }));
        }
        payload.push(b'\n');
        stream
            .write_all(&payload)
            .map_err(|error| map_io_error(&self.options, error, "write daemon frame"))?;
        stream
            .flush()
            .map_err(|error| map_io_error(&self.options, error, "flush daemon frame"))?;
        let mut reader = BufReader::new(stream);
        read_response(&mut reader).map_err(|error| match error {
            ClientError::Io(error) => map_io_error(&self.options, error, "read daemon response"),
            other => other,
        })
    }
    #[cfg(windows)]
    pub(super) fn request_once(&self, frame: &Frame) -> Result<Response, ClientError> {
        use interprocess::{
            ConnectWaitMode,
            os::windows::named_pipe::{pipe_mode, tokio::PipeStream},
        };
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let path = self.options.socket_path.to_string_lossy();
        let mut payload = serde_json::to_vec(frame).map_err(io::Error::other)?;
        if payload.len() >= MAX_FRAME_BYTES {
            return Err(ClientError::Transport(DaemonError {
                code: codes::MALFORMED_REQUEST.into(),
                message: "daemon frame exceeds size limit".into(),
                ..Default::default()
            }));
        }
        payload.push(b'\n');
        let timeout = self.options.read_timeout;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(ClientError::Io)?;
        runtime.block_on(async {
            let mut stream = tokio::time::timeout(
                timeout,
                PipeStream::<pipe_mode::Bytes, pipe_mode::Bytes>::connect_by_path_with_wait_mode(
                    path.as_ref(),
                    ConnectWaitMode::Timeout(timeout),
                ),
            )
            .await
            .map_err(|_| unavailable(&self.options, timed_out("connect to daemon")))?
            .map_err(|error| unavailable(&self.options, error))?;
            tokio::time::timeout(timeout, async {
                stream.write_all(&payload).await?;
                stream.flush().await?;
                let mut response = Vec::new();
                let mut chunk = [0_u8; 8192];
                loop {
                    let read = stream.read(&mut chunk).await?;
                    if read == 0 {
                        break;
                    }
                    if response.len().saturating_add(read) > MAX_FRAME_BYTES {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "daemon response exceeds size limit",
                        ));
                    }
                    response.extend_from_slice(&chunk[..read]);
                    if let Some(end) = response.iter().position(|byte| *byte == b'\n') {
                        response.truncate(end);
                        break;
                    }
                }
                if response.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "daemon closed connection without a response",
                    ));
                }
                serde_json::from_slice(&response).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("decode daemon response: {error}"),
                    )
                })
            })
            .await
            .map_err(|_| map_io_error(&self.options, timed_out("daemon I/O"), "daemon I/O"))?
            .map_err(|error| map_io_error(&self.options, error, "daemon I/O"))
        })
    }
    #[cfg(not(any(unix, windows)))]
    pub(super) fn request_once(&self, _frame: &Frame) -> Result<Response, ClientError> {
        Err(ClientError::Unsupported)
    }
}

#[cfg(unix)]
fn read_response<R: BufRead>(reader_source: &mut R) -> Result<Response, ClientError> {
    let mut reader = reader_source;
    let mut line = Vec::new();
    read_limited_line(&mut reader, &mut line, MAX_FRAME_BYTES)?;
    if line.is_empty() {
        return Err(ClientError::Io(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "daemon closed connection without a response",
        )));
    }
    serde_json::from_slice(line.trim_ascii_end()).map_err(|error| {
        ClientError::Transport(DaemonError {
            code: codes::OPERATION_FAILED.into(),
            message: format!("decode daemon response: {error}"),
            ..Default::default()
        })
    })
}

#[cfg(unix)]
fn read_limited_line<R: BufRead>(
    reader: &mut R,
    output: &mut Vec<u8>,
    limit: usize,
) -> io::Result<()> {
    output.clear();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(());
        }
        let take = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |n| n + 1);
        if output.len().saturating_add(take) > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "daemon response exceeds size limit",
            ));
        }
        output.extend_from_slice(&available[..take]);
        reader.consume(take);
        if output.last() == Some(&b'\n') {
            return Ok(());
        }
    }
}

#[cfg(windows)]
fn timed_out(operation: &str) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, operation)
}
