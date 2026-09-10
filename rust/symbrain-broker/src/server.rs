use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::Duration;

use serde_json::value::RawValue;

use crate::client::{BrokerError, Client, Options};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum State {
    Idle = 0,
    Starting = 1,
    Ready = 2,
    Degraded = 3,
    Stopped = 4,
}

impl State {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Stopped => "stopped",
        }
    }
}

impl From<i32> for State {
    fn from(value: i32) -> Self {
        match value {
            1 => Self::Starting,
            2 => Self::Ready,
            3 => Self::Degraded,
            4 => Self::Stopped,
            _ => Self::Idle,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub name: String,
    pub binary_path: String,
    pub args: Vec<String>,
    pub init_timeout: Duration,
    pub call_timeout: Duration,
    pub max_restarts: u32,
    pub backoff_base: Duration,
    pub shutdown_timeout: Duration,
    pub env: Option<Vec<(String, String)>>,
    pub capture_stderr: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: String::new(),
            binary_path: String::new(),
            args: Vec::new(),
            init_timeout: Duration::from_secs(10),
            call_timeout: Duration::ZERO,
            max_restarts: 0,
            backoff_base: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(5),
            env: None,
            capture_stderr: false,
        }
    }
}

struct SharedState {
    cfg: Config,
    state: AtomicI32,
    spawn_lock: Mutex<()>,
    client: Mutex<Option<Arc<Client>>>,
    restart_count: AtomicU32,
    last_error: Mutex<Option<String>>,
}

/// `ManagedServer` wraps a client with lifecycle management: lazy spawn,
/// crash detection, backoff restart, and graceful shutdown.
pub struct ManagedServer {
    shared: Arc<SharedState>,
}

impl ManagedServer {
    #[must_use]
    pub fn new(cfg: Config) -> Self {
        Self {
            shared: Arc::new(SharedState {
                cfg,
                state: AtomicI32::new(State::Idle as i32),
                spawn_lock: Mutex::new(()),
                client: Mutex::new(None),
                restart_count: AtomicU32::new(0),
                last_error: Mutex::new(None),
            }),
        }
    }

    #[must_use]
    pub fn state(&self) -> State {
        State::from(self.shared.state.load(Ordering::SeqCst))
    }

    /// Returns the last fatal error message, if any.
    ///
    /// # Panics
    /// Panics when the mutex is poisoned.
    #[must_use]
    pub fn last_error(&self) -> Option<String> {
        self.shared
            .last_error
            .lock()
            .expect("last_error lock")
            .clone()
    }

    #[must_use]
    pub fn restart_count(&self) -> u32 {
        self.shared.restart_count.load(Ordering::SeqCst)
    }

    /// Returns retained diagnostics from the current child when capture is enabled.
    ///
    /// # Panics
    /// Panics when the client mutex is poisoned.
    #[must_use]
    pub fn stderr_bytes(&self) -> Vec<u8> {
        self.shared
            .client
            .lock()
            .expect("client lock")
            .as_ref()
            .map_or_else(Vec::new, |client| client.stderr_bytes())
    }

    fn ensure_ready(&self) -> Result<Arc<Client>, BrokerError> {
        if let Some(client) = self.shared.client.lock().expect("client lock").clone() {
            return Ok(client);
        }

        match self.state() {
            State::Degraded => {
                return Err(BrokerError::Closed {
                    op: "ensure_ready".to_string(),
                    detail: format!("server {} is degraded", self.shared.cfg.name),
                });
            }
            State::Stopped => {
                return Err(BrokerError::Closed {
                    op: "ensure_ready".to_string(),
                    detail: format!("server {} is stopped", self.shared.cfg.name),
                });
            }
            _ => {}
        }

        self.spawn_and_init()
    }

    fn spawn_and_init(&self) -> Result<Arc<Client>, BrokerError> {
        let _spawn_guard = self.shared.spawn_lock.lock().expect("spawn lock");
        if let Some(client) = self.shared.client.lock().expect("client lock").clone() {
            return Ok(client);
        }
        match self.state() {
            State::Degraded | State::Stopped => {
                return Err(BrokerError::Closed {
                    op: "spawn".to_string(),
                    detail: format!(
                        "server {} is {}",
                        self.shared.cfg.name,
                        self.state().as_str()
                    ),
                });
            }
            _ => {}
        }
        self.shared
            .state
            .store(State::Starting as i32, Ordering::SeqCst);
        let opts = Options {
            args: self.shared.cfg.args.clone(),
            env: self.shared.cfg.env.clone(),
            capture_stderr: self.shared.cfg.capture_stderr,
        };
        let client = Client::spawn(&self.shared.cfg.binary_path, opts).map_err(|err| {
            self.shared
                .state
                .store(State::Idle as i32, Ordering::SeqCst);
            BrokerError::Closed {
                op: "spawn".to_string(),
                detail: err.to_string(),
            }
        })?;

        if let Err(err) = client.initialize(self.shared.cfg.init_timeout) {
            let _ = client.kill();
            if matches!(err, BrokerError::ProtocolMismatch { .. }) {
                self.shared
                    .state
                    .store(State::Degraded as i32, Ordering::SeqCst);
                *self.shared.last_error.lock().expect("last_error lock") = Some(err.to_string());
                return Err(err);
            }
            self.shared
                .state
                .store(State::Idle as i32, Ordering::SeqCst);
            return Err(BrokerError::Closed {
                op: "initialize".to_string(),
                detail: err.to_string(),
            });
        }

        let client = Arc::new(client);
        *self.shared.client.lock().expect("client lock") = Some(Arc::clone(&client));
        self.shared.restart_count.store(0, Ordering::SeqCst);
        *self.shared.last_error.lock().expect("last_error lock") = None;
        self.shared
            .state
            .store(State::Ready as i32, Ordering::SeqCst);

        let weak: Weak<SharedState> = Arc::downgrade(&self.shared);
        let client_for_watch = Arc::clone(&client);
        thread::spawn(move || watch_child(weak, client_for_watch));

        Ok(client)
    }

    /// Lists tools, spawning the child on first use.
    ///
    /// # Errors
    /// Returns closed or timeout errors from the child.
    pub fn list_tools(&self) -> Result<Vec<crate::client::Tool>, BrokerError> {
        let client = self.ensure_ready()?;
        let timeout = if self.shared.cfg.call_timeout.is_zero() {
            Duration::from_secs(30)
        } else {
            self.shared.cfg.call_timeout
        };
        client.list_tools(timeout)
    }

    /// Calls one tool, spawning the child on first use.
    ///
    /// # Errors
    /// Returns closed, timeout, RPC, or parse errors from the child.
    pub fn call_tool(
        &self,
        name: &str,
        args: Option<&RawValue>,
    ) -> Result<crate::client::CallToolResult, BrokerError> {
        let client = self.ensure_ready()?;
        let timeout = if self.shared.cfg.call_timeout.is_zero() {
            Duration::from_secs(30)
        } else {
            self.shared.cfg.call_timeout
        };
        client.call_tool(name, args, timeout)
    }

    /// Calls one tool while polling a connection cancellation predicate.
    ///
    /// # Errors
    /// Returns cancellation, closed, timeout, RPC, or parse errors from the child.
    pub fn call_tool_with_cancel(
        &self,
        name: &str,
        args: Option<&RawValue>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<crate::client::CallToolResult, BrokerError> {
        let client = self.ensure_ready()?;
        let timeout = if self.shared.cfg.call_timeout.is_zero() {
            Duration::from_secs(30)
        } else {
            self.shared.cfg.call_timeout
        };
        client.call_tool_with_cancel(name, args, timeout, cancelled)
    }

    /// Gracefully stops the child: stdin EOF, SIGTERM, then kill after timeout.
    ///
    /// # Panics
    /// Panics when a mutex is poisoned.
    pub fn shutdown(&self) {
        if self.state() == State::Stopped {
            return;
        }
        self.shared
            .state
            .store(State::Stopped as i32, Ordering::SeqCst);
        let client = self.shared.client.lock().expect("client lock").take();
        if let Some(client) = client {
            let _ = client.close();
            let _ = client.terminate();
            let deadline = std::time::Instant::now() + self.shared.cfg.shutdown_timeout;
            loop {
                if client.exited().is_some() {
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    let _ = client.kill();
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn watch_child(weak: Weak<SharedState>, client: Arc<Client>) {
    let _ = client.wait();
    let Some(shared) = weak.upgrade() else { return };
    if matches!(
        State::from(shared.state.load(Ordering::SeqCst)),
        State::Stopped | State::Degraded
    ) {
        return;
    }

    shared.client.lock().expect("client lock").take();

    let restarts = shared.restart_count.load(Ordering::SeqCst);
    if restarts >= shared.cfg.max_restarts {
        shared.state.store(State::Degraded as i32, Ordering::SeqCst);
        *shared.last_error.lock().expect("last_error lock") = Some(format!(
            "server {}: restart budget exhausted",
            shared.cfg.name
        ));
        return;
    }

    shared.restart_count.store(restarts + 1, Ordering::SeqCst);
    let backoff = shared
        .cfg
        .backoff_base
        .saturating_mul(1 << restarts.min(20));
    let weak_for_restart = Arc::downgrade(&shared);
    thread::spawn(move || {
        thread::sleep(backoff);
        let Some(shared) = weak_for_restart.upgrade() else {
            return;
        };
        if matches!(
            State::from(shared.state.load(Ordering::SeqCst)),
            State::Stopped | State::Degraded
        ) {
            return;
        }
        if shared.client.lock().expect("client lock").is_some() {
            return;
        }
        let ms = ManagedServer {
            shared: Arc::clone(&shared),
        };
        let _ = ms.spawn_and_init();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_strings_are_stable() {
        assert_eq!(State::Idle.as_str(), "idle");
        assert_eq!(State::Starting.as_str(), "starting");
        assert_eq!(State::Ready.as_str(), "ready");
        assert_eq!(State::Degraded.as_str(), "degraded");
        assert_eq!(State::Stopped.as_str(), "stopped");
    }

    #[test]
    fn config_defaults_match_go() {
        let cfg = Config::default();
        assert_eq!(cfg.init_timeout, Duration::from_secs(10));
        assert_eq!(cfg.shutdown_timeout, Duration::from_secs(5));
        assert_eq!(cfg.backoff_base, Duration::from_secs(1));
        assert_eq!(cfg.max_restarts, 0);
    }
}
