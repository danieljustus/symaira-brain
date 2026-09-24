#![deny(unsafe_code)]

//! Versioned, bounded NDJSON boundary for the legacy Go/AzureTLS transport.
//! The wire profile is intentionally not a browser identity: callers select
//! `compat` explicitly and receive only transport data.

use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 1 << 20;
pub const MAX_BODY_BYTES: usize = 10 << 20;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Frame {
    Handshake {
        protocol: u32,
        component: String,
        oracle: String,
    },
    HandshakeAck {
        protocol: u32,
        component: String,
        oracle: String,
    },
    Request(Request),
    Response(Response),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Request {
    pub id: u64,
    pub method: String,
    pub url: String,
    pub profile: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: String,
    pub timeout_ms: u64,
    pub max_body_bytes: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Response {
    pub id: u64,
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub status: u16,
    #[serde(default)]
    pub final_url: String,
    #[serde(default)]
    pub headers: Vec<(String, Vec<String>)>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub error: Option<TypedError>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TypedError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointPermissions {
    pub path: PathBuf,
    pub directory_mode: u32,
    pub endpoint_mode: u32,
    pub private: bool,
}

#[derive(Debug)]
pub enum CompatError {
    Io(io::Error),
    Json(serde_json::Error),
    Protocol(TypedError),
    Timeout,
    SidecarExited,
    Integrity(TypedError),
    FrameTooLarge,
}
impl std::fmt::Display for CompatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CompatError {}
impl From<io::Error> for CompatError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<serde_json::Error> for CompatError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

/// Creates the private runtime directory used by endpoint-based deployments.
/// The default stdio transport remains portable; this schema is shared by
/// socket and stdio launchers and is tested so a future socket cannot weaken it.
pub fn private_endpoint(
    root: impl AsRef<Path>,
    name: &str,
) -> Result<EndpointPermissions, CompatError> {
    let dir = root.as_ref().join("symbrowse-compat");
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let path = dir.join(name);
    Ok(EndpointPermissions {
        path,
        directory_mode: 0o700,
        endpoint_mode: 0o600,
        private: true,
    })
}

pub struct CompatClient {
    command: String,
    args: Vec<String>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
    next_id: AtomicU64,
    request_timeout: Duration,
}
impl CompatClient {
    pub fn new(
        command: impl Into<String>,
        args: impl IntoIterator<Item = String>,
        request_timeout: Duration,
    ) -> Self {
        Self {
            command: command.into(),
            args: args.into_iter().collect(),
            child: None,
            stdin: None,
            stdout: None,
            next_id: AtomicU64::new(1),
            request_timeout,
        }
    }
    pub fn from_environment(timeout: Duration) -> Self {
        let command =
            std::env::var("SYMBROWSE_COMPAT_BINARY").unwrap_or_else(|_| "symbrowse".into());
        Self::new(command, ["compat-sidecar".to_owned()], timeout)
    }
    async fn start(&mut self) -> Result<(), CompatError> {
        let mut command = Command::new(&self.command);
        command
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            // Handshake failures and canceled startup futures must not leave
            // an untracked compatibility process behind.
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|_| CompatError::SidecarExited)?;
        let mut stdin = child.stdin.take().ok_or(CompatError::SidecarExited)?;
        let stdout = child.stdout.take().ok_or(CompatError::SidecarExited)?;
        write_frame(
            &mut stdin,
            &Frame::Handshake {
                protocol: PROTOCOL_VERSION,
                component: "symbrowse-rust".into(),
                oracle: "go-azuretls-v0.8.0".into(),
            },
        )
        .await?;
        let mut reader = BufReader::new(stdout);
        let line = read_line(&mut reader).await?;
        let ack: Frame = serde_json::from_slice(&line)?;
        validate_handshake_ack(ack)?;
        self.stdin = Some(stdin);
        self.stdout = Some(reader);
        self.child = Some(child);
        Ok(())
    }
    pub async fn request(&mut self, mut request: Request) -> Result<Response, CompatError> {
        for attempt in 0..2 {
            if self.stdin.is_none() {
                self.start().await?;
            }
            request.id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let result = async {
                write_frame(
                    self.stdin.as_mut().ok_or(CompatError::SidecarExited)?,
                    &Frame::Request(request.clone()),
                )
                .await?;
                let line =
                    read_line(self.stdout.as_mut().ok_or(CompatError::SidecarExited)?).await?;
                let frame: Frame = serde_json::from_slice(&line)?;
                match frame {
                    Frame::Response(response) if response.id == request.id => Ok(response),
                    _ => Err(CompatError::Integrity(TypedError {
                        code: "compat_request_id_mismatch".into(),
                        message: "sidecar response id did not match request".into(),
                        retryable: false,
                    })),
                }
            }
            .await;
            match result {
                Ok(response) => return Ok(response),
                Err(error)
                    if attempt == 0
                        && matches!(error, CompatError::Io(_) | CompatError::SidecarExited) =>
                {
                    self.restart().await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(CompatError::SidecarExited)
    }
    pub async fn request_with_timeout(
        &mut self,
        request: Request,
    ) -> Result<Response, CompatError> {
        match timeout(self.request_timeout, self.request(request)).await {
            Ok(result) => result,
            Err(_) => {
                // A timed-out response may still be queued on stdout. Retire
                // that process so a later request cannot consume the stale
                // response and misattribute it to a new request ID.
                self.restart().await;
                Err(CompatError::Timeout)
            }
        }
    }
    /// Close stdin to request the sidecar's normal EOF exit and wait for it.
    /// A bounded kill is used if the process does not honor that protocol exit.
    pub async fn shutdown(&mut self) -> Result<(), CompatError> {
        self.stdin.take();
        self.stdout.take();
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        match timeout(self.request_timeout, child.wait()).await {
            Ok(Ok(status)) if status.success() => Ok(()),
            Ok(Ok(_)) => Err(CompatError::SidecarExited),
            Ok(Err(error)) => Err(CompatError::Io(error)),
            Err(_) => {
                let _ = child.kill().await;
                Err(CompatError::Timeout)
            }
        }
    }
    async fn restart(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
        self.stdin = None;
        self.stdout = None;
    }
}
fn validate_handshake_ack(ack: Frame) -> Result<(), CompatError> {
    match ack {
        Frame::HandshakeAck {
            protocol,
            component,
            oracle,
        } if protocol == PROTOCOL_VERSION
            && component == "symbrowse-go"
            && oracle == "go-azuretls-v0.8.0" =>
        {
            Ok(())
        }
        Frame::HandshakeAck { protocol, .. } => Err(CompatError::Integrity(TypedError {
            code: if protocol != PROTOCOL_VERSION {
                "compat_protocol_mismatch"
            } else {
                "compat_identity_mismatch"
            }
            .into(),
            message: format!(
                "sidecar handshake did not match the pinned protocol and oracle (protocol {protocol})"
            ),
            retryable: false,
        })),
        _ => Err(CompatError::Integrity(TypedError {
            code: "compat_handshake_invalid".into(),
            message: "sidecar did not acknowledge handshake".into(),
            retryable: false,
        })),
    }
}
async fn write_frame(stdin: &mut ChildStdin, frame: &Frame) -> Result<(), CompatError> {
    let bytes = encode_frame(frame)?;
    stdin.write_all(&bytes).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    Ok(())
}
fn encode_frame(frame: &Frame) -> Result<Vec<u8>, CompatError> {
    let bytes = serde_json::to_vec(frame)?;
    if bytes.len().saturating_add(1) > MAX_FRAME_BYTES {
        return Err(CompatError::FrameTooLarge);
    }
    Ok(bytes)
}
async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Vec<u8>, CompatError> {
    let mut line = Vec::new();
    loop {
        let (consumed, complete, too_large) = {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                return Err(CompatError::SidecarExited);
            }
            let remaining = MAX_FRAME_BYTES.saturating_add(1).saturating_sub(line.len());
            let scan = available.len().min(remaining);
            let newline = available[..scan].iter().position(|byte| *byte == b'\n');
            let consumed = newline.map_or(scan, |index| index + 1);
            let too_large = line.len().saturating_add(consumed) > MAX_FRAME_BYTES;
            line.extend_from_slice(&available[..consumed]);
            reader.consume(consumed);
            (consumed, newline.is_some(), too_large)
        };
        if too_large || consumed == 0 {
            return Err(CompatError::FrameTooLarge);
        }
        if complete {
            return Ok(line);
        }
        if line.len() >= MAX_FRAME_BYTES {
            return Err(CompatError::FrameTooLarge);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::io::AsyncWriteExt;

    struct TestRoot(PathBuf);
    impl TestRoot {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos();
            for _ in 0..8 {
                let path = std::env::temp_dir().join(format!(
                    "symbrowse-compat-test-{}-{timestamp}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create unique test root: {error}"),
                }
            }
            panic!("could not allocate unique test root")
        }
    }
    impl Drop for TestRoot {
        fn drop(&mut self) {
            // This exact directory was reserved with create_dir in new().
            std::fs::remove_dir_all(&self.0).expect("remove owned test root");
        }
    }

    #[test]
    fn private_runtime_directory_schema_and_unix_mode() {
        let root = TestRoot::new();
        let endpoint = private_endpoint(&root.0, "sidecar.sock").unwrap();
        assert!(endpoint.private);
        assert_eq!(endpoint.directory_mode, 0o700);
        assert_eq!(endpoint.endpoint_mode, 0o600);
        assert!(
            std::fs::metadata(root.0.join("symbrowse-compat"))
                .unwrap()
                .is_dir()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(root.0.join("symbrowse-compat"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700);
        }
    }
    #[test]
    fn frames_are_versioned_and_typed() {
        let frame = Frame::Handshake {
            protocol: PROTOCOL_VERSION,
            component: "x".into(),
            oracle: "y".into(),
        };
        let raw = serde_json::to_string(&frame).unwrap();
        assert!(raw.contains("handshake"));
        assert!(raw.contains("protocol"));
    }

    #[test]
    fn handshake_pins_protocol_component_and_oracle() {
        assert!(
            validate_handshake_ack(Frame::HandshakeAck {
                protocol: PROTOCOL_VERSION,
                component: "symbrowse-go".into(),
                oracle: "go-azuretls-v0.8.0".into(),
            })
            .is_ok()
        );
        for (frame, expected_code) in [
            (
                Frame::HandshakeAck {
                    protocol: PROTOCOL_VERSION + 1,
                    component: "symbrowse-go".into(),
                    oracle: "go-azuretls-v0.8.0".into(),
                },
                "compat_protocol_mismatch",
            ),
            (
                Frame::HandshakeAck {
                    protocol: PROTOCOL_VERSION,
                    component: "other-sidecar".into(),
                    oracle: "go-azuretls-v0.8.0".into(),
                },
                "compat_identity_mismatch",
            ),
        ] {
            let Err(CompatError::Integrity(error)) = validate_handshake_ack(frame) else {
                panic!("handshake mismatch unexpectedly passed")
            };
            assert_eq!(error.code, expected_code);
        }
    }

    #[tokio::test]
    async fn ndjson_reader_bounds_frames_before_unbounded_growth() {
        for (length, expected_large) in [(MAX_FRAME_BYTES, false), (MAX_FRAME_BYTES + 1, true)] {
            let (mut writer, reader) = tokio::io::duplex(length + 1);
            let mut frame = vec![b'x'; length];
            if !expected_large {
                frame[length - 1] = b'\n';
            }
            writer.write_all(&frame).await.unwrap();
            drop(writer);
            let mut reader = BufReader::new(reader);
            let result = read_line(&mut reader).await;
            if expected_large {
                assert!(matches!(result, Err(CompatError::FrameTooLarge)));
            } else {
                assert_eq!(result.unwrap().len(), MAX_FRAME_BYTES);
            }
        }
    }

    #[test]
    fn outbound_frames_are_bounded_including_the_newline() {
        let mut frame = Frame::Handshake {
            protocol: PROTOCOL_VERSION,
            component: String::new(),
            oracle: String::new(),
        };
        let base_length = serde_json::to_vec(&frame).unwrap().len();
        let component_length = MAX_FRAME_BYTES - 1 - base_length;
        if let Frame::Handshake { component, .. } = &mut frame {
            component.push_str(&"x".repeat(component_length));
        }
        assert_eq!(encode_frame(&frame).unwrap().len() + 1, MAX_FRAME_BYTES);

        if let Frame::Handshake { component, .. } = &mut frame {
            component.push('x');
        }
        assert!(matches!(
            encode_frame(&frame),
            Err(CompatError::FrameTooLarge)
        ));
    }
}
