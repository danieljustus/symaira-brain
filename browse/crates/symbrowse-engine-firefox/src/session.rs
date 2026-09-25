use super::{context_partition, evaluation_result};
use crate::FirefoxError;
use crate::startup::{capture_startup_output, captured_output, startup_detail};
use async_tungstenite::{tokio::connect_async, tungstenite::Message};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use symbrowse_engine::{EvaluationResult, NavigationResult};
use tokio::{
    net::TcpStream,
    process::{Child, Command},
    time::{sleep, timeout},
};

fn key_action(kind: &str, value: &str) -> Value {
    json!({"type":kind,"value":value})
}

pub(super) struct Bidi {
    socket: async_tungstenite::WebSocketStream<async_tungstenite::tokio::ConnectStream>,
    next: u64,
    pub(super) events: Vec<Value>,
    pub(super) events_overflowed: bool,
}
impl Bidi {
    pub(super) async fn command(
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
                            if value.get("error").and_then(Value::as_str)
                                == Some("unsupported operation")
                            {
                                return Err(FirefoxError::Unsupported {
                                    operation: method.into(),
                                });
                            }
                            return Err(FirefoxError::Driver(value.to_string()));
                        }
                        return Ok(value.get("result").cloned().unwrap_or(Value::Null));
                    }
                    if value.get("id").is_none()
                        && value.get("method").and_then(Value::as_str)
                            == Some("network.responseCompleted")
                    {
                        if self.events.len() == 256 {
                            self.events_overflowed = true;
                        } else {
                            self.events.push(value);
                        }
                    }
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
    pub(super) bidi: Bidi,
    child: Option<Child>,
    pub(super) context: String,
    pub(super) timeout: Duration,
    pub(super) endpoint: SocketAddr,
}
impl FirefoxSession {
    pub async fn launch(
        executable: PathBuf,
        profile: PathBuf,
        limit: Duration,
    ) -> Result<Self, FirefoxError> {
        std::fs::create_dir_all(&profile)
            .map_err(|e| FirefoxError::Driver(format!("create isolated profile: {e}")))?;
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|e| FirefoxError::Driver(e.to_string()))?;
        let endpoint = listener
            .local_addr()
            .map_err(|e| FirefoxError::Driver(e.to_string()))?;
        drop(listener);
        let mut child = Command::new(&executable)
            .args([
                "--headless",
                "--no-remote",
                "--new-instance",
                "--remote-debugging-port",
                &endpoint.port().to_string(),
                "--profile",
            ])
            .arg(&profile)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| FirefoxError::Driver(format!("launch Firefox: {e}")))?;
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let stdout_reader = child
            .stdout
            .take()
            .map(|reader| tokio::spawn(capture_startup_output(reader, Arc::clone(&stdout))));
        let stderr_reader = child
            .stderr
            .take()
            .map(|reader| tokio::spawn(capture_startup_output(reader, Arc::clone(&stderr))));
        let started = Instant::now();
        while started.elapsed() < limit {
            if TcpStream::connect(endpoint).await.is_ok() {
                break;
            }
            if let Some(status) = child
                .try_wait()
                .map_err(|e| FirefoxError::Driver(e.to_string()))?
            {
                if let Some(reader) = stdout_reader {
                    let _ = reader.await;
                }
                if let Some(reader) = stderr_reader {
                    let _ = reader.await;
                }
                let detail = startup_detail(&captured_output(&stdout), &captured_output(&stderr));
                return Err(FirefoxError::Driver(
                    format!("Firefox exited before BiDi became ready ({status})")
                        + &detail
                            .map(|detail| format!("; {detail}"))
                            .unwrap_or_default(),
                ));
            }
            sleep(Duration::from_millis(50)).await;
        }
        if started.elapsed() >= limit {
            let _ = terminate_process(&mut child).await;
            if let Some(reader) = stdout_reader {
                let _ = reader.await;
            }
            if let Some(reader) = stderr_reader {
                let _ = reader.await;
            }
            return Err(FirefoxError::Timeout {
                operation: startup_detail(&captured_output(&stdout), &captured_output(&stderr))
                    .map(|detail| format!("Firefox BiDi readiness: {detail}"))
                    .unwrap_or_else(|| "Firefox BiDi readiness".into()),
                timeout: limit,
            });
        }
        let (socket, _) = match connect_async(format!("ws://{endpoint}/session")).await {
            Ok(connection) => connection,
            Err(error) => {
                let _ = terminate_process(&mut child).await;
                return Err(FirefoxError::Driver(format!(
                    "connect Firefox BiDi loopback socket: {error}"
                )));
            }
        };

        let mut session = Self {
            bidi: Bidi {
                socket,
                next: 1,
                events: Vec::new(),
                events_overflowed: false,
            },
            child: Some(child),
            context: String::new(),
            timeout: limit,
            endpoint,
        };
        if let Err(error) = session
            .bidi
            .command("session.new", json!({"capabilities":{"alwaysMatch":{"browserName":"firefox","webSocketUrl":true}}}), limit)
            .await
        {
            let _ = session.stop_owned_process().await;
            return Err(error);
        }
        let tree = match session
            .bidi
            .command("browsingContext.getTree", json!({}), limit)
            .await
        {
            Ok(tree) => tree,
            Err(error) => {
                let _ = session.stop_owned_process().await;
                return Err(error);
            }
        };
        let Some(context) = tree
            .get("contexts")
            .and_then(Value::as_array)
            .and_then(|v| v.first())
            .and_then(|v| v.get("context"))
            .and_then(Value::as_str)
        else {
            let _ = session.stop_owned_process().await;
            return Err(FirefoxError::Driver(
                "Firefox returned no browsing context".into(),
            ));
        };
        session.context = context.to_owned();
        Ok(session)
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
        Ok(evaluation_result(result))
    }
    /// Read cookies through the Firefox BiDi storage module.
    pub async fn cookies(&mut self) -> Result<Value, FirefoxError> {
        self.bidi
            .command(
                "storage.getCookies",
                json!({"partition":context_partition(&self.context)}),
                self.timeout,
            )
            .await
    }

    /// Set a cookie through the Firefox BiDi storage module.
    pub async fn set_cookie(&mut self, cookie: Value) -> Result<Value, FirefoxError> {
        self.bidi
            .command(
                "storage.setCookie",
                json!({"cookie":cookie,"partition":context_partition(&self.context)}),
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
        match operation {
            "click" => {
                let center = self.interaction_target_center(&selector).await?;
                self.perform_actions(
                    "pointer",
                    "symbrowse-mouse",
                    vec![
                        json!({"type":"pointerMove","x":center[0],"y":center[1],"origin":"viewport"}),
                        json!({"type":"pointerDown","button":0}),
                        json!({"type":"pointerUp","button":0}),
                    ],
                )
                .await?;
                Ok(json!({"action":"click"}))
            }
            "type" | "fill" => {
                let expression = format!(
                    "(() => {{ const e=document.querySelector({selector}); if (!e) throw new Error('selector did not match'); if (!(e instanceof HTMLInputElement || e instanceof HTMLTextAreaElement || e.isContentEditable)) throw new Error('selector is not editable'); e.focus(); if (document.activeElement !== e) throw new Error('selector could not be focused'); return {{action:'{operation}'}}; }})()"
                );
                self.interaction_value(&expression).await?;
                let mut actions = Vec::new();
                if operation == "fill" {
                    let modifier = if cfg!(target_os = "macos") {
                        "\u{e03d}"
                    } else {
                        "\u{e009}"
                    };
                    actions.extend([
                        key_action("keyDown", modifier),
                        key_action("keyDown", "a"),
                        key_action("keyUp", "a"),
                        key_action("keyUp", modifier),
                        key_action("keyDown", "\u{e003}"),
                        key_action("keyUp", "\u{e003}"),
                    ]);
                }
                for character in value.unwrap_or_default().chars() {
                    let key = match character {
                        '\n' => "\u{e007}".to_owned(),
                        '\t' => "\u{e004}".to_owned(),
                        '\u{8}' => "\u{e003}".to_owned(),
                        character => character.to_string(),
                    };
                    actions.push(key_action("keyDown", &key));
                    actions.push(key_action("keyUp", &key));
                }
                self.perform_actions("key", "symbrowse-keyboard", actions)
                    .await?;
                Ok(json!({"action":operation}))
            }
            _ => Err(Self::unsupported(operation)),
        }
    }

    async fn interaction_target_center(
        &mut self,
        selector: &str,
    ) -> Result<[f64; 2], FirefoxError> {
        let expression = format!(
            "(() => {{ const e=document.querySelector({selector}); if (!e) throw new Error('selector did not match'); e.scrollIntoView({{block:'center',inline:'center'}}); const r=e.getBoundingClientRect(); if (r.width <= 0 || r.height <= 0) throw new Error('selector target is not visible'); const x=r.left+r.width/2, y=r.top+r.height/2, hit=document.elementFromPoint(x,y); if (!hit || (hit !== e && !e.contains(hit))) throw new Error('selector target is obstructed'); return {{x,y}}; }})()"
        );
        let value = self.interaction_value(&expression).await?;
        let x = value.get("x").and_then(Value::as_f64).ok_or_else(|| {
            FirefoxError::Driver("Firefox returned no interaction x coordinate".into())
        })?;
        let y = value.get("y").and_then(Value::as_f64).ok_or_else(|| {
            FirefoxError::Driver("Firefox returned no interaction y coordinate".into())
        })?;
        Ok([x, y])
    }

    async fn interaction_value(&mut self, expression: &str) -> Result<Value, FirefoxError> {
        let result = self.evaluate(expression).await?;
        if !result.exception_text.is_empty() {
            return Err(FirefoxError::Driver(result.exception_text));
        }
        result
            .value
            .ok_or_else(|| FirefoxError::Driver("Firefox interaction returned no value".into()))
    }

    async fn perform_actions(
        &mut self,
        source_type: &str,
        source_id: &str,
        actions: Vec<Value>,
    ) -> Result<(), FirefoxError> {
        let result = self
            .bidi
            .command(
                "input.performActions",
                json!({"context":self.context,"actions":[{"type":source_type,"id":source_id,"actions":actions}]}),
                self.timeout,
            )
            .await;
        let release = self
            .bidi
            .command(
                "input.releaseActions",
                json!({"context":self.context}),
                self.timeout,
            )
            .await;
        result?;
        release?;
        Ok(())
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
        self.stop_owned_process().await?;
        Ok(())
    }

    async fn stop_owned_process(&mut self) -> Result<(), FirefoxError> {
        if let Some(mut child) = self.child.take() {
            terminate_process(&mut child).await?;
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
            #[cfg(windows)]
            // Firefox Nightly may leave child processes owning the BiDi port
            // after its parent exits; this session uses an isolated profile.
            if !child.id().is_some_and(terminate_windows_process_tree) {
                let _ = child.start_kill();
            }
            #[cfg(not(windows))]
            let _ = child.start_kill();
        }
    }
}

async fn terminate_process(child: &mut Child) -> Result<(), FirefoxError> {
    let running = child
        .try_wait()
        .map_err(|error| FirefoxError::Driver(format!("check Firefox process: {error}")))?
        .is_none();
    if running {
        #[cfg(windows)]
        let tree_stopped = child.id().is_some_and(terminate_windows_process_tree);
        #[cfg(not(windows))]
        let tree_stopped = false;

        if !tree_stopped {
            if let Err(error) = child.start_kill()
                && child
                    .try_wait()
                    .map_err(|check| {
                        FirefoxError::Driver(format!("check Firefox process: {check}"))
                    })?
                    .is_none()
            {
                return Err(FirefoxError::Driver(format!(
                    "stop Firefox process: {error}"
                )));
            }
        }
    }
    child
        .wait()
        .await
        .map_err(|error| FirefoxError::Driver(format!("wait for Firefox process: {error}")))?;
    Ok(())
}

#[cfg(windows)]
fn terminate_windows_process_tree(pid: u32) -> bool {
    // /T matters here: killing only the Firefox parent can leave the remote
    // agent socket open in one of its child processes.
    std::process::Command::new("taskkill.exe")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
