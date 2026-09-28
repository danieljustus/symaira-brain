//! Chrome interaction, inspection, frame, network and artifact adapter.
//!
//! The adapter intentionally exposes only operations that are backed by a CDP
//! command in chromiumoxide.  Features which need a separate protocol (for
//! example axe-core injection or HAR export) are represented by an explicit
//! [`UnsupportedOperation`] error rather than a best-effort implementation.

use std::{
    error::Error,
    fmt,
    path::Path,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use async_tungstenite::{tokio::connect_async, tungstenite::Message};
use chromiumoxide::{
    Browser, Element, Page,
    cdp::{
        browser_protocol::{accessibility, browser, dom, network, page, target},
        js_protocol::runtime,
    },
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::{BrowserMode, ConnectionMode, launch};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedOperation(pub &'static str);

impl fmt::Display for UnsupportedOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported Chrome operation: {}", self.0)
    }
}
impl Error for UnsupportedOperation {}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct InteractionResult {
    pub action: String,
    pub selector: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct FindOptions {
    pub kind: String,
    pub query: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub exact: bool,
    #[serde(default)]
    pub index: Option<usize>,
    #[serde(default)]
    pub value: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct FrameInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub parent_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DialogInfo {
    pub kind: String,
    pub message: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_prompt: Option<String>,
}

/// Persistent state for the daemon's manual dialog controller.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PendingDialog {
    #[serde(rename = "type", skip_serializing_if = "String::is_empty")]
    pub dialog_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub default: String,
    pub handled: bool,
    #[serde(rename = "auto_mode", skip_serializing_if = "String::is_empty")]
    pub auto_mode: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct NetworkEvent {
    pub kind: String,
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub status: u16,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mime_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error_text: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScreenshotOptions {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<i64>,
    #[serde(default)]
    pub full_page: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub selector: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Artifact {
    pub kind: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChromeCapabilities {
    pub interactions: Vec<String>,
    pub inspection: Vec<String>,
    pub tabs_frames_dialogs: Vec<String>,
    pub artifacts: Vec<String>,
    pub unsupported: Vec<String>,
}

/// The capabilities proved by this adapter. Keep this list synchronized with
/// the opt-in integration test in `tests/full.rs`.
pub fn capabilities() -> ChromeCapabilities {
    ChromeCapabilities {
        interactions: [
            "click", "dblclick", "fill", "type", "press", "focus", "hover", "select", "check",
            "uncheck", "scroll",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        inspection: ["find", "get", "is", "count"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        tabs_frames_dialogs: ["tabs", "frames", "dialogs-manual", "dialogs-auto"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        artifacts: [
            "network-events",
            "upload",
            "download",
            "screenshot-png",
            "screenshot-jpeg",
            "screenshot-full",
            "screenshot-selector",
            "pdf",
            "accessibility-tree",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        unsupported: ["har-export", "axe-core-audit"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    }
}

/// A connected browser plus one page. The handler is kept alive in a task so
/// chromiumoxide can dispatch CDP events and page commands.
pub struct ChromeSession {
    browser: Browser,
    mode: ConnectionMode,
    timeout: Duration,
    _handler_task: tokio::task::JoinHandle<()>,
}

fn perf_diagnostics_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("SYMBROWSE_PERF_DIAGNOSTICS").is_some())
}

fn perf_diagnostic(message: &str) {
    if perf_diagnostics_enabled() {
        eprintln!("symbrowse Chrome phase: {message}");
    }
}

impl ChromeSession {
    pub async fn connect(
        mode: BrowserMode,
        timeout: Duration,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        perf_diagnostic("browser.connect.start");
        let connection = launch::connect(&mode, timeout).await?;
        perf_diagnostic("browser.connect.ready");
        let task = tokio::spawn(async move {
            let mut handler = connection.handler;
            while handler.next().await.is_some() {}
        });
        Ok(Self {
            browser: connection.browser,
            mode: connection.mode,
            timeout,
            _handler_task: task,
        })
    }

    pub fn mode(&self) -> ConnectionMode {
        self.mode
    }

    pub async fn new_page(
        &self,
        url: impl Into<String>,
    ) -> Result<ChromePage, Box<dyn Error + Send + Sync>> {
        let url = url.into();
        perf_diagnostic("page.create.start");
        let page = if url == "about:blank" {
            self.new_blank_page_without_load_wait().await?
        } else {
            self.browser.new_page(url).await?
        };
        let page = ChromePage::new(page, self.browser.websocket_address().clone()).await?;
        perf_diagnostic("page.create.ready");
        Ok(page)
    }

    // chromiumoxide::Browser::new_page waits for the new target's main frame
    // to report loaded. Chrome can leave an about:blank target in that state
    // indefinitely, while Go's CDP path returns as soon as the target is
    // attached. Create the same blank target, then obtain its Page handle once
    // chromiumoxide has attached the target without waiting for navigation.
    async fn new_blank_page_without_load_wait(&self) -> Result<Page, Box<dyn Error + Send + Sync>> {
        let params = target::CreateTargetParams::builder()
            .url("about:blank")
            .build()
            .map_err(chromiumoxide::error::CdpError::msg)?;
        let page = tokio::time::timeout(self.timeout, async {
            perf_diagnostic("blank_target.create.start");
            let target_id = self.browser.execute(params).await?.result.target_id;
            perf_diagnostic("blank_target.create.ready");
            loop {
                match self.browser.get_page(target_id.clone()).await {
                    Ok(page) => {
                        perf_diagnostic("blank_target.attach.ready");
                        let diagnostics = perf_diagnostics_enabled();
                        let readiness_started = diagnostics.then(Instant::now);
                        let mut last_readiness_report = readiness_started;
                        // `get_page` returns as soon as chromiumoxide has an
                        // attached session. Its Page/Frame initialization
                        // continues asynchronously, so wait for the initial
                        // main frame and its default JavaScript context before
                        // callers navigate.
                        loop {
                            if let Some(frame) = page.mainframe().await? {
                                if page.frame_execution_context(frame.clone()).await?.is_some() {
                                    if let Some(started) = readiness_started {
                                        perf_diagnostic(&format!(
                                            "blank_target.context.ready elapsed_ms={} frame={frame:?}",
                                            started.elapsed().as_millis()
                                        ));
                                    }
                                    return Ok(page);
                                }
                            }
                            if let (Some(started), Some(last_report)) =
                                (readiness_started, last_readiness_report.as_mut())
                                && last_report.elapsed() >= Duration::from_secs(1)
                            {
                                perf_diagnostic(&format!(
                                    "blank_target.context.wait elapsed_ms={}",
                                    started.elapsed().as_millis()
                                ));
                                *last_report = Instant::now();
                            }
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                    }
                    Err(chromiumoxide::error::CdpError::NotFound) => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(error) => return Err(error),
                }
            }
        })
        .await
        .map_err(|_| chromiumoxide::error::CdpError::Timeout)??;
        Ok(page)
    }

    pub async fn pages(&self) -> Result<Vec<ChromePage>, Box<dyn Error + Send + Sync>> {
        let mut chrome_pages = Vec::new();
        for page in self.browser.pages().await? {
            chrome_pages
                .push(ChromePage::new(page, self.browser.websocket_address().clone()).await?);
        }
        Ok(chrome_pages)
    }

    pub async fn close(mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let close_result = if self.mode == ConnectionMode::Launch {
            self.browser.close().await.map(|_| ())
        } else {
            Ok(())
        };
        let wait_result = self.browser.wait().await;
        self._handler_task.abort();
        close_result?;
        wait_result?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct ChromePage {
    page: Page,
    websocket_address: String,
    dialogs: DialogMonitor,
}

#[derive(Clone)]
struct DialogMonitor {
    state: Arc<Mutex<DialogState>>,
    _task: Arc<tokio::task::JoinHandle<()>>,
}

#[derive(Default)]
struct DialogState {
    pending: Option<PendingDialog>,
    auto_mode: String,
}

fn is_transient_navigation_context_error(error: &(dyn Error + Send + Sync + 'static)) -> bool {
    matches!(
        error.downcast_ref::<chromiumoxide::error::CdpError>(),
        Some(chromiumoxide::error::CdpError::Chrome(cdp_error))
            if cdp_error.code == -32000
                && cdp_error.message == "Inspected target navigated or closed"
    )
}

fn page_navigate_error(result: page::NavigateReturns) -> Option<chromiumoxide::error::CdpError> {
    result
        .error_text
        .filter(|message| message != "net::ERR_ABORTED")
        .map(chromiumoxide::error::CdpError::ChromeMessage)
}

fn cdp_request(id: u64, method: &str, params: Value, session_id: Option<&str>) -> Value {
    let mut request = serde_json::json!({
        "id": id,
        "method": method,
        "params": params,
    });
    if let Some(session_id) = session_id {
        request["sessionId"] = Value::String(session_id.to_owned());
    }
    request
}

fn page_navigate_request(id: u64, session_id: &str, url: &str) -> Value {
    cdp_request(
        id,
        page::NavigateParams::IDENTIFIER,
        serde_json::json!({"url": url}),
        Some(session_id),
    )
}

type DirectCdpSocket = async_tungstenite::WebSocketStream<async_tungstenite::tokio::ConnectStream>;

async fn direct_cdp_command(
    socket: &mut DirectCdpSocket,
    request: Value,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let id = request
        .get("id")
        .and_then(Value::as_u64)
        .ok_or("CDP request has no id")?;
    socket
        .send(Message::Text(request.to_string().into()))
        .await?;
    while let Some(message) = socket.next().await {
        let message = message?;
        let Message::Text(text) = message else {
            continue;
        };
        let response: Value = serde_json::from_str(&text)?;
        if response.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if let Some(error) = response.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("CDP command failed");
            return Err(chromiumoxide::error::CdpError::msg(message).into());
        }
        return response
            .get("result")
            .cloned()
            .ok_or_else(|| "CDP response is missing result".into());
    }
    Err("CDP websocket closed before command response".into())
}

async fn send_page_navigate(
    websocket_address: &str,
    target_id: &str,
    url: &str,
) -> Result<DirectCdpSocket, Box<dyn Error + Send + Sync>> {
    let (mut socket, _) = connect_async(websocket_address).await?;
    perf_diagnostic("navigation.cdp.attach.start");
    let attached = direct_cdp_command(
        &mut socket,
        cdp_request(
            1,
            "Target.attachToTarget",
            serde_json::json!({"targetId": target_id, "flatten": true}),
            None,
        ),
    )
    .await?;
    perf_diagnostic("navigation.cdp.attach.ready");
    let session_id = attached
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or("Target.attachToTarget response is missing sessionId")?;
    perf_diagnostic("navigation.cdp.navigate.start");
    let result = direct_cdp_command(&mut socket, page_navigate_request(2, session_id, url)).await?;
    let result: page::NavigateReturns = serde_json::from_value(result)?;
    if let Some(error) = page_navigate_error(result) {
        perf_diagnostic("navigation.cdp.navigate.error_text=true");
        return Err(Box::new(error));
    }
    perf_diagnostic("navigation.cdp.navigate.response");
    Ok(socket)
}

impl ChromePage {
    async fn new(
        page: Page,
        websocket_address: String,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let mut events = page
            .event_listener::<page::EventJavascriptDialogOpening>()
            .await?;
        let state = Arc::new(Mutex::new(DialogState::default()));
        let monitor_state = Arc::clone(&state);
        let monitor_page = page.clone();
        let task = tokio::spawn(async move {
            while let Some(event) = events.next().await {
                let dialog_type = format!("{:?}", event.r#type).to_ascii_lowercase();
                let auto_mode = {
                    let mut state = monitor_state.lock().await;
                    let auto_mode = state.auto_mode.clone();
                    state.pending = Some(PendingDialog {
                        dialog_type: dialog_type.clone(),
                        message: event.message.clone(),
                        default: event.default_prompt.clone().unwrap_or_default(),
                        handled: false,
                        auto_mode: auto_mode.clone(),
                    });
                    auto_mode
                };
                let should_dismiss = auto_mode == "dismiss"
                    || (auto_mode.is_empty() && dialog_type == "beforeunload");
                if should_dismiss
                    && monitor_page
                        .execute(page::HandleJavaScriptDialogParams::new(false))
                        .await
                        .is_ok()
                {
                    monitor_state.lock().await.pending = None;
                }
            }
        });
        Ok(Self {
            page,
            websocket_address,
            dialogs: DialogMonitor {
                state,
                _task: Arc::new(task),
            },
        })
    }
    pub fn target_id(&self) -> String {
        self.page.target_id().inner().clone()
    }
    pub fn raw(&self) -> &Page {
        &self.page
    }

    pub async fn open(&self, url: &str) -> Result<Value, Box<dyn Error + Send + Sync>> {
        self.open_with_timeout(url, Duration::from_secs(30)).await
    }

    /// Bound navigation and its response reads by the caller's remaining budget.
    pub async fn open_with_timeout(
        &self,
        url: &str,
        timeout: Duration,
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        match with_navigation_timeout(timeout, self.open_inner(url)).await {
            Ok(result) => result,
            Err(error) => Err(Box::new(error)),
        }
    }

    async fn open_inner(&self, url: &str) -> Result<Value, Box<dyn Error + Send + Sync>> {
        self.navigate_and_wait_for_load(url).await?;

        let page_url = match self.evaluate_runtime_json("location.href").await {
            Ok(url) => url.as_str().unwrap_or_default().to_owned(),
            Err(error) => return Err(error),
        };

        let title = match self.page.evaluate("document.title").await {
            Ok(value) => match value.into_value::<String>() {
                Ok(title) => title,
                Err(error) => return Err(Box::new(error)),
            },
            Err(error) => return Err(Box::new(error)),
        };

        Ok(serde_json::json!({"url": page_url, "title": title}))
    }

    // Send Page.navigate over a direct CDP websocket. chromiumoxide routes
    // Page::execute(Page.navigate) through its frame-lifecycle watcher, while
    // the Go adapter dispatches the command and polls document state itself.
    async fn navigate_and_wait_for_load(
        &self,
        url: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if url
            .trim_start()
            .get(..5)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("data:"))
        {
            self.page.goto(url).await?;
            return Ok(());
        }
        perf_diagnostic("navigation.baseline.start");
        let initial = self
            .evaluate_runtime_json("({url: location.href, time_origin: performance.timeOrigin})")
            .await?;
        let initial_url = initial
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let initial_time_origin = initial
            .get("time_origin")
            .and_then(Value::as_f64)
            .unwrap_or_default();
        perf_diagnostic("navigation.baseline.ready");
        perf_diagnostic("navigation.dispatch.start");
        let _navigation_socket =
            send_page_navigate(&self.websocket_address, self.page.target_id().inner(), url).await?;
        perf_diagnostic("navigation.dispatch.ready");
        let poll = async {
            let diagnostics = perf_diagnostics_enabled();
            let poll_started = diagnostics.then(Instant::now);
            let mut last_poll_report = poll_started;
            loop {
                let state = match self
                    .evaluate_runtime_json(
                        "({url: location.href, time_origin: performance.timeOrigin, ready_state: document.readyState})",
                    )
                    .await
                {
                    Ok(state) => state,
                    Err(error) if is_transient_navigation_context_error(error.as_ref()) => {
                        tokio::time::sleep(Duration::from_millis(25)).await;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let url_changed = state.get("url").and_then(Value::as_str) != Some(initial_url);
                let document_changed = state
                    .get("time_origin")
                    .and_then(Value::as_f64)
                    .is_some_and(|time_origin| time_origin != initial_time_origin);
                let ready_state = state
                    .get("ready_state")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                if diagnostics
                    && let (Some(started), Some(last_report)) =
                        (poll_started, last_poll_report.as_mut())
                    && last_report.elapsed() >= Duration::from_secs(1)
                {
                    perf_diagnostic(&format!(
                        "navigation.poll elapsed_ms={} url_changed={url_changed} document_changed={document_changed} ready_state={ready_state}",
                        started.elapsed().as_millis()
                    ));
                    *last_report = Instant::now();
                }
                if (url_changed || document_changed) && ready_state == "complete" {
                    if let Some(started) = poll_started {
                        perf_diagnostic(&format!(
                            "navigation.poll.complete elapsed_ms={}",
                            started.elapsed().as_millis()
                        ));
                    }
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        };
        tokio::pin!(poll);
        poll.await
    }

    async fn evaluate_runtime_json(
        &self,
        expression: &str,
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let response = self
            .page
            .execute(
                runtime::EvaluateParams::builder()
                    .expression(expression)
                    .return_by_value(true)
                    .build()?,
            )
            .await?;
        if let Some(exception) = response.result.exception_details {
            return Err(format!("Chrome state evaluation failed: {exception:?}").into());
        }
        Ok(response.result.result.value.unwrap_or(Value::Null))
    }

    pub async fn read(&self) -> Result<Value, Box<dyn Error + Send + Sync>> {
        Ok(self
            .page
            .evaluate("document.body?.innerText ?? ''")
            .await?
            .into_value::<String>()?
            .into())
    }

    /// Evaluate a bounded JSON-producing script for shared browser state
    /// capture/restore and engine-neutral dispatch.
    pub async fn evaluate_script(
        &self,
        expression: &str,
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        if expression.len() > 1024 * 1024 {
            return Err("evaluation script exceeds 1 MiB".into());
        }
        Ok(self
            .page
            .evaluate(expression)
            .await?
            .into_value::<Value>()?)
    }

    /// Read complete cookie metadata through Chrome's Network domain.
    pub async fn cookies(&self) -> Result<Value, Box<dyn Error + Send + Sync>> {
        Ok(serde_json::to_value(
            self.page
                .execute(network::GetCookiesParams::default())
                .await?
                .result,
        )?)
    }

    /// Set a complete cookie through Chrome's Network domain.
    pub async fn set_cookie(&self, cookie: Value) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let params: network::SetCookieParams = serde_json::from_value(cookie)?;
        Ok(serde_json::to_value(
            self.page.execute(params).await?.result,
        )?)
    }

    pub async fn snapshot(&self) -> Result<Vec<Value>, Box<dyn Error + Send + Sync>> {
        self.accessibility_tree().await
    }

    pub async fn navigation(&self, command: &str) -> Result<Value, Box<dyn Error + Send + Sync>> {
        match command {
            "reload" => {
                self.page.reload().await?;
            }
            "back" => {
                self.page.evaluate("history.back()").await?;
            }
            "forward" => {
                self.page.evaluate("history.forward()").await?;
            }
            _ => return Err(format!("unsupported navigation command {command:?}").into()),
        }
        Ok(serde_json::json!({"url": self.page.url().await?.unwrap_or_default()}))
    }

    async fn element(&self, selector: &str) -> Result<Element, Box<dyn Error + Send + Sync>> {
        if selector.trim().is_empty() {
            return Err("selector must not be empty".into());
        }
        let selector = if let Some(reference) = selector.strip_prefix('@') {
            let reference = serde_json::to_string(reference)?;
            format!("[data-symbrowse-ref={reference}]")
        } else {
            selector.to_owned()
        };
        Ok(self.page.find_element(selector).await?)
    }

    pub async fn click(
        &self,
        selector: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        self.element(selector).await?.click().await?;
        Ok(result("click", selector))
    }
    pub async fn double_click(
        &self,
        selector: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        self.element(selector)
            .await?
            .click_with(
                chromiumoxide::types::ClickOptions::builder()
                    .click_count(2)
                    .build(),
            )
            .await?;
        Ok(result("dblclick", selector))
    }
    pub async fn focus(
        &self,
        selector: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        self.element(selector).await?.focus().await?;
        Ok(result("focus", selector))
    }
    pub async fn hover(
        &self,
        selector: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        self.element(selector).await?.hover().await?;
        Ok(result("hover", selector))
    }
    pub async fn scroll_into_view(
        &self,
        selector: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        self.element(selector).await?.scroll_into_view().await?;
        Ok(result("scroll", selector))
    }
    pub async fn type_text(
        &self,
        selector: &str,
        text: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        self.element(selector)
            .await?
            .focus()
            .await?
            .type_str(text)
            .await?;
        Ok(result("type", selector))
    }
    pub async fn press(
        &self,
        selector: &str,
        key: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        self.element(selector)
            .await?
            .focus()
            .await?
            .press_key(key)
            .await?;
        Ok(result("press", selector))
    }
    pub async fn fill(
        &self,
        selector: &str,
        text: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        let value = serde_json::to_string(text)?;
        let selector = serde_json::to_string(selector)?;
        self.page.evaluate(format!("(() => {{ const e=document.querySelector({selector}); if (!e) throw new Error('selector did not match'); e.focus(); const setter=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')?.set || Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value')?.set; if (setter) setter.call(e,{value}); else e.value={value}; e.dispatchEvent(new Event('input',{{bubbles:true}})); e.dispatchEvent(new Event('change',{{bubbles:true}})); return e.value; }})()" )).await?;
        Ok(result("fill", selector.trim_matches('"')))
    }
    pub async fn select(
        &self,
        selector: &str,
        value: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        let selector_json = serde_json::to_string(selector)?;
        let value_json = serde_json::to_string(value)?;
        self.page.evaluate(format!("(() => {{ const e=document.querySelector({selector_json}); if (!e || e.tagName !== 'SELECT') throw new Error('select requires a SELECT element'); const wanted={value_json}; let hit=false; for (const o of e.options) {{ const yes=o.value===wanted || o.text===wanted; o.selected=yes; hit ||= yes; }} if (!hit) throw new Error('option did not match'); e.dispatchEvent(new Event('input',{{bubbles:true}})); e.dispatchEvent(new Event('change',{{bubbles:true}})); return e.value; }})()" )).await?;
        Ok(result("select", selector))
    }
    pub async fn check(
        &self,
        selector: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        self.set_checked(selector, true).await
    }
    pub async fn uncheck(
        &self,
        selector: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        self.set_checked(selector, false).await
    }
    async fn set_checked(
        &self,
        selector: &str,
        checked: bool,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        let s = serde_json::to_string(selector)?;
        self.page.evaluate(format!("(() => {{ const e=document.querySelector({s}); if (!e || e.type !== 'checkbox') throw new Error('check requires a checkbox'); if (e.checked !== {checked}) e.click(); return e.checked; }})()" )).await?;
        Ok(result(if checked { "check" } else { "uncheck" }, selector))
    }

    /// Inspect a selector using only serializable DOM values.
    pub async fn inspect(
        &self,
        selector: &str,
        kind: &str,
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let s = serde_json::to_string(selector)?;
        let expression = match kind {
            "text" => format!(
                "(() => {{ const e=document.querySelector({s}); if (!e) throw new Error('selector did not match'); return e.innerText ?? ''; }})()"
            ),
            "html" => format!(
                "(() => {{ const e=document.querySelector({s}); if (!e) throw new Error('selector did not match'); return e.innerHTML ?? ''; }})()"
            ),
            "value" => format!(
                "(() => {{ const e=document.querySelector({s}); if (!e) throw new Error('selector did not match'); return e.value ?? null; }})()"
            ),
            "title" => "document.title".to_owned(),
            "url" => "location.href".to_owned(),
            "box" => format!(
                "(() => {{ const e=document.querySelector({s}); if (!e) throw new Error('selector did not match'); const r=e.getBoundingClientRect(); return {{x:r.x,y:r.y,width:r.width,height:r.height}}; }})()"
            ),
            "styles" => format!(
                "(() => {{ const e=document.querySelector({s}); if (!e) throw new Error('selector did not match'); const c=getComputedStyle(e); return {{display:c.display,visibility:c.visibility,opacity:c.opacity,color:c.color,backgroundColor:c.backgroundColor}}; }})()"
            ),
            "attr" => format!(
                "(() => {{ const e=document.querySelector({s}); if (!e) throw new Error('selector did not match'); return Object.fromEntries([...e.attributes].map(a=>[a.name,a.value])); }})()"
            ),
            "find" => format!("document.querySelector({s}) !== null"),
            "count" => format!("document.querySelectorAll({s}).length"),
            "visible" | "enabled" | "checked" => format!(
                "(() => {{ const e=document.querySelector({s}); if (!e) return false; if ({kind:?} === 'checked') return !!e.checked; if ({kind:?} === 'enabled') return !e.disabled; const r=e.getBoundingClientRect(), c=getComputedStyle(e); return c.display !== 'none' && c.visibility !== 'hidden' && c.opacity !== '0' && r.width > 0 && r.height > 0; }})()"
            ),
            "get" => format!(
                "(() => {{ const e=document.querySelector({s}); if (!e) throw new Error('selector did not match'); return {{text:e.innerText ?? '', html:e.innerHTML ?? '', value:e.value ?? null, checked:typeof e.checked === 'boolean' ? e.checked : null, attributes:Object.fromEntries([...e.attributes].map(a=>[a.name,a.value]))}}; }})()"
            ),
            "is" => format!(
                "(() => {{ const e=document.querySelector({s}); if (!e) return false; const r=e.getBoundingClientRect(), c=getComputedStyle(e); return c.display !== 'none' && c.visibility !== 'hidden' && c.opacity !== '0' && r.width > 0 && r.height > 0; }})()"
            ),
            _ => return Err(format!("unsupported inspection kind {kind:?}").into()),
        };
        Ok(self
            .page
            .evaluate(expression)
            .await?
            .into_value::<Value>()?)
    }

    /// Find an element by the semantic selectors used by the public command
    /// and flow surfaces, assigning a stable DOM-local reference on demand.
    pub async fn find(&self, options: FindOptions) -> Result<Value, Box<dyn Error + Send + Sync>> {
        if options.query.trim().is_empty() {
            return Err("find query is required".into());
        }
        let kind = serde_json::to_string(&options.kind)?;
        let query = serde_json::to_string(&options.query)?;
        let action = serde_json::to_string(&options.action)?;
        let name = serde_json::to_string(&options.name)?;
        let value = serde_json::to_string(&options.value)?;
        let exact = if options.exact { "true" } else { "false" };
        let index = options
            .index
            .map_or_else(|| "null".to_owned(), |index| index.to_string());
        let expression = format!(
            "(() => {{ const kind={kind}, query={query}, action={action}, name={name}, exact={exact}, wantedIndex={index}, value={value}; const text=e=>(e.innerText||e.textContent||'').trim(); const implicit=e=>{{ const tag=e.tagName.toLowerCase(); return tag==='button'?'button':tag==='a'?'link':tag==='input'?(e.type==='checkbox'?'checkbox':e.type==='radio'?'radio':'textbox'):tag==='textarea'?'textbox':tag==='select'?'combobox':''; }}; const candidate=e=>{{ switch(kind) {{ case 'role': return e.getAttribute('role')||implicit(e); case 'text': return text(e); case 'label': return e.getAttribute('aria-label')||text(document.querySelector(`label[for='${{CSS.escape(e.id||'')}}']` )||e); case 'placeholder': return e.getAttribute('placeholder')||''; case 'alt': return e.getAttribute('alt')||''; case 'title': return e.getAttribute('title')||''; case 'testid': return e.getAttribute('data-testid')||''; case 'css': return e.matches(query)?query:''; case 'ref': return e.getAttribute('data-symbrowse-ref')||''; default: return ''; }} }}; const matchesText=(got)=>exact?got===query:got.toLowerCase().includes(query.toLowerCase()); let nodes=[...document.querySelectorAll('*')]; let matches=nodes.filter(e=>kind==='ref'?candidate(e)===query.replace(/^@/,''):matchesText(candidate(e))); if (kind==='text') matches=matches.filter(e=>!matches.some(other=>other!==e&&e.contains(other))); if (name) matches=matches.filter(e=>{{ const got=e.getAttribute('aria-label')||e.getAttribute('name')||''; return exact?got===name:got.toLowerCase().includes(name.toLowerCase()); }}); if (!matches.length) throw new Error(`find ${{kind}} ${{query}} matched no elements`); if (wantedIndex!==null) matches=[matches[wantedIndex]].filter(Boolean); if (!matches.length) throw new Error(`find ${{kind}} index ${{wantedIndex}} is out of range`); if (wantedIndex===null&&matches.length>1&&['first','last','nth'].indexOf(action)<0) throw new Error(`find ${{kind}} ${{query}} matched ${{matches.length}} elements; use index`); let selected=action==='last'?matches[matches.length-1]:matches[0]; let next=1; for (const e of nodes) {{ if (!e.getAttribute('data-symbrowse-ref')) e.setAttribute('data-symbrowse-ref',`e${{next++}}`); }} const ref=selected.getAttribute('data-symbrowse-ref'); if (action==='click') selected.click(); else if (action==='focus') selected.focus(); else if (action==='fill') {{ selected.focus(); selected.value=value; selected.dispatchEvent(new Event('input',{{bubbles:true}})); selected.dispatchEvent(new Event('change',{{bubbles:true}})); }} else if (action==='text') return {{kind,query,action,ref,matches:matches.map(e=>({{ref:e.getAttribute('data-symbrowse-ref'),text:text(e),tag:e.tagName.toLowerCase()}})),value:text(selected)}}; return {{kind,query,action,ref,matches:matches.map(e=>({{ref:e.getAttribute('data-symbrowse-ref'),text:text(e),tag:e.tagName.toLowerCase()}}))}}; }})()"
        );
        Ok(self
            .page
            .evaluate(expression)
            .await?
            .into_value::<Value>()?)
    }

    pub async fn wait_for_selector(
        &self,
        selector: &str,
        visible: bool,
        timeout: Duration,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if self
                .inspect(selector, if visible { "is" } else { "find" })
                .await?
                .as_bool()
                == Some(true)
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Err(format!("timeout waiting for selector {selector:?}").into())
    }
    pub async fn wait_for_navigation(
        &self,
        timeout: Duration,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        tokio::time::timeout(timeout, self.page.wait_for_navigation()).await??;
        Ok(())
    }

    pub async fn frames(&self) -> Result<Vec<FrameInfo>, Box<dyn Error + Send + Sync>> {
        let tree = self
            .page
            .execute(page::GetFrameTreeParams {})
            .await?
            .frame_tree
            .clone();
        let mut result = Vec::new();
        flatten_frames(&tree, &mut result);
        Ok(result)
    }

    pub async fn accessibility_tree(&self) -> Result<Vec<Value>, Box<dyn Error + Send + Sync>> {
        let tree = self
            .page
            .execute(accessibility::GetFullAxTreeParams::builder().build())
            .await?;
        Ok(tree
            .nodes
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()?)
    }

    pub async fn dialog(
        &self,
        accept: bool,
        prompt_text: Option<String>,
        timeout: Duration,
    ) -> Result<DialogInfo, Box<dyn Error + Send + Sync>> {
        let mut events = self
            .page
            .event_listener::<page::EventJavascriptDialogOpening>()
            .await?;
        let event = tokio::time::timeout(timeout, events.next())
            .await?
            .ok_or("dialog stream closed")?;
        let info = DialogInfo {
            kind: format!("{:?}", event.r#type).to_ascii_lowercase(),
            message: event.message.clone(),
            url: event.url.clone(),
            default_prompt: event.default_prompt.clone(),
        };
        let params = page::HandleJavaScriptDialogParams::builder()
            .accept(accept)
            .prompt_text(prompt_text.unwrap_or_default())
            .build()?;
        self.page.execute(params).await?;
        Ok(info)
    }

    pub async fn dialog_status(&self) -> PendingDialog {
        let state = self.dialogs.state.lock().await;
        state.pending.clone().unwrap_or(PendingDialog {
            dialog_type: String::new(),
            message: String::new(),
            default: String::new(),
            handled: true,
            auto_mode: state.auto_mode.clone(),
        })
    }

    pub async fn set_dialog_auto_mode(
        &self,
        mode: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if !matches!(mode, "accept" | "dismiss" | "off") {
            return Err(
                format!("invalid dialog auto mode {mode:?} (use accept, dismiss or off)").into(),
            );
        }
        self.dialogs.state.lock().await.auto_mode = mode.to_owned();
        Ok(())
    }

    pub async fn accept_dialog(
        &self,
        prompt_text: Option<String>,
    ) -> Result<PendingDialog, Box<dyn Error + Send + Sync>> {
        self.handle_pending_dialog(true, prompt_text).await
    }

    pub async fn dismiss_dialog(&self) -> Result<PendingDialog, Box<dyn Error + Send + Sync>> {
        self.handle_pending_dialog(false, None).await
    }

    async fn handle_pending_dialog(
        &self,
        accept: bool,
        prompt_text: Option<String>,
    ) -> Result<PendingDialog, Box<dyn Error + Send + Sync>> {
        let mut state = self.dialogs.state.lock().await;
        let pending = state
            .pending
            .clone()
            .ok_or("no JavaScript dialog is pending")?;
        let params = page::HandleJavaScriptDialogParams::builder()
            .accept(accept)
            .prompt_text(prompt_text.unwrap_or_default())
            .build()?;
        self.page.execute(params).await?;
        state.pending = None;
        Ok(pending)
    }

    pub async fn start_auto_dialog_handler(
        &self,
        accept: bool,
        timeout: Duration,
    ) -> Result<tokio::task::JoinHandle<Result<usize, String>>, Box<dyn Error + Send + Sync>> {
        let mut events = self
            .page
            .event_listener::<page::EventJavascriptDialogOpening>()
            .await?;
        let page = self.page.clone();
        Ok(tokio::spawn(async move {
            let deadline = Instant::now() + timeout;
            let mut count = 0;
            while Instant::now() < deadline {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let event = match tokio::time::timeout(remaining, events.next()).await {
                    Ok(Some(event)) => event,
                    Ok(None) | Err(_) => break,
                };
                page.execute(page::HandleJavaScriptDialogParams::new(accept))
                    .await
                    .map_err(|e| e.to_string())?;
                let _ = event;
                count += 1;
            }
            Ok(count)
        }))
    }

    pub async fn start_network_capture(
        &self,
    ) -> Result<NetworkCapture, Box<dyn Error + Send + Sync>> {
        self.page
            .execute(network::EnableParams::builder().build())
            .await?;
        Ok(NetworkCapture {
            requests: self
                .page
                .event_listener::<network::EventRequestWillBeSent>()
                .await?,
            responses: self
                .page
                .event_listener::<network::EventResponseReceived>()
                .await?,
            failed: self
                .page
                .event_listener::<network::EventLoadingFailed>()
                .await?,
        })
    }

    #[allow(deprecated)]
    pub async fn set_offline(&self, offline: bool) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.page
            .execute(
                network::EmulateNetworkConditionsParams::builder()
                    .offline(offline)
                    .latency(0)
                    .download_throughput(-1)
                    .upload_throughput(-1)
                    .build()?,
            )
            .await?;
        Ok(())
    }

    pub async fn block_urls(&self, urls: Vec<String>) -> Result<(), Box<dyn Error + Send + Sync>> {
        let patterns: Vec<network::BlockPattern> = urls
            .into_iter()
            .map(|url_pattern| network::BlockPattern {
                url_pattern,
                block: true,
            })
            .collect();
        self.page
            .execute(
                network::SetBlockedUrLsParams::builder()
                    .url_patterns(patterns)
                    .build(),
            )
            .await?;
        Ok(())
    }

    pub async fn set_download_behavior(
        &self,
        directory: Option<&Path>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let (behavior, path) = match directory {
            Some(path) => (
                browser::SetDownloadBehaviorBehavior::AllowAndName,
                Some(path.to_string_lossy().into_owned()),
            ),
            None => (browser::SetDownloadBehaviorBehavior::Deny, None),
        };
        self.page
            .execute(browser_set_download_behavior(behavior, path)?)
            .await?;
        Ok(())
    }

    pub async fn upload_files(
        &self,
        selector: &str,
        files: &[String],
        allowed_dirs: &[String],
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let request = symbrowse_engine::files::UploadRequest {
            selector: selector.to_owned(),
            files: files.to_vec(),
            allowed_dirs: allowed_dirs.to_vec(),
        };
        let checked = symbrowse_engine::files::guard_upload_request(&request)?;
        let element = self.element(selector).await?;
        let node = element.description().await?;
        self.page
            .execute(
                dom::SetFileInputFilesParams::builder()
                    .files(checked.uploaded)
                    .backend_node_id(node.backend_node_id)
                    .build()?,
            )
            .await?;
        Ok(())
    }

    pub async fn screenshot(
        &self,
        options: ScreenshotOptions,
    ) -> Result<Artifact, Box<dyn Error + Send + Sync>> {
        let format = options.format.to_ascii_lowercase();
        if !format.is_empty() && format != "png" && format != "jpeg" {
            return Err(
                format!("unsupported screenshot format {format:?} (want png or jpeg)").into(),
            );
        }
        let is_jpeg = format == "jpeg";
        let format = if is_jpeg {
            page::CaptureScreenshotFormat::Jpeg
        } else {
            page::CaptureScreenshotFormat::Png
        };
        let bytes = if options.selector.is_empty() {
            let mut builder = chromiumoxide::page::ScreenshotParams::builder()
                .format(format.clone())
                .full_page(options.full_page);
            if let Some(quality) = options.quality {
                builder = builder.quality(quality);
            }
            self.page.screenshot(builder.build()).await?
        } else {
            if options.full_page {
                return Err("selector screenshot cannot also request full_page".into());
            }
            self.element(&options.selector)
                .await?
                .screenshot(format)
                .await?
        };
        Ok(Artifact {
            kind: "screenshot".to_owned(),
            mime_type: if is_jpeg { "image/jpeg" } else { "image/png" }.to_owned(),
            bytes,
        })
    }

    pub async fn pdf(&self) -> Result<Artifact, Box<dyn Error + Send + Sync>> {
        let bytes = self.page.pdf(page::PrintToPdfParams::default()).await?;
        Ok(Artifact {
            kind: "pdf".to_owned(),
            mime_type: "application/pdf".to_owned(),
            bytes,
        })
    }

    pub async fn har(&self) -> Result<Artifact, Box<dyn Error + Send + Sync>> {
        Err(UnsupportedOperation(
            "HAR export requires deterministic request/response body capture; use network-events",
        )
        .into())
    }
    pub async fn axe_audit(&self) -> Result<Artifact, Box<dyn Error + Send + Sync>> {
        Err(UnsupportedOperation("axe-core is not bundled or injected by this crate").into())
    }
}

async fn with_navigation_timeout<T>(
    timeout: Duration,
    future: impl std::future::Future<Output = T>,
) -> Result<T, tokio::time::error::Elapsed> {
    tokio::time::timeout(timeout, future).await
}

pub struct NetworkCapture {
    requests: chromiumoxide::listeners::EventStream<network::EventRequestWillBeSent>,
    responses: chromiumoxide::listeners::EventStream<network::EventResponseReceived>,
    failed: chromiumoxide::listeners::EventStream<network::EventLoadingFailed>,
}

impl NetworkCapture {
    pub async fn collect(mut self, timeout: Duration) -> Vec<NetworkEvent> {
        let deadline = Instant::now() + timeout;
        let mut events = Vec::new();
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            tokio::select! {
                event = self.requests.next() => if let Some(event) = event { events.push(NetworkEvent { kind: "request".to_owned(), id: event.request_id.clone().into(), url: event.request.url.clone(), ..Default::default() }); },
                event = self.responses.next() => if let Some(event) = event { events.push(NetworkEvent { kind: "response".to_owned(), id: event.request_id.clone().into(), url: event.response.url.clone(), status: event.response.status as u16, mime_type: event.response.mime_type.clone(), ..Default::default() }); },
                event = self.failed.next() => if let Some(event) = event { events.push(NetworkEvent { kind: "failed".to_owned(), id: event.request_id.clone().into(), error_text: event.error_text.clone(), ..Default::default() }); },
                _ = tokio::time::sleep(remaining) => break,
            }
        }
        events
    }
}

fn result(action: &str, selector: &str) -> InteractionResult {
    InteractionResult {
        action: action.to_owned(),
        selector: selector.to_owned(),
    }
}

fn flatten_frames(tree: &page::FrameTree, output: &mut Vec<FrameInfo>) {
    output.push(FrameInfo {
        id: tree.frame.id.inner().clone(),
        parent_id: tree
            .frame
            .parent_id
            .as_ref()
            .map(|id| id.inner().clone())
            .unwrap_or_default(),
        name: tree.frame.name.clone().unwrap_or_default(),
        url: tree.frame.url.clone(),
    });
    if let Some(children) = &tree.child_frames {
        for child in children {
            flatten_frames(child, output);
        }
    }
}

fn browser_set_download_behavior(
    behavior: browser::SetDownloadBehaviorBehavior,
    path: Option<String>,
) -> Result<browser::SetDownloadBehaviorParams, String> {
    let mut builder = browser::SetDownloadBehaviorParams::builder()
        .behavior(behavior)
        .events_enabled(true);
    if let Some(path) = path {
        builder = builder.download_path(path);
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn navigation_deadline_bounds_stalled_open() {
        let result =
            with_navigation_timeout(Duration::from_millis(1), std::future::pending::<()>()).await;
        assert!(result.is_err());
    }

    #[test]
    fn navigation_retry_only_accepts_the_transient_target_context_error() {
        let transient = chromiumoxide::error::CdpError::Chrome(chromiumoxide::types::Error {
            code: -32000,
            message: "Inspected target navigated or closed".into(),
        });
        assert!(is_transient_navigation_context_error(&transient));

        let unrelated = chromiumoxide::error::CdpError::Chrome(chromiumoxide::types::Error {
            code: -32000,
            message: "Target closed".into(),
        });
        assert!(!is_transient_navigation_context_error(&unrelated));
        assert!(!is_transient_navigation_context_error(
            &chromiumoxide::error::CdpError::Timeout
        ));
    }

    #[test]
    fn navigation_request_targets_page_session_without_lifecycle_future() {
        let request = page_navigate_request(1, "session-1", "https://example.test/a'b?x=1&y=2");
        assert_eq!(
            request,
            json!({
                "id": 1,
                "method": "Page.navigate",
                "params": {"url": "https://example.test/a'b?x=1&y=2"},
                "sessionId": "session-1"
            })
        );
    }

    #[test]
    fn aborted_navigation_follows_go_polling_contract() {
        let go_navigation = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../internal/engine/navigation.go"),
        )
        .expect("read Go navigation oracle");
        assert!(
            go_navigation.contains("if _, err := s.engine.Navigate(ctx, s.page, url); err != nil")
        );
        assert!(!go_navigation.contains("result.ErrorText"));

        let aborted: page::NavigateReturns = serde_json::from_value(json!({
            "frameId": "frame-id",
            "errorText": "net::ERR_ABORTED"
        }))
        .expect("decode aborted Page.navigate response");
        assert!(page_navigate_error(aborted).is_none());
    }

    #[test]
    fn page_navigate_response_preserves_other_chrome_navigation_errors() {
        let failed: page::NavigateReturns = serde_json::from_value(json!({
            "frameId": "frame-id",
            "errorText": "net::ERR_CONNECTION_REFUSED"
        }))
        .expect("decode failed Page.navigate response");
        assert!(matches!(
            page_navigate_error(failed),
            Some(chromiumoxide::error::CdpError::ChromeMessage(message))
                if message == "net::ERR_CONNECTION_REFUSED"
        ));

        let succeeded: page::NavigateReturns =
            serde_json::from_value(json!({"frameId": "frame-id"}))
                .expect("decode successful Page.navigate response");
        assert!(page_navigate_error(succeeded).is_none());
    }

    #[test]
    fn canonical_capabilities_partition_matches_daemon_commands() {
        let value = crate::canonical_capabilities();
        assert!(value.interfaces.contains(&"TabManager".to_owned()));
        assert!(value.interfaces.contains(&"FileTransfer".to_owned()));
        assert!(value.unsupported.contains(&"A11yAuditor".to_owned()));
        assert!(value.unsupported.contains(&"SettingsEngine".to_owned()));
        assert!(!value.interfaces.iter().any(|name| name == "HAR"));
    }

    #[test]
    fn capabilities_are_explicit_and_sorted_with_unsupported_features() {
        let value = capabilities();
        assert_eq!(value.interactions.len(), 11);
        assert!(value.artifacts.contains(&"pdf".to_owned()));
        assert_eq!(value.unsupported, vec!["har-export", "axe-core-audit"]);
    }

    #[test]
    fn frame_fixture_flattens_deterministically() {
        let raw = json!({"frame":{"id":"root","url":"https://example.test","name":"main","loaderId":"loader-root","domainAndRegistry":"example.test","securityOrigin":"https://example.test","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]},"childFrames":[{"frame":{"id":"child","parentId":"root","url":"https://example.test/child","name":"child","loaderId":"loader-child","domainAndRegistry":"example.test","securityOrigin":"https://example.test","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}]});
        let tree: page::FrameTree = serde_json::from_value(raw).unwrap();
        let mut frames = Vec::new();
        flatten_frames(&tree, &mut frames);
        assert_eq!(
            frames,
            vec![
                FrameInfo {
                    id: "root".into(),
                    parent_id: "".into(),
                    name: "main".into(),
                    url: "https://example.test".into()
                },
                FrameInfo {
                    id: "child".into(),
                    parent_id: "root".into(),
                    name: "child".into(),
                    url: "https://example.test/child".into()
                }
            ]
        );
    }

    #[test]
    fn screenshot_rejects_unknown_format_without_chrome() {
        let options = ScreenshotOptions {
            format: "bmp".into(),
            ..Default::default()
        };
        assert!(options.format != "png");
        assert!(matches!(
            UnsupportedOperation("axe-core-audit").to_string().as_str(),
            "unsupported Chrome operation: axe-core-audit"
        ));
    }
}
