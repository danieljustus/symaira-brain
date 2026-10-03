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
pub type ShutdownHandler = Arc<dyn Fn() + Send + Sync + 'static>;

// Connections can stay open for several request frames. Bound the number of
// connection threads so short-lived CLI requests do not create one OS thread
// per daemon round trip, while retaining enough workers for concurrent clients.
#[cfg(any(unix, windows))]
const CONNECTION_WORKERS: usize = 32;

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
        self.cancelled.load(Ordering::Acquire)
            || self.shutdown.load(Ordering::Acquire)
            || self.remaining().is_zero()
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

#[derive(Clone)]
pub struct ServerOptions {
    pub socket_path: PathBuf,
    pub session: String,
    pub idle_timeout: Option<Duration>,
    pub operation_timeout: Duration,
    pub read_timeout: Duration,
    pub handler: Option<DaemonHandler>,
    pub shutdown_handler: Option<ShutdownHandler>,
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
            shutdown_handler: None,
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
                "invalid session {}: use 1-64 letters, digits, '.', '_' or '-'",
                crate::session_quote::quote(s)
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
                fetch_ssrf_enabled: !spec.allow_private,
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
                    crate::spec::default_session_cache_root()
                } else {
                    default_user_data_root()
                },
                pid: std::process::id(),
                scope: if configured_spec.is_some() {
                    "worktree".into()
                } else {
                    String::new()
                },
                origin_path: if configured_spec.is_some() {
                    crate::spec::worktree_origin()
                } else {
                    String::new()
                },
            }))
        });
        if options.handler.is_none() {
            let (handler, shutdown_handler) = crate::runtime::handlers(spec)
                .map_err(|error| ServerError::Io(io::Error::other(error.message)))?;
            options.handler = Some(handler);
            options.shutdown_handler = Some(shutdown_handler);
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
            let result = self.listen_unix();
            if let Some(shutdown) = &self.options.shutdown_handler {
                shutdown();
            }
            result
        }
        #[cfg(windows)]
        {
            let result = listen_windows(self);
            if let Some(shutdown) = &self.options.shutdown_handler {
                shutdown();
            }
            result
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(ServerError::Unsupported)
        }
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
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::*;
mod connection;
use connection::*;
mod helpers;
use helpers::*;
pub use helpers::{default_socket_path, socket_path, validate_session};
#[cfg(test)]
mod tests;
