#[cfg(unix)]
mod connect;
mod errors;
mod process;
#[cfg(all(test, unix))]
mod tests;
mod transport;
#[cfg(unix)]
use connect::connect_error;
#[cfg(unix)]
pub use connect::connect_unix;
use errors::{lifecycle_error, map_io_error, should_autostart, unavailable};
use process::terminate_child;

use crate::{
    DaemonError, Frame, MAX_FRAME_BYTES, Response, codes, default_log_path, default_socket_path,
    redact_json, redact_str,
};
use serde_json::Value;
#[cfg(unix)]
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::fd::{AsFd, AsRawFd};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(unix)]
use std::path::Path;
use std::{
    io,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct StartOptions {
    pub executable: PathBuf,
    pub log_path: PathBuf,
    pub args: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ClientOptions {
    pub socket_path: PathBuf,
    pub session: String,
    pub read_timeout: Duration,
    pub startup_timeout: Duration,
    pub autostart: bool,
    pub start: Option<StartOptions>,
    pub expected_engine: Option<String>,
    pub expected_policy: Option<crate::PolicyStatus>,
}
impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path("default"),
            session: "default".into(),
            read_timeout: resolved_read_timeout(),
            startup_timeout: Duration::from_millis(crate::DEFAULT_STARTUP_TIMEOUT_MS),
            autostart: std::env::var("SYMBROWSE_NO_AUTOSTART").as_deref() != Ok("1"),
            start: None,
            expected_engine: None,
            expected_policy: None,
        }
    }
}

#[derive(Debug)]
pub enum ClientError {
    Transport(DaemonError),
    Io(io::Error),
    Unsupported,
}
impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(error) => error.fmt(f),
            Self::Io(error) => write!(f, "{}", redact_str(&error.to_string())),
            Self::Unsupported => f.write_str("daemon sockets are not supported on this platform"),
        }
    }
}
impl std::error::Error for ClientError {}
impl From<io::Error> for ClientError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct Client {
    options: ClientOptions,
}
impl Client {
    pub fn new(mut options: ClientOptions) -> Self {
        if options.session.is_empty() {
            options.session = "default".into();
        }
        if options.socket_path.as_os_str().is_empty() {
            options.socket_path = default_socket_path(&options.session);
        }
        if options.read_timeout.is_zero() {
            options.read_timeout = resolved_read_timeout();
        }
        if options.startup_timeout.is_zero() {
            options.startup_timeout = Duration::from_millis(crate::DEFAULT_STARTUP_TIMEOUT_MS);
        }
        Self { options }
    }
    pub fn options(&self) -> &ClientOptions {
        &self.options
    }
    pub fn request(&self, mut frame: Frame) -> Result<Response, ClientError> {
        if frame.session.is_empty() {
            frame.session = self.options.session.clone();
        }
        if !crate::validate_session(&frame.session) {
            return Err(ClientError::Transport(DaemonError {
                code: codes::INVALID_SESSION.into(),
                message: format!("invalid session {:?}", redact_str(&frame.session)),
                ..Default::default()
            }));
        }
        match self.checked_request(&frame) {
            Ok(response) => Ok(response),
            Err(error) if self.options.autostart && should_autostart(&error) => {
                let mut child = self.start_daemon()?;
                let deadline = Instant::now() + self.options.startup_timeout;
                loop {
                    match self.checked_request(&frame) {
                        Ok(response) => {
                            // Dropping Child closes this client's process handle
                            // without killing the detached daemon. Forgetting it
                            // leaks the handle for every autostarted request.
                            drop(child);
                            return Ok(response);
                        }
                        Err(error) if !should_autostart(&error) => {
                            terminate_child(&mut child);
                            return Err(error);
                        }
                        Err(_) => {}
                    }
                    if Instant::now() >= deadline {
                        break;
                    }
                    thread::sleep(
                        Duration::from_millis(25)
                            .min(deadline.saturating_duration_since(Instant::now())),
                    );
                }
                terminate_child(&mut child);
                Err(lifecycle_error(
                    &self.options,
                    format!(
                        "daemon did not become ready for session {:?}",
                        self.options.session
                    ),
                ))
            }
            Err(error) => Err(error),
        }
    }
    pub fn request_without_autostart(&self, mut frame: Frame) -> Result<Response, ClientError> {
        if frame.session.is_empty() {
            frame.session = self.options.session.clone();
        }
        self.checked_request(&frame)
    }

    fn checked_request(&self, frame: &Frame) -> Result<Response, ClientError> {
        // Plain CLI clients share Go's one-request transport. Explicitly
        // configured clients validate first, before any state-changing command.
        if (self.options.expected_engine.is_some() || self.options.expected_policy.is_some())
            && !matches!(frame.cmd.as_str(), "daemon.status" | "daemon.stop")
        {
            self.verify_status()?;
        }
        self.request_once(frame)
    }

    fn verify_status(&self) -> Result<(), ClientError> {
        let status = self.request_once(&Frame {
            cmd: "daemon.status".into(),
            session: self.options.session.clone(),
            ..Frame::default()
        })?;
        if !status.success {
            return Err(ClientError::Transport(status.error.unwrap_or_else(|| {
                DaemonError {
                    code: codes::OPERATION_FAILED.into(),
                    message: "daemon status request failed".into(),
                    ..Default::default()
                }
            })));
        }
        let data = status.data.unwrap_or(Value::Null);
        let session_ok = data
            .get("session")
            .and_then(Value::as_str)
            .is_none_or(|session| session == self.options.session);
        let engine_ok = self
            .options
            .expected_engine
            .as_ref()
            .is_none_or(|expected| {
                data.get("engine").and_then(Value::as_str) == Some(expected.as_str())
            });
        let policy_ok = self
            .options
            .expected_policy
            .as_ref()
            .is_none_or(|expected| {
                serde_json::to_value(expected)
                    .ok()
                    .is_some_and(|value| data.get("policy") == Some(&value))
            });
        if session_ok && engine_ok && policy_ok {
            return Ok(());
        }
        let _ = self.request_once(&Frame {
            cmd: "daemon.stop".into(),
            session: self.options.session.clone(),
            ..Frame::default()
        });
        Err(ClientError::Transport(DaemonError {
            code: codes::DAEMON_UNAVAILABLE.into(),
            message: "existing daemon configuration is incompatible; it was stopped".into(),
            hint: "retry to start a daemon with the requested session configuration".into(),
            retryable: Some(true),
            ..Default::default()
        }))
    }
}

fn resolved_read_timeout() -> Duration {
    use symbrowse_core::config::{FlagOverrides, LoadContext, load};
    let seconds = LoadContext::from_process(FlagOverrides::default())
        .ok()
        .and_then(|context| load(&context).ok())
        .map(|result| result.config.read_timeout)
        .or_else(|| {
            std::env::var("SYMBROWSE_READ_TIMEOUT")
                .ok()?
                .parse::<i64>()
                .ok()
        })
        .filter(|seconds| *seconds > 0)
        .unwrap_or(30);
    Duration::from_secs(seconds as u64)
}
