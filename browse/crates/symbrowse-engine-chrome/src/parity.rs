//! Raw CDP access and the runtime-event buffers behind the Go daemon's
//! `eval`, `console.*`, `errors.*`, `frame.*`, `set.*`, cookie and storage
//! commands.  Everything here is a thin, honest wrapper over a CDP command or
//! event; nothing is simulated.

use std::{
    borrow::Cow,
    collections::VecDeque,
    error::Error,
    sync::{Arc, Mutex as StdMutex},
    time::{SystemTime, UNIX_EPOCH},
};

use chromiumoxide::{
    Command, Method, Page,
    cdp::{
        browser_protocol::{fetch, network},
        js_protocol::runtime,
    },
    error::CdpError,
    types::MethodId,
};
use futures::StreamExt;
use serde::Serialize;
use serde_json::{Value, json};

use crate::ChromePage;

/// Maximum buffered console/error entries per page (Go: `maxConsoleEntries`).
pub const MAX_RUNTIME_ENTRIES: usize = 500;
const MAX_ENTRY_CHARS: usize = 4096;

#[derive(Serialize)]
struct RawCommand {
    #[serde(skip)]
    method: String,
    #[serde(flatten)]
    params: serde_json::Map<String, Value>,
}

impl Method for RawCommand {
    fn identifier(&self) -> MethodId {
        Cow::Owned(self.method.clone())
    }
}

impl Command for RawCommand {
    type Response = Value;
}

/// One captured console API call.
#[derive(Clone, Debug)]
pub struct ConsoleEntry {
    pub kind: String,
    pub text: String,
    pub url: String,
    pub line: i64,
    pub unix_nanos: i128,
}

/// One captured uncaught exception.
#[derive(Clone, Debug)]
pub struct ErrorEntry {
    pub text: String,
    pub url: String,
    pub line: i64,
    pub stacktrace: Vec<String>,
    pub unix_nanos: i128,
}

/// One captured HTTP request (Go `engine.NetworkRequest`); sensitive header
/// values are masked before they are stored.
#[derive(Clone, Debug, Default)]
pub struct NetworkRequestEntry {
    pub id: String,
    pub url: String,
    pub method: String,
    pub kind: String,
    pub status: i64,
    pub status_text: String,
    pub mime_type: String,
    pub started_unix_secs: i64,
    pub finished: bool,
    pub failed: String,
    pub request_headers: std::collections::BTreeMap<String, String>,
    pub response_headers: std::collections::BTreeMap<String, String>,
    pub encoded_body_size: i64,
}

/// A mock route installed by `network.route`.
#[derive(Clone, Debug, Default)]
pub struct NetworkRoute {
    pub pattern: String,
    pub action: String,
    pub status: i64,
    pub body: Option<Value>,
    pub content_type: String,
}

const MAX_NETWORK_REQUESTS: usize = 2000;

#[derive(Default)]
struct Buffers {
    console: VecDeque<ConsoleEntry>,
    errors: VecDeque<ErrorEntry>,
    requests: Vec<NetworkRequestEntry>,
    routes: std::collections::BTreeMap<String, NetworkRoute>,
}

fn mask_headers(headers: &Value) -> std::collections::BTreeMap<String, String> {
    let mut masked = std::collections::BTreeMap::new();
    if let Some(map) = headers.as_object() {
        for (name, value) in map {
            let lower = name.to_ascii_lowercase();
            let text = match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            let secret = matches!(
                lower.as_str(),
                "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
            );
            masked.insert(
                name.clone(),
                if secret { "[redacted]".into() } else { text },
            );
        }
    }
    masked
}

fn route_matches(pattern: &str, url: &str) -> bool {
    pattern
        .strip_suffix('*')
        .map_or_else(|| url == pattern, |prefix| url.starts_with(prefix))
}

/// Shared event buffers plus the listener tasks that fill them.
#[derive(Clone)]
pub(crate) struct RuntimeMonitor {
    buffers: Arc<StdMutex<Buffers>>,
    active_frame_context: Arc<StdMutex<i64>>,
    _tasks: Arc<Vec<tokio::task::JoinHandle<()>>>,
}

fn now_nanos() -> i128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i128)
}

fn truncate(text: String) -> String {
    if text.len() > MAX_ENTRY_CHARS {
        let mut end = MAX_ENTRY_CHARS;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &text[..end])
    } else {
        text
    }
}

fn render_console_args(args: &[runtime::RemoteObject]) -> String {
    let mut parts = Vec::new();
    for arg in args {
        let kind = serde_json::to_value(&arg.r#type)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        if kind == "string" {
            match &arg.value {
                Some(Value::String(text)) => parts.push(text.clone()),
                Some(other) => parts.push(other.to_string()),
                None => {}
            }
        } else if let Some(description) = arg.description.as_ref().filter(|d| !d.is_empty()) {
            parts.push(description.clone());
        } else if let Some(value) = arg.value.as_ref().filter(|v| !v.is_null()) {
            parts.push(value.to_string());
        }
    }
    parts.join(" ")
}

fn push_bounded<T>(queue: &mut VecDeque<T>, entry: T) {
    if queue.len() >= MAX_RUNTIME_ENTRIES {
        queue.pop_front();
    }
    queue.push_back(entry);
}

impl RuntimeMonitor {
    pub(crate) async fn start(page: &Page) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let buffers = Arc::new(StdMutex::new(Buffers::default()));
        let mut console = page
            .event_listener::<runtime::EventConsoleApiCalled>()
            .await?;
        let mut exceptions = page
            .event_listener::<runtime::EventExceptionThrown>()
            .await?;
        let mut will_be_sent = page
            .event_listener::<network::EventRequestWillBeSent>()
            .await?;
        let mut response_received = page
            .event_listener::<network::EventResponseReceived>()
            .await?;
        let mut finished = page
            .event_listener::<network::EventLoadingFinished>()
            .await?;
        let mut failed = page.event_listener::<network::EventLoadingFailed>().await?;
        let mut paused = page.event_listener::<fetch::EventRequestPaused>().await?;
        // Runtime.enable is idempotent; the listeners above only receive
        // events once the domain is enabled for this target.
        page.execute(runtime::EnableParams::default()).await?;
        page.execute(network::EnableParams::default()).await?;
        let console_buffers = Arc::clone(&buffers);
        let console_task = tokio::spawn(async move {
            while let Some(event) = console.next().await {
                let text = render_console_args(&event.args);
                if text.is_empty() {
                    continue;
                }
                let kind = serde_json::to_value(&event.r#type)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                // Go decodes the top-level `url`/`lineNumber` of the event,
                // which Chrome never populates for console API calls: the
                // recorded entry therefore has no url and line 0 + 1.
                let entry = ConsoleEntry {
                    kind,
                    text: truncate(text),
                    url: String::new(),
                    line: 1,
                    unix_nanos: now_nanos(),
                };
                if let Ok(mut buffers) = console_buffers.lock() {
                    push_bounded(&mut buffers.console, entry);
                }
            }
        });
        let error_buffers = Arc::clone(&buffers);
        let error_task = tokio::spawn(async move {
            while let Some(event) = exceptions.next().await {
                let details = &event.exception_details;
                let stacktrace = details.stack_trace.as_ref().map_or_else(Vec::new, |stack| {
                    stack
                        .call_frames
                        .iter()
                        .map(|frame| {
                            let function = if frame.function_name.is_empty() {
                                "(anonymous)"
                            } else {
                                frame.function_name.as_str()
                            };
                            format!(
                                "{function} ({}:{}:{})",
                                frame.url,
                                frame.line_number + 1,
                                frame.column_number + 1
                            )
                        })
                        .collect()
                });
                let entry = ErrorEntry {
                    text: truncate(details.text.clone()),
                    url: details.url.clone().unwrap_or_default(),
                    line: details.line_number + 1,
                    stacktrace,
                    unix_nanos: now_nanos(),
                };
                if let Ok(mut buffers) = error_buffers.lock() {
                    push_bounded(&mut buffers.errors, entry);
                }
            }
        });
        let net_buffers = Arc::clone(&buffers);
        let network_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(event) = will_be_sent.next() => {
                        let Ok(event) = serde_json::to_value(&*event) else { continue };
                        let entry = NetworkRequestEntry {
                            id: event["requestId"].as_str().unwrap_or_default().to_owned(),
                            url: event["request"]["url"].as_str().unwrap_or_default().to_owned(),
                            method: event["request"]["method"].as_str().unwrap_or_default().to_owned(),
                            kind: event["type"].as_str().unwrap_or_default().to_owned(),
                            started_unix_secs: event["wallTime"].as_f64().unwrap_or(0.0) as i64,
                            request_headers: mask_headers(&event["request"]["headers"]),
                            ..NetworkRequestEntry::default()
                        };
                        if let Ok(mut buffers) = net_buffers.lock() {
                            if buffers.requests.len() >= MAX_NETWORK_REQUESTS {
                                buffers.requests.remove(0);
                            }
                            buffers.requests.push(entry);
                        }
                    }
                    Some(event) = response_received.next() => {
                        let Ok(event) = serde_json::to_value(&*event) else { continue };
                        let id = event["requestId"].as_str().unwrap_or_default();
                        if let Ok(mut buffers) = net_buffers.lock()
                            && let Some(entry) = buffers.requests.iter_mut().rev().find(|e| e.id == id)
                        {
                            let response = &event["response"];
                            entry.status = response["status"].as_i64().unwrap_or(0);
                            entry.status_text = response["statusText"].as_str().unwrap_or_default().to_owned();
                            entry.mime_type = response["mimeType"].as_str().unwrap_or_default().to_owned();
                            entry.response_headers = mask_headers(&response["headers"]);
                            entry.encoded_body_size = response["encodedDataLength"].as_f64().unwrap_or(0.0) as i64;
                        }
                    }
                    Some(event) = finished.next() => {
                        let Ok(event) = serde_json::to_value(&*event) else { continue };
                        let id = event["requestId"].as_str().unwrap_or_default();
                        if let Ok(mut buffers) = net_buffers.lock()
                            && let Some(entry) = buffers.requests.iter_mut().rev().find(|e| e.id == id)
                        {
                            entry.finished = true;
                        }
                    }
                    Some(event) = failed.next() => {
                        let Ok(event) = serde_json::to_value(&*event) else { continue };
                        let id = event["requestId"].as_str().unwrap_or_default();
                        if let Ok(mut buffers) = net_buffers.lock()
                            && let Some(entry) = buffers.requests.iter_mut().rev().find(|e| e.id == id)
                        {
                            entry.failed = event["errorText"].as_str().unwrap_or_default().to_owned();
                            entry.finished = true;
                        }
                    }
                    else => break,
                }
            }
        });
        let route_buffers = Arc::clone(&buffers);
        let route_page = page.clone();
        let route_task = tokio::spawn(async move {
            while let Some(event) = paused.next().await {
                let Ok(event) = serde_json::to_value(&*event) else {
                    continue;
                };
                let request_id = event["requestId"].as_str().unwrap_or_default().to_owned();
                let url = event["request"]["url"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let route = route_buffers.lock().ok().and_then(|buffers| {
                    buffers
                        .routes
                        .values()
                        .find(|route| route_matches(&route.pattern, &url))
                        .cloned()
                });
                let (method, params) = match route {
                    None => ("Fetch.continueRequest", json!({"requestId": request_id})),
                    Some(route) if route.action == "abort" => (
                        "Fetch.failRequest",
                        json!({"requestId": request_id, "errorReason": "BlockedByClient"}),
                    ),
                    Some(route) => {
                        let mut params = json!({
                            "requestId": request_id,
                            "responseCode": if route.status == 0 { 200 } else { route.status },
                        });
                        if !route.content_type.is_empty() {
                            params["responseHeaders"] =
                                json!([{"name": "Content-Type", "value": route.content_type}]);
                        }
                        if let Some(body) = &route.body {
                            params["body"] = json!(base64_encode(body.to_string().as_bytes()));
                        }
                        ("Fetch.fulfillRequest", params)
                    }
                };
                if let Value::Object(map) = params {
                    let _ = route_page
                        .execute(RawCommand {
                            method: method.to_owned(),
                            params: map,
                        })
                        .await;
                }
            }
        });
        Ok(Self {
            buffers,
            active_frame_context: Arc::new(StdMutex::new(0)),
            _tasks: Arc::new(vec![console_task, error_task, network_task, route_task]),
        })
    }
}

fn cdp_error(method: &str, error: CdpError) -> Box<dyn Error + Send + Sync> {
    match error {
        CdpError::Chrome(inner) => {
            format!("CDP {method} failed ({}): {}", inner.code, inner.message).into()
        }
        other => Box::new(other),
    }
}

impl ChromePage {
    /// Execute one raw CDP command on this page's session.
    pub async fn cdp(
        &self,
        method: &str,
        params: Value,
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let params = match params {
            Value::Object(map) => map,
            Value::Null => serde_json::Map::new(),
            other => {
                return Err(format!("CDP params for {method} must be an object: {other}").into());
            }
        };
        let response = self
            .raw()
            .execute(RawCommand {
                method: method.to_owned(),
                params,
            })
            .await
            .map_err(|error| cdp_error(method, error))?;
        Ok(response.result)
    }

    /// `Runtime.evaluate` with Go's flags (`returnByValue`, `awaitPromise`) in
    /// the active frame's execution context.  Returns the protocol-neutral
    /// `{type, value, description, exception_text}` object.
    pub async fn evaluate_expression(
        &self,
        expression: &str,
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let context = self.active_frame_context();
        let mut params = json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": true,
        });
        if context != 0 {
            params["contextId"] = json!(context);
        }
        let result = self.cdp("Runtime.evaluate", params).await?;
        let object = result.get("result").cloned().unwrap_or(Value::Null);
        Ok(json!({
            "type": object.get("type").cloned().unwrap_or(json!("")),
            "value": object.get("value").cloned().unwrap_or(Value::Null),
            "description": object.get("description").cloned().unwrap_or(json!("")),
            "exception_text": result
                .pointer("/exceptionDetails/text")
                .cloned()
                .unwrap_or(json!("")),
        }))
    }

    pub fn active_frame_context(&self) -> i64 {
        self.events
            .active_frame_context
            .lock()
            .map_or(0, |context| *context)
    }

    pub fn set_active_frame_context(&self, context: i64) {
        if let Ok(mut guard) = self.events.active_frame_context.lock() {
            *guard = context;
        }
    }

    pub fn console_entries(&self) -> Vec<ConsoleEntry> {
        self.events
            .buffers
            .lock()
            .map_or_else(|_| Vec::new(), |b| b.console.iter().cloned().collect())
    }

    pub fn error_entries(&self) -> Vec<ErrorEntry> {
        self.events
            .buffers
            .lock()
            .map_or_else(|_| Vec::new(), |b| b.errors.iter().cloned().collect())
    }

    pub fn clear_console(&self) {
        if let Ok(mut buffers) = self.events.buffers.lock() {
            buffers.console.clear();
        }
    }

    pub fn clear_errors(&self) {
        if let Ok(mut buffers) = self.events.buffers.lock() {
            buffers.errors.clear();
        }
    }
}

impl ChromePage {
    pub fn network_requests(&self) -> Vec<NetworkRequestEntry> {
        self.events
            .buffers
            .lock()
            .map_or_else(|_| Vec::new(), |b| b.requests.clone())
    }

    pub fn network_request(&self, id: &str) -> Option<NetworkRequestEntry> {
        self.events
            .buffers
            .lock()
            .ok()
            .and_then(|b| b.requests.iter().rev().find(|r| r.id == id).cloned())
    }

    /// Install a mock route; Fetch interception turns on with the first one.
    pub async fn route_requests(
        &self,
        route: NetworkRoute,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if route.pattern.trim().is_empty() {
            return Err("route pattern is required".into());
        }
        if route.action != "abort" && route.action != "mock" {
            return Err(format!("route action must be {:?} or {:?}", "abort", "mock").into());
        }
        let first = {
            let mut buffers = self
                .events
                .buffers
                .lock()
                .map_err(|_| "network lock poisoned")?;
            let first = buffers.routes.is_empty();
            buffers.routes.insert(route.pattern.clone(), route);
            first
        };
        if first {
            self.cdp(
                "Fetch.enable",
                json!({"patterns": [{"urlPattern": "*", "requestStage": "Request"}]}),
            )
            .await?;
        }
        Ok(())
    }

    /// Remove one route (all when `pattern` is empty); disables Fetch once the
    /// last route is gone.
    pub async fn unroute_requests(
        &self,
        pattern: &str,
    ) -> Result<bool, Box<dyn Error + Send + Sync>> {
        let (removed, remaining) = {
            let mut buffers = self
                .events
                .buffers
                .lock()
                .map_err(|_| "network lock poisoned")?;
            let removed = if pattern.is_empty() {
                let had = !buffers.routes.is_empty();
                buffers.routes.clear();
                had
            } else {
                buffers.routes.remove(pattern).is_some()
            };
            (removed, buffers.routes.len())
        };
        if remaining == 0 {
            self.cdp("Fetch.disable", json!({}))
                .await
                .map_err(|error| format!("disable fetch interception: {error}"))?;
        }
        Ok(removed)
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}
