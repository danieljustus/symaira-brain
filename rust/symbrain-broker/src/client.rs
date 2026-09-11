use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

const PROTOCOL_VERSION: &str = "2024-11-05";
const CLIENT_NAME: &str = "symbrain";
const CLIENT_VERSION: &str = "dev";
const MAX_LINE_BYTES: usize = 1 << 20;
const MAX_STDERR_BYTES: usize = 1 << 20;

#[derive(Debug)]
pub enum BrokerError {
    Closed { op: String, detail: String },
    Timeout { op: String },
    Cancelled { op: String },
    Rpc { code: i64, message: String },
    ProtocolMismatch { expected: String, actual: String },
    Io(io::Error),
    Parse(String),
}

impl fmt::Display for BrokerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed { op, detail } => {
                if detail.is_empty() {
                    write!(f, "broker: {op}: child closed")
                } else {
                    write!(f, "broker: {op}: child closed: {detail}")
                }
            }
            Self::Timeout { op } => write!(f, "broker: {op}: timeout"),
            Self::Cancelled { op } => write!(f, "broker: {op}: cancelled"),
            Self::Rpc { code, message } => write!(f, "broker: rpc error {code}: {message}"),
            Self::ProtocolMismatch { expected, actual } => {
                write!(
                    f,
                    "broker: protocol mismatch: want {expected}, got {actual}"
                )
            }
            Self::Io(err) => write!(f, "broker: {err}"),
            Self::Parse(detail) => write!(f, "broker: parse: {detail}"),
        }
    }
}

impl std::error::Error for BrokerError {}

impl From<io::Error> for BrokerError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// Resolves the child binary path: explicit override wins, then the managed
/// directory (`~/.symaira/bin`), then PATH.
///
/// # Errors
/// Returns an error when the binary is not found or not executable.
pub fn discover(binary_name: &str, override_path: &str) -> Result<String, BrokerError> {
    if !override_path.is_empty() {
        let path = std::path::Path::new(override_path);
        if is_usable_executable(path) {
            return Ok(override_path.to_string());
        }
        let kind = if path.exists() {
            io::ErrorKind::PermissionDenied
        } else {
            io::ErrorKind::NotFound
        };
        return Err(BrokerError::Io(io::Error::new(
            kind,
            format!(
                "broker: configured binary_path {override_path:?} for {binary_name:?} is not an executable regular file"
            ),
        )));
    }

    if let Ok(home) = std::env::var("HOME") {
        let managed_dir = std::path::PathBuf::from(home).join(".symaira").join("bin");
        if let Some(managed) = find_in_dir(&managed_dir, binary_name) {
            return Ok(managed.to_string_lossy().to_string());
        }
    }

    which(binary_name).ok_or_else(|| {
        BrokerError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("broker: {binary_name:?} not found on PATH or in managed directory"),
        ))
    })
}

fn is_usable_executable(path: &std::path::Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn find_in_dir(directory: &std::path::Path, binary: &str) -> Option<std::path::PathBuf> {
    executable_names(binary)
        .into_iter()
        .map(|name| directory.join(name))
        .find(|candidate| is_usable_executable(candidate))
}

fn executable_names(binary: &str) -> Vec<OsString> {
    #[cfg(windows)]
    {
        let extensions =
            std::env::var_os("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
        let mut names = vec![OsString::from(binary)];
        names.extend(
            extensions
                .to_string_lossy()
                .split(';')
                .filter(|ext| !ext.is_empty())
                .map(|ext| OsString::from(format!("{binary}{ext}"))),
        );
        names
    }
    #[cfg(not(windows))]
    {
        vec![OsString::from(binary)]
    }
}

fn which(binary: &str) -> Option<String> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .find_map(|dir| find_in_dir(&dir, binary))
            .map(|path| path.to_string_lossy().to_string())
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams<'a> {
    protocol_version: &'a str,
    capabilities: Capabilities,
    client_info: ClientInfo<'a>,
}

#[derive(Debug, Clone, Serialize)]
struct Capabilities {}

#[derive(Debug, Clone, Serialize)]
struct ClientInfo<'a> {
    name: &'a str,
    version: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub server_info: ServerInfo,
    #[serde(default)]
    pub capabilities: Option<Box<RawValue>>,
    #[serde(default)]
    pub instructions: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input_schema: Option<Box<RawValue>>,
    #[serde(default)]
    pub annotations: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Debug, Default)]
pub struct Options {
    pub args: Vec<String>,
    pub env: Option<Vec<(String, String)>>,
    pub capture_stderr: bool,
}

struct Inner {
    next_id: AtomicI64,
    pending: Mutex<HashMap<i64, Sender<Result<serde_json::Value, BrokerError>>>>,
    pid: u32,
    done: Mutex<Option<String>>,
}

pub struct Client {
    inner: Arc<Inner>,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    pid: u32,
    read_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    stderr_bytes: Arc<Mutex<Vec<u8>>>,
    stderr_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    exited: Mutex<Option<ExitStatus>>,
}

impl Client {
    /// Spawns a child process and starts its stdout reader.
    ///
    /// # Errors
    /// Returns an error when the process cannot be started.
    ///
    /// # Panics
    /// Panics when the piped stdin/stdout handles are absent (impossible after successful spawn).
    pub fn spawn(path: &str, opts: Options) -> Result<Self, BrokerError> {
        let mut cmd = Command::new(path);
        cmd.args(&opts.args);
        if let Some(env) = opts.env {
            cmd.env_clear();
            for (key, value) in env {
                cmd.env(key, value);
            }
        }
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(if opts.capture_stderr {
            Stdio::piped()
        } else {
            Stdio::inherit()
        });
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn().map_err(BrokerError::Io)?;
        let pid = child.id();
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take();

        let inner = Arc::new(Inner {
            next_id: AtomicI64::new(0),
            pending: Mutex::new(HashMap::new()),
            pid,
            done: Mutex::new(None),
        });
        let read_inner = Arc::clone(&inner);
        let read_thread = std::thread::spawn(move || read_loop(stdout, read_inner));
        let stderr_bytes = Arc::new(Mutex::new(Vec::new()));
        let stderr_thread = stderr.map(|stderr| {
            let retained = Arc::clone(&stderr_bytes);
            std::thread::spawn(move || capture_stderr(stderr, &retained))
        });

        Ok(Self {
            inner,
            stdin: Mutex::new(Some(stdin)),
            child: Mutex::new(Some(child)),
            pid,
            read_thread: Mutex::new(Some(read_thread)),
            stderr_bytes,
            stderr_thread: Mutex::new(stderr_thread),
            exited: Mutex::new(None),
        })
    }

    /// Performs the MCP initialize handshake.
    ///
    /// # Errors
    /// Returns timeout, closed, RPC, protocol-mismatch, or parse errors.
    pub fn initialize(&self, timeout: Duration) -> Result<InitializeResult, BrokerError> {
        let params = InitializeParams {
            protocol_version: PROTOCOL_VERSION,
            capabilities: Capabilities {},
            client_info: ClientInfo {
                name: CLIENT_NAME,
                version: CLIENT_VERSION,
            },
        };
        let raw = self.call("initialize", Some(&params), timeout, None)?;
        let result: InitializeResult = serde_json::from_str(raw.get())
            .map_err(|err| BrokerError::Parse(format!("initialize result: {err}")))?;
        if result.protocol_version != PROTOCOL_VERSION {
            return Err(BrokerError::ProtocolMismatch {
                expected: PROTOCOL_VERSION.to_string(),
                actual: result.protocol_version,
            });
        }
        let _ = self.notify("notifications/initialized", None::<&()>);
        Ok(result)
    }

    /// Lists tools advertised by the child.
    ///
    /// # Errors
    /// Returns timeout, closed, RPC, or parse errors.
    pub fn list_tools(&self, timeout: Duration) -> Result<Vec<Tool>, BrokerError> {
        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(default)]
            tools: Vec<Tool>,
        }
        let raw = self.call("tools/list", None::<&()>, timeout, None)?;
        let wrapper: Wrapper = serde_json::from_str(raw.get())
            .map_err(|err| BrokerError::Parse(format!("tools/list result: {err}")))?;
        Ok(wrapper.tools)
    }

    /// Calls one tool with raw JSON arguments.
    ///
    /// # Errors
    /// Returns timeout, closed, RPC, or parse errors. Tool-level failures arrive as
    /// `is_error = true` rather than as an error.
    pub fn call_tool(
        &self,
        name: &str,
        arguments: Option<&RawValue>,
        timeout: Duration,
    ) -> Result<CallToolResult, BrokerError> {
        #[derive(Serialize)]
        struct Params<'a> {
            name: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            arguments: Option<&'a RawValue>,
        }
        let params = Params { name, arguments };
        let raw = self.call("tools/call", Some(&params), timeout, None)?;
        serde_json::from_str(raw.get())
            .map_err(|err| BrokerError::Parse(format!("tools/call result: {err}")))
    }

    /// Calls one tool while polling a connection cancellation predicate.
    ///
    /// # Errors
    /// Returns cancellation, timeout, closed, RPC, or parse errors.
    pub fn call_tool_with_cancel(
        &self,
        name: &str,
        arguments: Option<&RawValue>,
        timeout: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<CallToolResult, BrokerError> {
        #[derive(Serialize)]
        struct Params<'a> {
            name: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            arguments: Option<&'a RawValue>,
        }
        let params = Params { name, arguments };
        let raw = self.call("tools/call", Some(&params), timeout, Some(cancelled))?;
        serde_json::from_str(raw.get())
            .map_err(|err| BrokerError::Parse(format!("tools/call result: {err}")))
    }

    fn call<T: Serialize + ?Sized>(
        &self,
        method: &str,
        params: Option<&T>,
        timeout: Duration,
        cancelled: Option<&dyn Fn() -> bool>,
    ) -> Result<Box<RawValue>, BrokerError> {
        if cancelled.is_some_and(|is_cancelled| is_cancelled()) {
            return Err(BrokerError::Cancelled {
                op: method.to_string(),
            });
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx): (Sender<_>, Receiver<_>) = mpsc::channel();

        {
            let mut pending = self.inner.pending.lock().expect("pending lock");
            if let Some(detail) = self.inner.done.lock().expect("done lock").clone() {
                return Err(BrokerError::Closed {
                    op: method.to_string(),
                    detail,
                });
            }
            pending.insert(id, tx);
        }

        let write_result = (|| {
            let mut request = serde_json::Map::new();
            request.insert(
                "jsonrpc".to_string(),
                serde_json::Value::String("2.0".to_string()),
            );
            request.insert("id".to_string(), serde_json::Value::Number(id.into()));
            request.insert(
                "method".to_string(),
                serde_json::Value::String(method.to_string()),
            );
            if let Some(params) = params {
                let params_value = serde_json::to_value(params)
                    .map_err(|err| BrokerError::Parse(format!("marshal {method} params: {err}")))?;
                request.insert("params".to_string(), params_value);
            }
            let mut data = serde_json::to_vec(&request)
                .map_err(|err| BrokerError::Parse(format!("marshal {method}: {err}")))?;
            data.push(b'\n');
            let mut stdin_guard = self.stdin.lock().expect("stdin lock");
            let stdin = stdin_guard.as_mut().ok_or_else(|| BrokerError::Closed {
                op: method.to_string(),
                detail: "stdin closed".to_string(),
            })?;
            stdin.write_all(&data).map_err(BrokerError::Io)
        })();

        if let Err(err) = write_result {
            let mut pending = self.inner.pending.lock().expect("pending lock");
            pending.remove(&id);
            return Err(BrokerError::Closed {
                op: method.to_string(),
                detail: err.to_string(),
            });
        }

        let recv_result = receive_response(&rx, method, timeout, cancelled);

        if recv_result.is_err() {
            self.inner.pending.lock().expect("pending lock").remove(&id);
        }
        let response = recv_result.and_then(|result| result)?;
        if let Some(error) = response.get("error") {
            let code = error
                .get("code")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or_default();
            let message = error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            return Err(BrokerError::Rpc { code, message });
        }
        let result = response
            .get("result")
            .ok_or_else(|| BrokerError::Parse("response missing result field".to_string()))?;
        let raw = serde_json::to_string(result)
            .map_err(|err| BrokerError::Parse(format!("serialize result: {err}")))?;
        RawValue::from_string(raw).map_err(|err| BrokerError::Parse(err.to_string()))
    }

    fn notify<T: Serialize + ?Sized>(
        &self,
        method: &str,
        params: Option<&T>,
    ) -> Result<(), BrokerError> {
        let mut request = serde_json::Map::new();
        request.insert(
            "jsonrpc".to_string(),
            serde_json::Value::String("2.0".to_string()),
        );
        request.insert(
            "method".to_string(),
            serde_json::Value::String(method.to_string()),
        );
        if let Some(params) = params {
            let params_value = serde_json::to_value(params)
                .map_err(|err| BrokerError::Parse(format!("marshal {method} params: {err}")))?;
            request.insert("params".to_string(), params_value);
        }
        let mut data = serde_json::to_vec(&request)
            .map_err(|err| BrokerError::Parse(format!("marshal {method}: {err}")))?;
        data.push(b'\n');
        let mut stdin_guard = self.stdin.lock().expect("stdin lock");
        let stdin = stdin_guard.as_mut().ok_or_else(|| BrokerError::Closed {
            op: method.to_string(),
            detail: "stdin closed".to_string(),
        })?;
        stdin.write_all(&data).map_err(BrokerError::Io)
    }

    /// Closes the child's stdin, signaling EOF.
    ///
    /// # Errors
    /// Returns an error when the lock is poisoned.
    ///
    /// # Panics
    /// Panics when the stdin mutex is poisoned.
    pub fn close(&self) -> Result<(), BrokerError> {
        let mut stdin_guard = self.stdin.lock().expect("stdin lock");
        drop(stdin_guard.take());
        Ok(())
    }

    /// Waits for the child to exit and returns its status.
    ///
    /// # Errors
    /// Returns an error when there is no child or waiting fails.
    ///
    /// # Panics
    /// Panics when a mutex is poisoned.
    pub fn wait(&self) -> Result<ExitStatus, BrokerError> {
        loop {
            if let Some(status) = *self.exited.lock().expect("exited lock") {
                return Ok(status);
            }
            let status = {
                let mut child_guard = self.child.lock().expect("child lock");
                let child = child_guard.as_mut().ok_or_else(|| BrokerError::Closed {
                    op: "wait".to_string(),
                    detail: "no child".to_string(),
                })?;
                child.try_wait().map_err(BrokerError::Io)?
            };
            if let Some(status) = status {
                *self.exited.lock().expect("exited lock") = Some(status);
                return Ok(status);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Returns the child's exit status if it has already exited.
    ///
    /// # Panics
    /// Panics when the exited mutex is poisoned.
    pub fn exited(&self) -> Option<ExitStatus> {
        *self.exited.lock().expect("exited lock")
    }

    /// Kills the child process immediately.
    ///
    /// # Errors
    /// Returns an error when the kill or wait fails.
    ///
    /// # Panics
    /// Panics when a mutex is poisoned.
    pub fn kill(&self) -> Result<(), BrokerError> {
        let mut child_guard = self.child.lock().expect("child lock");
        if let Some(child) = child_guard.as_mut() {
            #[cfg(unix)]
            signal_process_group(self.pid, "-KILL").map_err(BrokerError::Io)?;
            #[cfg(not(unix))]
            child.kill().map_err(BrokerError::Io)?;
            let status = child.wait().map_err(BrokerError::Io)?;
            *self.exited.lock().expect("exited lock") = Some(status);
        }
        Ok(())
    }

    /// Sends SIGTERM to the child's process group on Unix. Other platforms
    /// rely on stdin EOF followed by [`Self::kill`] after the deadline.
    ///
    /// # Errors
    /// Returns an error when SIGTERM delivery fails.
    pub fn terminate(&self) -> Result<(), BrokerError> {
        #[cfg(unix)]
        {
            signal_process_group(self.pid, "-TERM").map_err(BrokerError::Io)?;
        }
        Ok(())
    }

    /// Returns the child's process ID.
    pub const fn pid(&self) -> u32 {
        self.pid
    }

    /// Returns the retained tail of captured child diagnostics.
    ///
    /// # Panics
    /// Panics when the capture mutex is poisoned.
    #[must_use]
    pub fn stderr_bytes(&self) -> Vec<u8> {
        self.stderr_bytes.lock().expect("stderr lock").clone()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.close();
        let mut child_guard = self.child.lock().expect("child lock");
        if let Some(mut child) = child_guard.take() {
            #[cfg(unix)]
            let _ = signal_process_group(self.pid, "-KILL");
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(thread) = self.read_thread.lock().expect("read thread lock").take() {
            let _ = thread.join();
        }
        if let Some(thread) = self
            .stderr_thread
            .lock()
            .expect("stderr thread lock")
            .take()
        {
            let _ = thread.join();
        }
    }
}

#[cfg(unix)]
fn signal_process_group(pid: u32, signal: &str) -> io::Result<()> {
    let status = Command::new("/bin/kill")
        .arg(signal)
        .arg("--")
        .arg(format!("-{pid}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{signal} process group {pid} failed with {status}"
        )))
    }
}

fn receive_response(
    receiver: &Receiver<Result<serde_json::Value, BrokerError>>,
    method: &str,
    timeout: Duration,
    cancelled: Option<&dyn Fn() -> bool>,
) -> Result<Result<serde_json::Value, BrokerError>, BrokerError> {
    if let Some(cancelled) = cancelled {
        let deadline = (!timeout.is_zero()).then(|| Instant::now() + timeout);
        loop {
            if cancelled() {
                return Err(BrokerError::Cancelled {
                    op: method.to_string(),
                });
            }
            let wait = deadline.map_or(Duration::from_millis(10), |deadline| {
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(10))
            });
            if wait.is_zero() {
                return Err(BrokerError::Timeout {
                    op: method.to_string(),
                });
            }
            match receiver.recv_timeout(wait) {
                Ok(response) => return Ok(response),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(BrokerError::Closed {
                        op: method.to_string(),
                        detail: "child closed".to_string(),
                    });
                }
            }
        }
    }
    if timeout.is_zero() {
        receiver.recv().map_err(|_| BrokerError::Closed {
            op: method.to_string(),
            detail: "child closed".to_string(),
        })
    } else {
        receiver.recv_timeout(timeout).map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => BrokerError::Timeout {
                op: method.to_string(),
            },
            mpsc::RecvTimeoutError::Disconnected => BrokerError::Closed {
                op: method.to_string(),
                detail: "child closed".to_string(),
            },
        })
    }
}

#[allow(clippy::needless_pass_by_value)]
fn read_loop(stdout: ChildStdout, inner: Arc<Inner>) {
    let mut reader = BufReader::with_capacity(4096, stdout);
    let mut terminal_detail = "EOF".to_string();
    loop {
        match read_limited_line(&mut reader) {
            Ok((line, eof)) => {
                if !line.is_empty()
                    && let Ok(response) = serde_json::from_slice::<serde_json::Value>(&line)
                    && let Some(id) = response.get("id").and_then(serde_json::Value::as_i64)
                {
                    deliver(&inner, id, Ok(response));
                }
                if eof {
                    break;
                }
            }
            Err(err) => {
                terminal_detail = err.to_string();
                #[cfg(unix)]
                let _ = signal_process_group(inner.pid, "-KILL");
                break;
            }
        }
    }
    *inner.done.lock().expect("done lock") = Some(terminal_detail.clone());
    let mut pending = inner.pending.lock().expect("pending lock");
    for (_, tx) in pending.drain() {
        let _ = tx.send(Err(BrokerError::Closed {
            op: "read".to_string(),
            detail: terminal_detail.clone(),
        }));
    }
}

/// Reads one newline-delimited message without allowing the input reader to
/// grow a buffer beyond the protocol cap. A final unterminated line is valid
/// input for the purpose of delivering a response, then reports EOF.
fn read_limited_line<R: BufRead>(reader: &mut R) -> io::Result<(Vec<u8>, bool)> {
    let mut line = Vec::with_capacity(4096);
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok((line, true));
        }
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(buffer.len(), |index| index + 1);
        let needed = line.len().saturating_add(take);
        if needed > MAX_LINE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("broker: line exceeds {MAX_LINE_BYTES} bytes"),
            ));
        }
        if line.capacity() < needed {
            line.reserve_exact(needed - line.len());
        }
        line.extend_from_slice(&buffer[..take]);
        reader.consume(take);
        if newline.is_some() {
            while matches!(line.last(), Some(b'\n' | b'\r')) {
                line.pop();
            }
            return Ok((line, false));
        }
    }
}

fn deliver(inner: &Arc<Inner>, id: i64, result: Result<serde_json::Value, BrokerError>) {
    let mut pending = inner.pending.lock().expect("pending lock");
    if let Some(tx) = pending.remove(&id) {
        let _ = tx.send(result);
    }
}

fn capture_stderr(mut stderr: ChildStderr, retained: &Mutex<Vec<u8>>) {
    let mut chunk = [0_u8; 4096];
    while let Ok(read) = stderr.read(&mut chunk) {
        if read == 0 {
            break;
        }
        let mut bytes = retained.lock().expect("stderr lock");
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > MAX_STDERR_BYTES {
            let excess = bytes.len() - MAX_STDERR_BYTES;
            bytes.drain(..excess);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn bounded_reader_handles_newline_and_crlf() {
        let mut reader = BufReader::new(Cursor::new(b"one\r\ntwo\n"));
        assert_eq!(
            read_limited_line(&mut reader).unwrap(),
            (b"one".to_vec(), false)
        );
        assert_eq!(
            read_limited_line(&mut reader).unwrap(),
            (b"two".to_vec(), false)
        );
        assert_eq!(read_limited_line(&mut reader).unwrap(), (Vec::new(), true));
    }

    #[test]
    fn bounded_reader_delivers_unterminated_line_then_eof() {
        let mut reader = BufReader::new(Cursor::new(b"unterminated"));
        assert_eq!(
            read_limited_line(&mut reader).unwrap(),
            (b"unterminated".to_vec(), true)
        );
    }

    #[test]
    fn bounded_reader_rejects_before_growing_past_cap() {
        let input = vec![b'x'; MAX_LINE_BYTES + 1];
        let mut reader = BufReader::new(Cursor::new(input));
        let error = read_limited_line(&mut reader).expect_err("oversized line");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("line exceeds"));
    }

    #[test]
    fn discover_uses_actual_path_selection() {
        let executable = std::env::current_exe().expect("test executable");
        let fallback = discover("fixture", executable.to_str().expect("executable path"))
            .expect("explicit test executable");
        assert_eq!(std::path::Path::new(&fallback), executable);

        let missing = std::env::temp_dir().join("symbrain-broker-missing-explicit");
        let error = discover("sh", missing.to_str().expect("temp path"))
            .expect_err("invalid explicit path must not fall back to PATH");
        assert!(error.to_string().contains("configured binary_path"));
    }

    #[test]
    fn discover_rejects_directory_and_accepts_unix_executable() {
        let directory = tempfile::tempdir().expect("temp directory");
        let dir_path = directory.path().join("directory");
        std::fs::create_dir(&dir_path).expect("nested directory");
        assert!(discover("sh", dir_path.to_str().expect("directory path")).is_err());

        let fixture = directory.path().join("fixture");
        std::fs::copy(std::env::current_exe().expect("test executable"), &fixture)
            .expect("copy executable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&fixture)
                .expect("fixture metadata")
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&fixture, permissions).expect("set executable mode");
        }
        assert_eq!(
            discover("fixture", fixture.to_str().expect("fixture path"))
                .expect("executable fixture"),
            fixture.to_string_lossy()
        );
    }
}
