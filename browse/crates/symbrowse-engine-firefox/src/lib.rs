#![deny(unsafe_code)]

//! Owned Firefox WebDriver BiDi adapter.  Firefox is launched with an isolated
//! profile and a loopback-only BiDi socket; no geckodriver or Chrome fallback
//! is involved.

use async_tungstenite::{tokio::connect_async, tungstenite::Message};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use symbrowse_engine::{
    EvaluationResult, NavigationResult,
    capabilities::{Capabilities, capabilities_for},
};
use tokio::{
    net::TcpStream,
    process::{Child, Command},
    time::{sleep, timeout},
};

pub const ENGINE_KIND: &str = "firefox";

#[derive(Debug)]
pub enum FirefoxError {
    Unsupported {
        operation: String,
    },
    Timeout {
        operation: String,
        timeout: Duration,
    },
    Driver(String),
    InvalidTarget(String),
    /// The requested Firefox cannot be started on this host (not installed,
    /// bad configured path, exited before BiDi came up). Never substituted by
    /// another engine.
    Unavailable(String),
}
impl std::fmt::Display for FirefoxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported { operation } => write!(f, "unsupported: {operation}"),
            Self::Timeout { operation, timeout } => {
                write!(f, "{operation} timed out after {timeout:?}")
            }
            Self::Driver(s) | Self::InvalidTarget(s) | Self::Unavailable(s) => f.write_str(s),
        }
    }
}
impl std::error::Error for FirefoxError {}

pub fn canonical_capabilities() -> Capabilities {
    capabilities_for(
        ENGINE_KIND,
        [
            "CookieEngine",
            "FrameManager",
            "InspectionEngine",
            "InteractionEngine",
            "NavigationStateProvider",
            "NetworkEvents",
            "ScreenshotEngine",
            "TabManager",
        ],
    )
}

pub fn resolve_firefox_executable(explicit: Option<&Path>) -> Result<PathBuf, FirefoxError> {
    if let Some(path) = explicit {
        if path.is_file() {
            return Ok(path.to_owned());
        }
        return Err(FirefoxError::Unavailable(format!(
            "Firefox executable not found at {}",
            path.display()
        )));
    }
    let candidates = if cfg!(target_os = "macos") {
        vec![PathBuf::from(
            "/Applications/Firefox.app/Contents/MacOS/firefox",
        )]
    } else {
        vec![
            PathBuf::from("/usr/bin/firefox"),
            PathBuf::from("/usr/lib/firefox/firefox"),
        ]
    };
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| FirefoxError::Unavailable("Firefox executable is unavailable".into()))
}

/// Upper bound on buffered BiDi events so a chatty page cannot grow memory.
const MAX_BUFFERED_EVENTS: usize = 4096;

struct Bidi {
    socket: async_tungstenite::WebSocketStream<async_tungstenite::tokio::ConnectStream>,
    next: u64,
    /// Events delivered while a command was waiting for its reply.
    events: Vec<Value>,
}
impl Bidi {
    fn buffer_event(&mut self, value: Value) {
        if value.get("type").and_then(Value::as_str) == Some("event")
            && self.events.len() < MAX_BUFFERED_EVENTS
        {
            self.events.push(value);
        }
    }

    /// Read whatever the browser has already queued without issuing a command.
    async fn pump(&mut self, window: Duration) {
        let _ = timeout(window, async {
            while let Some(Ok(Message::Text(text))) = self.socket.next().await {
                if let Ok(value) = serde_json::from_str::<Value>(&text) {
                    self.buffer_event(value);
                }
            }
        })
        .await;
    }

    async fn command(
        &mut self,
        method: &str,
        params: Value,
        limit: Duration,
    ) -> Result<Value, FirefoxError> {
        let id = self.next;
        self.next += 1;
        self.socket
            .send(Message::Text(
                json!({"id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|e| FirefoxError::Driver(format!("send BiDi command: {e}")))?;
        timeout(limit, async {
            while let Some(message) = self.socket.next().await {
                let message = message
                    .map_err(|e| FirefoxError::Driver(format!("read BiDi response: {e}")))?;
                if let Message::Text(text) = message {
                    let value: Value = serde_json::from_str(&text)
                        .map_err(|e| FirefoxError::Driver(format!("decode BiDi response: {e}")))?;
                    if value.get("id").and_then(Value::as_u64) == Some(id) {
                        if value.get("error").is_some() {
                            return Err(FirefoxError::Driver(value.to_string()));
                        }
                        return Ok(value.get("result").cloned().unwrap_or(Value::Null));
                    }
                    self.buffer_event(value);
                }
            }
            Err(FirefoxError::Driver("BiDi socket closed".into()))
        })
        .await
        .map_err(|_| FirefoxError::Timeout {
            operation: method.into(),
            timeout: limit,
        })?
    }
}

pub struct FirefoxSession {
    bidi: Bidi,
    child: Option<Child>,
    context: String,
    timeout: Duration,
    browser_name: String,
    browser_version: String,
    capturing: bool,
}
impl FirefoxSession {
    pub async fn launch(
        executable: PathBuf,
        profile: PathBuf,
        limit: Duration,
    ) -> Result<Self, FirefoxError> {
        std::fs::create_dir_all(&profile)
            .map_err(|e| FirefoxError::Driver(format!("create isolated profile: {e}")))?;
        let port = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|e| FirefoxError::Driver(e.to_string()))?
            .local_addr()
            .map_err(|e| FirefoxError::Driver(e.to_string()))?
            .port();
        let mut child = Command::new(&executable)
            .args([
                "--headless",
                "--remote-debugging-port",
                &port.to_string(),
                "--profile",
            ])
            .arg(&profile)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| FirefoxError::Unavailable(format!("launch Firefox: {e}")))?;
        let started = Instant::now();
        while started.elapsed() < limit {
            if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
                break;
            }
            if child
                .try_wait()
                .map_err(|e| FirefoxError::Driver(e.to_string()))?
                .is_some()
            {
                return Err(FirefoxError::Unavailable(
                    "Firefox exited before BiDi became ready".into(),
                ));
            }
            sleep(Duration::from_millis(50)).await;
        }
        if started.elapsed() >= limit {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(FirefoxError::Timeout {
                operation: "Firefox BiDi readiness".into(),
                timeout: limit,
            });
        }
        let (socket, _) = connect_async(format!("ws://127.0.0.1:{port}/session"))
            .await
            .map_err(|e| {
                FirefoxError::Driver(format!("connect Firefox BiDi loopback socket: {e}"))
            })?;

        let mut session = Self {
            bidi: Bidi {
                socket,
                next: 1,
                events: Vec::new(),
            },
            child: Some(child),
            context: String::new(),
            timeout: limit,
            browser_name: String::new(),
            browser_version: String::new(),
            capturing: false,
        };
        let result = session.bidi.command("session.new", json!({"capabilities":{"alwaysMatch":{"browserName":"firefox","webSocketUrl":true}}}), limit).await?;
        let negotiated = result.get("capabilities");
        session.browser_name = negotiated
            .and_then(|v| v.get("browserName"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        session.browser_version = negotiated
            .and_then(|v| v.get("browserVersion"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        session.context = negotiated
            .and_then(|v| v.get("webSocketUrl"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        // Own a fresh content tab. The first context of a new profile can be a
        // privileged first-run page, where Firefox rejects script evaluation
        // ("System access is required").
        let created = session
            .bidi
            .command("browsingContext.create", json!({"type":"tab"}), limit)
            .await?;
        session.context = created
            .get("context")
            .and_then(Value::as_str)
            .ok_or_else(|| FirefoxError::Driver("Firefox returned no browsing context".into()))?
            .into();
        Ok(session)
    }
    /// Product name the running Firefox negotiated (`firefox`).
    #[must_use]
    pub fn browser_name(&self) -> &str {
        &self.browser_name
    }

    /// Version the running Firefox reported in `session.new`
    /// (`capabilities.browserVersion`, for example `160.0`).
    #[must_use]
    pub fn browser_version(&self) -> &str {
        &self.browser_version
    }

    pub async fn navigate(&mut self, target: &str) -> Result<NavigationResult, FirefoxError> {
        if !target.starts_with("http://") && !target.starts_with("https://") {
            return Err(FirefoxError::InvalidTarget(target.into()));
        }
        let result = self
            .bidi
            .command(
                "browsingContext.navigate",
                json!({"context":self.context,"url":target,"wait":"complete"}),
                self.timeout,
            )
            .await?;
        Ok(NavigationResult {
            frame_id: self.context.clone(),
            loader_id: result
                .get("navigation")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            url: result
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or(target)
                .into(),
            error_text: String::new(),
        })
    }
    pub async fn evaluate(&mut self, expression: &str) -> Result<EvaluationResult, FirefoxError> {
        let result = self.bidi.command("script.evaluate", json!({"expression":expression,"target":{"context":self.context},"awaitPromise":true,"resultOwnership":"root"}), self.timeout).await?;
        if result.get("type").and_then(Value::as_str) == Some("exception") {
            let text = result
                .get("exceptionDetails")
                .and_then(|details| details.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("script exception");
            return Ok(EvaluationResult {
                value: None,
                value_type: "exception".into(),
                description: String::new(),
                exception_text: text.into(),
            });
        }
        let remote = result.get("result").unwrap_or(&Value::Null);
        Ok(EvaluationResult {
            value: remote.get("value").cloned(),
            value_type: remote
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            description: remote
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            exception_text: String::new(),
        })
    }
    /// Read cookies through the Firefox BiDi storage module.
    pub async fn cookies(&mut self) -> Result<Value, FirefoxError> {
        self.bidi
            .command(
                "storage.getCookies",
                json!({"partition":{"type":"context","context":self.context}}),
                self.timeout,
            )
            .await
    }

    /// Delete cookies with `name` through the Firefox BiDi storage module.
    pub async fn delete_cookies(&mut self, name: &str) -> Result<Value, FirefoxError> {
        self.bidi
            .command(
                "storage.deleteCookies",
                json!({"filter":{"name":name},"partition":{"type":"context","context":self.context}}),
                self.timeout,
            )
            .await
    }

    /// Set a cookie through the Firefox BiDi storage module.
    pub async fn set_cookie(&mut self, cookie: Value) -> Result<Value, FirefoxError> {
        self.bidi
            .command(
                "storage.setCookie",
                json!({"cookie":cookie,"partition":{"type":"context","context":self.context}}),
                self.timeout,
            )
            .await
    }

    /// Execute the two canonical interaction primitives without a browser
    /// specific fallback. DOM events are generated in the selected context.
    pub async fn interact(
        &mut self,
        operation: &str,
        selector: &str,
        value: Option<&str>,
    ) -> Result<Value, FirefoxError> {
        let selector = serde_json::to_string(selector)
            .map_err(|error| FirefoxError::Driver(error.to_string()))?;
        let value = serde_json::to_string(value.unwrap_or_default())
            .map_err(|error| FirefoxError::Driver(error.to_string()))?;
        let expression = match operation {
            "click" => format!(
                "(() => {{ const e=document.querySelector({selector}); if (!e) throw new Error('selector did not match'); e.click(); return {{action:'click'}}; }})()"
            ),
            "type" | "fill" => format!(
                "(() => {{ const e=document.querySelector({selector}); if (!e) throw new Error('selector did not match'); e.focus(); e.value={value}; e.dispatchEvent(new Event('input',{{bubbles:true}})); e.dispatchEvent(new Event('change',{{bubbles:true}})); return {{action:'{operation}',value:e.value}}; }})()"
            ),
            _ => return Err(Self::unsupported(operation)),
        };
        Ok(self
            .evaluate(&expression)
            .await?
            .value
            .unwrap_or(Value::Null))
    }

    /// Start recording completed network responses for this session. BiDi
    /// delivers them as `network.responseCompleted` events, so the capture is
    /// the browser's own view of the traffic, not a proxy.
    pub async fn start_network_capture(&mut self) -> Result<(), FirefoxError> {
        if !self.capturing {
            self.bidi
                .command(
                    "session.subscribe",
                    json!({"events":["network.responseCompleted"]}),
                    self.timeout,
                )
                .await?;
            self.capturing = true;
        }
        self.bidi.events.clear();
        Ok(())
    }

    /// Responses completed since [`Self::start_network_capture`], oldest first.
    pub async fn network_responses(&mut self) -> Result<Vec<Value>, FirefoxError> {
        if !self.capturing {
            return Err(FirefoxError::InvalidTarget(
                "network capture was not started".into(),
            ));
        }
        self.bidi.pump(Duration::from_millis(250)).await;
        Ok(self
            .bidi
            .events
            .iter()
            .filter(|event| {
                event.get("method").and_then(Value::as_str) == Some("network.responseCompleted")
            })
            .filter_map(|event| {
                let params = event.get("params")?;
                let request = params.get("request")?;
                let response = params.get("response")?;
                Some(json!({
                    "url": response.get("url").or_else(|| request.get("url")).cloned().unwrap_or(Value::Null),
                    "method": request.get("method").cloned().unwrap_or(Value::Null),
                    "status": response.get("status").cloned().unwrap_or(Value::Null),
                    "mime_type": response.get("mimeType").cloned().unwrap_or(Value::Null),
                }))
            })
            .collect())
    }

    pub async fn browsing_contexts(&mut self) -> Result<Value, FirefoxError> {
        self.bidi
            .command("browsingContext.getTree", json!({}), self.timeout)
            .await
    }

    pub async fn screenshot(&mut self, format: &str) -> Result<Value, FirefoxError> {
        if !matches!(format, "" | "png" | "jpeg") {
            return Err(FirefoxError::Unsupported {
                operation: format!("screenshot format {format}"),
            });
        }
        self.bidi
            .command(
                "browsingContext.captureScreenshot",
                json!({"context":self.context,"format":{"type":if format.is_empty() { "png" } else { format }},"origin":"viewport"}),
                self.timeout,
            )
            .await
    }

    pub async fn close(&mut self) -> Result<(), FirefoxError> {
        let _ = self
            .bidi
            .command("session.end", json!({}), self.timeout)
            .await;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        Ok(())
    }
    pub fn unsupported(operation: &str) -> FirefoxError {
        FirefoxError::Unsupported {
            operation: operation.into(),
        }
    }
}
impl Drop for FirefoxSession {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capabilities_are_truthful() {
        let c = canonical_capabilities();
        assert!(c.interfaces.contains(&"NavigationStateProvider".into()));
        assert!(c.interfaces.contains(&"ScreenshotEngine".into()));
    }
    #[test]
    fn invalid_explicit_path_is_typed() {
        assert!(matches!(
            resolve_firefox_executable(Some(Path::new("/missing/firefox"))),
            Err(FirefoxError::Unavailable(_))
        ));
    }
}
