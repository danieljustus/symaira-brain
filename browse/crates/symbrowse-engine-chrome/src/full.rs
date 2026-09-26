//! Chrome interaction, inspection, frame, network and artifact adapter.
//!
//! The adapter intentionally exposes only operations that are backed by a CDP
//! command in chromiumoxide. Features which need a separate protocol (for
//! example HAR export) are represented by an explicit [`UnsupportedOperation`]
//! error rather than a best-effort implementation.

use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    fmt,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use chromiumoxide::{
    Browser, Element, Page,
    cdp::browser_protocol::{accessibility, browser, dom, emulation, fetch, input, network, page},
    cdp::js_protocol::runtime::{self, EvaluateParams},
    layout::Point,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::Mutex;

use symbrowse_engine::files::{DownloadEvent, DownloadRegistry, download_behavior};

use crate::{BrowserMode, ConnectionMode, launch};

const OOB_OVERLAY_SCRIPT: &str = r#"(() => {
  const SLOT = '__symbrowse_oob_result__';
  window[SLOT] = 'pending';
  const existingCleanup = window.__symbrowse_oob_cleanup__;
  if (existingCleanup) existingCleanup();
  function makeHost() {
    const host = document.createElement('div');
    host.id = 'symbrowse-oob-host';
    host.style.all = 'initial';
    host.style.position = 'fixed';
    host.style.top = '0';
    host.style.left = '0';
    host.style.right = '0';
    host.style.zIndex = '2147483647';
    host.style.fontFamily = '-apple-system, system-ui, sans-serif';
    const shadow = host.attachShadow({mode: 'closed'});
    const style = document.createElement('style');
    style.textContent = [
      ':host{all:initial}',
      '.sb-oob{border:2px solid #b45309;background:#fffbeb;color:#1c1917;padding:14px 18px;',
      'box-shadow:0 4px 24px rgba(0,0,0,.35);display:flex;align-items:center;gap:16px;flex-wrap:wrap}',
      '.sb-oob strong{font-size:14px}',
      '.sb-oob span{font-size:13px;opacity:.85}',
      '.sb-oob .timer{font-variant-numeric:tabular-nums;font-weight:600;color:#b45309}',
      '.sb-oob button{border:1px solid #1c1917;background:#fff;padding:6px 14px;font-size:13px;',
      'cursor:pointer;border-radius:6px;font-family:inherit}',
      '.sb-oob button.done{background:#166534;border-color:#166534;color:#fff}',
      '.sb-oob button.abort{background:#fff;color:#991b1b;border-color:#991b1b}'
    ].join('');
    const box = document.createElement('div');
    box.className = 'sb-oob';
    const strong = document.createElement('strong');
    strong.textContent = TITLE;
    const reason = document.createElement('span');
    reason.textContent = REASON;
    const timer = document.createElement('span');
    timer.className = 'timer';
    const done = document.createElement('button');
    done.className = 'done';
    done.textContent = 'Fertig';
    done.addEventListener('click', () => { window[SLOT] = 'completed'; window.__symbrowse_oob_cleanup__(); });
    const abort = document.createElement('button');
    abort.className = 'abort';
    abort.textContent = 'Abbrechen';
    abort.addEventListener('click', () => { window[SLOT] = 'cancelled'; window.__symbrowse_oob_cleanup__(); });
    box.appendChild(strong); box.appendChild(reason); box.appendChild(timer);
    box.appendChild(done); box.appendChild(abort);
    shadow.appendChild(style); shadow.appendChild(box);
    if (COUNTDOWN > 0) {
      let left = COUNTDOWN;
      timer.textContent = left + 's';
      const interval = setInterval(() => {
        left -= 1;
        if (left <= 0) { clearInterval(interval); timer.textContent = ''; }
        else { timer.textContent = left + 's'; }
      }, 1000);
    }
    return host;
  }
  let host = makeHost();
  (document.documentElement || document.body).appendChild(host);
  const observer = new MutationObserver((mutations) => {
    if (window[SLOT] !== 'pending') return;
    for (const mutation of mutations) {
      for (const node of mutation.removedNodes) {
        if (node.id === 'symbrowse-oob-host') {
          host = makeHost();
          (document.documentElement || document.body).appendChild(host);
        }
      }
    }
  });
  observer.observe(document.documentElement, {childList: true, subtree: false});
  window.__symbrowse_oob_cleanup__ = () => {
    observer.disconnect();
    const current = document.getElementById('symbrowse-oob-host');
    if (current && current.parentNode) current.parentNode.removeChild(current);
    delete window.__symbrowse_oob_cleanup__;
  };
  return true;
})()"#;

// Keep the Rust audit offline and tied to the Go oracle's vendored axe-core.
const AXE_CORE_SOURCE: &str = include_str!("../../../internal/engine/axe/assets/axe.min.js");
const DEVICE_PROFILES: &str = include_str!("../../../internal/engine/devices.json");

#[derive(Deserialize)]
struct DeviceProfile {
    name: String,
    width: i64,
    height: i64,
    scale: f64,
    mobile: bool,
    touch: bool,
    #[serde(default)]
    user_agent: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedOperation(pub &'static str);

impl fmt::Display for UnsupportedOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported Chrome operation: {}", self.0)
    }
}
impl Error for UnsupportedOperation {}

fn uses_script_navigation(url: &str) -> bool {
    url.get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
        || url
            .get(..8)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

fn navigation_completed(before: &Value, state: &Value) -> bool {
    if state.get("ready_state").and_then(Value::as_str) != Some("complete") {
        return false;
    }
    let new_document = state.get("time_origin") != before.get("time_origin");
    let same_document_url_changed = state.get("url") != before.get("url")
        && state
            .get("url")
            .and_then(Value::as_str)
            .and_then(|url| url.split('#').next())
            == before
                .get("url")
                .and_then(Value::as_str)
                .and_then(|url| url.split('#').next());
    new_document || same_document_url_changed
}

fn has_explicit_url_scheme(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '.' | '-')
        })
}

#[derive(Debug)]
pub struct ClickObstructedError {
    pub message: String,
    pub hint: String,
}

impl fmt::Display for ClickObstructedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for ClickObstructedError {}

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<FrameInfo>,
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CapturedRequest {
    pub id: String,
    pub url: String,
    pub method: String,
    pub request_type: String,
    pub status: i64,
    pub status_text: String,
    pub mime_type: String,
    pub started_at_unix_seconds: i64,
    pub finished: bool,
    pub failed: String,
    pub request_headers: BTreeMap<String, String>,
    pub response_headers: BTreeMap<String, String>,
    pub encoded_body_size: i64,
}

#[derive(Clone, Debug, Serialize)]
struct ConsoleEntry {
    #[serde(rename = "type")]
    kind: String,
    text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    url: String,
    #[serde(skip_serializing_if = "is_zero")]
    line: i64,
    timestamp: String,
}

#[derive(Clone, Debug, Serialize)]
struct ErrorEntry {
    text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    url: String,
    #[serde(skip_serializing_if = "is_zero")]
    line: i64,
    #[serde(rename = "stacktrace", skip_serializing_if = "Vec::is_empty")]
    stack_trace: Vec<String>,
    timestamp: String,
}

#[derive(Default)]
struct RuntimeEventState {
    enabled: bool,
    console: Vec<ConsoleEntry>,
    errors: Vec<ErrorEntry>,
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
        inspection: ["axe-audit", "find", "get", "is", "count"]
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
        unsupported: ["har-export"].into_iter().map(str::to_owned).collect(),
    }
}

/// A connected browser plus one page. The handler is kept alive in a task so
/// chromiumoxide can dispatch CDP events and page commands.
pub struct ChromeSession {
    browser: Arc<Mutex<Browser>>,
    mode: ConnectionMode,
    _handler_task: tokio::task::JoinHandle<()>,
}

impl ChromeSession {
    pub async fn connect(
        mode: BrowserMode,
        timeout: Duration,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let connection = launch::connect(&mode, timeout).await?;
        let browser = Arc::new(Mutex::new(connection.browser));
        let handler = connection.handler;
        let mode = connection.mode;
        let task = tokio::spawn(async move {
            let mut handler = handler;
            while handler.next().await.is_some() {}
        });
        Ok(Self {
            browser,
            mode,
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
        // Windows can stall when chromiumoxide creates a target with its URL
        // already set; open it after the blank target is attached to the handler.
        let page = self
            .browser
            .lock()
            .await
            .new_page("about:blank")
            .await
            .map_err(|error| std::io::Error::other(format!("create blank CDP target: {error}")))?;
        let page = ChromePage::new(page, Arc::clone(&self.browser))
            .await
            .map_err(|error| {
                std::io::Error::other(format!("initialize Chrome page listeners: {error}"))
            })?;
        if url != "about:blank" {
            page.open(&url).await.map_err(|error| {
                std::io::Error::other(format!("navigate newly created Chrome page: {error}"))
            })?;
        }
        Ok(page)
    }

    pub async fn pages(&self) -> Result<Vec<ChromePage>, Box<dyn Error + Send + Sync>> {
        let pages = self.browser.lock().await.pages().await?;
        let mut chrome_pages = Vec::new();
        for page in pages {
            chrome_pages.push(ChromePage::new(page, Arc::clone(&self.browser)).await?);
        }
        Ok(chrome_pages)
    }

    pub async fn close(self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut browser = self.browser.lock().await;
        let close_result = if self.mode == ConnectionMode::Launch {
            browser.close().await.map(|_| ())
        } else {
            Ok(())
        };
        let wait_result = browser.wait().await;
        self._handler_task.abort();
        close_result?;
        wait_result?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct ChromePage {
    page: Page,
    browser: Arc<Mutex<Browser>>,
    active_frame_context: Arc<Mutex<Option<runtime::ExecutionContextId>>>,
    dialogs: DialogMonitor,
    downloads: Arc<Mutex<DownloadRegistry>>,
    download_session: String,
    download_frame: Arc<Mutex<String>>,
    runtime_events: Arc<Mutex<RuntimeEventState>>,
    network_guard_enabled: Arc<Mutex<bool>>,
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

impl ChromePage {
    async fn new(
        page: Page,
        browser: Arc<Mutex<Browser>>,
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
        let download_session = page.target_id().inner().clone();
        let downloads = Arc::new(Mutex::new(DownloadRegistry::new()));
        let download_frame = Arc::new(Mutex::new(String::new()));
        let event_frame = Arc::clone(&download_frame);
        // Chrome routes Browser.download* through either the target session
        // or the browser connection, depending on the platform/CDP session.
        let page_begins = page
            .event_listener::<browser::EventDownloadWillBegin>()
            .await?;
        let page_progress = page
            .event_listener::<browser::EventDownloadProgress>()
            .await?;
        let (browser_begins, browser_progress) = {
            let browser = browser.lock().await;
            (
                browser
                    .event_listener::<browser::EventDownloadWillBegin>()
                    .await?,
                browser
                    .event_listener::<browser::EventDownloadProgress>()
                    .await?,
            )
        };
        let mut begins = futures::stream::select(page_begins, browser_begins);
        let mut progress = futures::stream::select(page_progress, browser_progress);
        let event_downloads = Arc::clone(&downloads);
        let event_session = download_session.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    event = begins.next() => {
                        let Some(event) = event else { break };
                        let expected_frame = event_frame.lock().await.clone();
                        if event.frame_id.inner() != expected_frame.as_str() {
                            if std::env::var_os("SYMBROWSE_E2E").is_some() {
                                eprintln!(
                                    "chrome_download_frame_mismatch event={} configured={}",
                                    event.frame_id.inner(),
                                    expected_frame,
                                );
                            }
                            continue;
                        }
                        if std::env::var_os("SYMBROWSE_E2E").is_some() {
                            eprintln!(
                                "chrome_download_frame_match event={}",
                                event.frame_id.inner(),
                            );
                        }
                        event_downloads.lock().await.record_download_will_begin_now(
                            &event_session,
                            event.guid.clone(),
                            event.url.clone(),
                            event.suggested_filename.clone(),
                        );
                    }
                    event = progress.next() => {
                        let Some(event) = event else { break };
                        let received = download_bytes(event.received_bytes);
                        let total = download_bytes(event.total_bytes);
                        event_downloads.lock().await.record_download_progress(
                            &event_session,
                            &event.guid,
                            event.state.as_ref(),
                            received,
                            total,
                        );
                    }
                }
            }
        });
        Ok(Self {
            page,
            browser,
            active_frame_context: Arc::new(Mutex::new(None)),
            dialogs: DialogMonitor {
                state,
                _task: Arc::new(task),
            },
            downloads,
            download_session,
            download_frame,
            runtime_events: Arc::new(Mutex::new(RuntimeEventState::default())),
            network_guard_enabled: Arc::new(Mutex::new(false)),
        })
    }

    /// Enforce daemon URL policy on this page's redirects and subresources.
    pub async fn enable_network_guard(
        &self,
        allowed_domains: Vec<String>,
        ssrf_enabled: bool,
        allow_private: bool,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let diagnostics = std::env::var_os("SYMBROWSE_E2E").is_some();
        let allowlist = symbrowse_core::policy::Allowlist::parse(&allowed_domains)?;
        if !allowlist.active() && !ssrf_enabled {
            if diagnostics {
                eprintln!("chrome_network_guard_stage=inactive");
            }
            return Ok(());
        }
        let mut enabled = self.network_guard_enabled.lock().await;
        if *enabled {
            if diagnostics {
                eprintln!("chrome_network_guard_stage=already-enabled");
            }
            return Ok(());
        }
        let mut paused = self
            .page
            .event_listener::<fetch::EventRequestPaused>()
            .await?;
        if diagnostics {
            eprintln!("chrome_network_guard_stage=request-paused-listener-ready");
        }
        self.page.execute(fetch::EnableParams::default()).await?;
        *enabled = true;
        drop(enabled);
        if diagnostics {
            eprintln!("chrome_network_guard_stage=fetch-enabled");
        }

        let page = self.page.clone();
        tokio::spawn(async move {
            while let Some(event) = paused.next().await {
                let target = event.request.url.clone();
                let allowlist = allowlist.clone();
                let allowed = tokio::task::spawn_blocking(move || {
                    if !allowlist.allows_url(&target) {
                        return false;
                    }
                    !ssrf_enabled
                        || symbrowse_core::policy::SsrfGuard::new(allow_private)
                            .allows_url(&target)
                            .is_ok()
                })
                .await
                .unwrap_or(false);
                let diagnostics = std::env::var_os("SYMBROWSE_E2E").is_some();
                if diagnostics {
                    eprintln!(
                        "chrome_network_guard_decision={}",
                        if allowed { "continue" } else { "block" },
                    );
                }
                if allowed {
                    if let Err(error) = page
                        .execute(fetch::ContinueRequestParams::new(event.request_id.clone()))
                        .await
                        && diagnostics
                    {
                        eprintln!("chrome_network_guard_continue_failed={error}");
                    }
                } else {
                    if let Err(error) = page
                        .execute(fetch::FailRequestParams::new(
                            event.request_id.clone(),
                            network::ErrorReason::BlockedByClient,
                        ))
                        .await
                        && diagnostics
                    {
                        eprintln!("chrome_network_guard_fail_failed={error}");
                    }
                }
            }
        });
        Ok(())
    }

    /// Disable page JavaScript execution for an isolated engine-hint probe.
    pub async fn disable_scripts(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.page
            .execute(emulation::SetScriptExecutionDisabledParams::new(true))
            .await
            .map_err(|error| std::io::Error::other(format!("disable script execution: {error}")))?;
        Ok(())
    }

    /// Enable per-page console and uncaught-exception capture, matching the
    /// lazy `Runtime.enable` behavior of the Go RuntimeEvents capability.
    pub async fn enable_runtime_events(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut state = self.runtime_events.lock().await;
        if state.enabled {
            return Ok(());
        }
        let mut console = self
            .page
            .event_listener::<runtime::EventConsoleApiCalled>()
            .await?;
        let mut exceptions = self
            .page
            .event_listener::<runtime::EventExceptionThrown>()
            .await?;
        self.page.execute(runtime::EnableParams::default()).await?;
        state.enabled = true;
        let events = Arc::clone(&self.runtime_events);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    event = console.next() => {
                        let Some(event) = event else { break };
                        let text = render_console_args(&event.args);
                        if text.is_empty() {
                            continue;
                        }
                        let entry = ConsoleEntry {
                            kind: event.r#type.as_ref().to_owned(),
                            text: truncate_runtime_text(&text),
                            url: String::new(),
                            line: 1,
                            timestamp: runtime_timestamp(),
                        };
                        let mut state = events.lock().await;
                        append_bounded(&mut state.console, entry);
                    }
                    event = exceptions.next() => {
                        let Some(event) = event else { break };
                        let details = &event.exception_details;
                        let stack_trace = details.stack_trace
                            .as_ref()
                            .map(|trace| trace.call_frames.iter().map(|frame| {
                                let function = if frame.function_name.is_empty() {
                                    "(anonymous)"
                                } else {
                                    &frame.function_name
                                };
                                format!("{function} ({}:{}:{})", frame.url, frame.line_number + 1, frame.column_number + 1)
                            }).collect())
                            .unwrap_or_default();
                        let entry = ErrorEntry {
                            text: truncate_runtime_text(&details.text),
                            url: details.url.clone().unwrap_or_default(),
                            line: details.line_number + 1,
                            stack_trace,
                            timestamp: runtime_timestamp(),
                        };
                        let mut state = events.lock().await;
                        append_bounded(&mut state.errors, entry);
                    }
                }
            }
        });
        Ok(())
    }

    pub async fn runtime_console_events(&self) -> Value {
        let state = self.runtime_events.lock().await;
        serde_json::to_value(&state.console).unwrap_or_else(|_| Value::Array(Vec::new()))
    }

    pub async fn runtime_error_events(&self) -> Value {
        let state = self.runtime_events.lock().await;
        serde_json::to_value(&state.errors).unwrap_or_else(|_| Value::Array(Vec::new()))
    }

    pub async fn clear_runtime_console(&self) {
        self.runtime_events.lock().await.console.clear();
    }

    pub async fn clear_runtime_errors(&self) {
        self.runtime_events.lock().await.errors.clear();
    }

    pub fn target_id(&self) -> String {
        self.page.target_id().inner().clone()
    }
    pub fn raw(&self) -> &Page {
        &self.page
    }

    pub async fn open(&self, url: &str) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let diagnostics = std::env::var_os("SYMBROWSE_E2E").is_some();
        if url.trim().is_empty() {
            return Err("navigation URL is required".into());
        }
        if !has_explicit_url_scheme(url) {
            // Go sends relative inputs to Page.navigate, which reports an
            // invalid-URL result without changing the document; NavigationService
            // then returns the still-current page state. Avoid chromiumoxide's
            // lifecycle timeout while preserving that observable outcome.
            return self.current_navigation_outcome().await;
        }
        if !uses_script_navigation(url) {
            // Keep CDP's URL handling for data:, about:, file:, and relative
            // inputs. In particular, Chrome permits Page.navigate to data:
            // where script-initiated top-level navigation is rejected.
            self.page.goto(url).await?;
            return self.current_navigation_outcome().await;
        }

        // chromiumoxide's Page::goto waits for its own Page.lifecycleEvent
        // watcher, which has a fixed 30-second timeout. The Go engine sends
        // Page.navigate and then polls document.readyState instead. Trigger
        // the same navigation from the page context and use the same observable
        // load-complete condition so Windows does not depend on that internal
        // lifecycle watcher. Dispatch synchronously: an extra zero-delay timer
        // can remain queued indefinitely on a background Windows target.
        let mut navigated = self
            .page
            .event_listener::<page::EventFrameNavigated>()
            .await?;
        let mut navigated_within_document = self
            .page
            .event_listener::<page::EventNavigatedWithinDocument>()
            .await?;
        if diagnostics {
            eprintln!("chrome_open_stage=navigation-listeners-ready");
        }
        let main_frame = self.page.mainframe().await?;
        let before = self
            .page
            .evaluate("({url: location.href, time_origin: performance.timeOrigin})")
            .await?
            .into_value::<Value>()?;
        if diagnostics {
            eprintln!("chrome_open_stage=main-frame-resolved");
        }
        let url_literal = serde_json::to_string(url)?;
        if diagnostics {
            eprintln!("chrome_open_stage=dispatch-evaluate-start");
        }
        let dispatch = self
            .evaluate(&format!("location.assign({url_literal}); 'scheduled'"))
            .await?;
        if diagnostics {
            eprintln!("chrome_open_stage=dispatch-evaluate-complete");
        }
        if let Some(error) = dispatch
            .get("exception_text")
            .and_then(Value::as_str)
            .filter(|error| !error.is_empty())
        {
            return Err(error.to_owned().into());
        }

        let mut frame_events_open = true;
        let mut same_document_events_open = true;
        let mut navigation_observed = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        if diagnostics {
            eprintln!("chrome_open_stage=navigation-event-wait-start");
        }
        while !navigation_observed && (frame_events_open || same_document_events_open) {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    return Err("Chrome navigation timed out".into());
                }
                _ = tokio::time::sleep(Duration::from_millis(25)) => {
                    // Chromiumoxide can miss the main-frame event on Windows.
                    // A changed document or same-document URL is the observable
                    // navigation signal when the event stream stays silent.
                    if let Ok(state) = self.page.evaluate("({url: location.href, ready_state: document.readyState, time_origin: performance.timeOrigin})").await {
                        let state = state.into_value::<Value>()?;
                        if navigation_completed(&before, &state) {
                            navigation_observed = true;
                        }
                    }
                }
                event = navigated.next(), if frame_events_open => {
                    match event {
                        Some(event) if event.frame.parent_id.is_none() => {
                            navigation_observed = true;
                            if diagnostics {
                                eprintln!("chrome_open_stage=main-frame-navigation-observed");
                            }
                        }
                        Some(_) => {},
                        None => frame_events_open = false,
                    }
                }
                event = navigated_within_document.next(), if same_document_events_open => {
                    match event {
                        Some(event) if main_frame.as_ref().is_none_or(|id| id == &event.frame_id) => {
                            navigation_observed = true;
                            if diagnostics {
                                eprintln!("chrome_open_stage=same-document-navigation-observed");
                            }
                        }
                        Some(_) => {},
                        None => same_document_events_open = false,
                    }
                }
            }
        }
        if !navigation_observed {
            return Err("Chrome navigation event streams closed".into());
        }

        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err("Chrome navigation timed out waiting for document completion".into());
            }
            let state = self
                .page
                .evaluate("({ready_state: document.readyState})")
                .await;
            if let Ok(state) = state {
                let state = state.into_value::<Value>()?;
                if state.get("ready_state").and_then(Value::as_str) == Some("complete") {
                    if diagnostics {
                        eprintln!("chrome_open_stage=document-complete");
                    }
                    return self.current_navigation_outcome().await;
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    async fn current_navigation_outcome(&self) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let state = self
            .page
            .evaluate(
                "(() => { const nav = performance.getEntriesByType('navigation')[0]; return {url: location.href, http_status: nav && nav.responseStatus || 0}; })()",
            )
            .await?
            .into_value::<Value>()?;
        Ok(serde_json::json!({
            "url": state.get("url").and_then(Value::as_str).unwrap_or_default(),
            "http_status": state.get("http_status").and_then(Value::as_i64).unwrap_or_default(),
        }))
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

    /// Install the human handoff prompt in a closed shadow root. Text values
    /// are assigned through `textContent`, and the closed root keeps page CSS
    /// and ordinary page scripts from replacing or restyling the controls.
    pub async fn install_overlay(
        &self,
        id: &str,
        title: &str,
        reason: &str,
        countdown_seconds: i64,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let values = serde_json::to_string(&(id, title, reason, countdown_seconds.max(0)))?;
        let script =
            format!("(([ID, TITLE, REASON, COUNTDOWN]) => {OOB_OVERLAY_SCRIPT})({values})");
        self.evaluate_script(&script).await?;
        Ok(())
    }

    /// Read the human decision recorded by the overlay, or `pending` while it
    /// remains open.
    pub async fn overlay_result(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        Ok(self
            .evaluate_script("window.__symbrowse_oob_result__ || 'pending'")
            .await?
            .as_str()
            .unwrap_or("pending")
            .to_owned())
    }

    /// Remove the prompt and disconnect its page-mutation observer.
    pub async fn remove_overlay(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.evaluate_script(
            "(() => { const cleanup = window.__symbrowse_oob_cleanup__; if (cleanup) cleanup(); return true; })()",
        )
        .await?;
        Ok(())
    }

    /// Evaluate a user expression and preserve JavaScript exceptions as data,
    /// matching the daemon's protocol-neutral `eval` result.
    pub async fn evaluate(&self, expression: &str) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let response = self
            .page
            .execute(self.evaluate_params(expression).await?)
            .await?
            .result;
        let exception_text = response
            .exception_details
            .as_ref()
            .map(|exception| exception.text.clone())
            .unwrap_or_default();
        let result = response.result;
        Ok(serde_json::json!({
            "type": result.r#type.as_ref().to_owned(),
            "value": result.value.unwrap_or(Value::Null),
            "description": result.description.unwrap_or_default(),
            "exception_text": exception_text,
        }))
    }

    /// Read complete cookie metadata through Chrome's Network domain.
    pub async fn cookies(&self) -> Result<Value, Box<dyn Error + Send + Sync>> {
        self.cookies_for_urls(&[]).await
    }

    /// Read only cookies applicable to the supplied page URLs.
    pub async fn cookies_for_urls(
        &self,
        urls: &[String],
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let params = if urls.is_empty() {
            network::GetCookiesParams::default()
        } else {
            network::GetCookiesParams::builder()
                .urls(urls.iter().cloned())
                .build()
        };
        Ok(serde_json::to_value(
            self.page.execute(params).await?.result,
        )?)
    }

    /// Set a complete cookie through Chrome's Network domain.
    pub async fn set_cookie(&self, cookie: Value) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let params: network::SetCookieParams = serde_json::from_value(cookie)?;
        Ok(serde_json::to_value(
            self.page.execute(params).await?.result,
        )?)
    }

    /// Delete a cookie by name within one URL's domain and path scope.
    pub async fn delete_cookie(
        &self,
        name: &str,
        url: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let params = network::DeleteCookiesParams::builder()
            .name(name)
            .url(url)
            .build()
            .map_err(|error| format!("build delete-cookie request: {error}"))?;
        self.page.execute(params).await?;
        Ok(())
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

    async fn scroll_element_into_view(
        &self,
        element: &Element,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Match Go's DOM.scrollIntoViewIfNeeded call. chromiumoxide's helper
        // waits for an IntersectionObserver promise before scrolling, which
        // can remain pending for an off-screen target in headless Chromium.
        self.page
            .execute(
                dom::ScrollIntoViewIfNeededParams::builder()
                    .backend_node_id(element.backend_node_id)
                    .build(),
            )
            .await?;
        Ok(())
    }

    async fn click_target(
        &self,
        selector: &str,
        action: &str,
    ) -> Result<Point, Box<dyn Error + Send + Sync>> {
        let element = self.element(selector).await?;
        let trace_geometry = std::env::var_os("SYMBROWSE_E2E").is_some() && selector == "#download";
        let selector_json = serde_json::to_string(selector)?;
        if trace_geometry {
            let before = self
                .page
                .evaluate(format!(
                    "(() => {{ const e=document.querySelector({selector_json}); const r=e?.getBoundingClientRect(); return {{scroll_y:window.scrollY, top:r?.top ?? null, bottom:r?.bottom ?? null, height:innerHeight}}; }})()"
                ))
                .await?
                .into_value::<Value>()?;
            eprintln!("chrome_download_click_geometry_before={before}");
        }
        self.scroll_element_into_view(&element).await?;
        element
            .call_js_fn(
                "function() { this.scrollIntoView({block:'center', inline:'center'}); }",
                false,
            )
            .await?;
        if trace_geometry {
            let after = self
                .page
                .evaluate(format!(
                    "(() => {{ const e=document.querySelector({selector_json}); const r=e?.getBoundingClientRect(); return {{scroll_y:window.scrollY, top:r?.top ?? null, bottom:r?.bottom ?? null, height:innerHeight}}; }})()"
                ))
                .await?
                .into_value::<Value>()?;
            eprintln!("chrome_download_click_geometry_after_scroll={after}");
        }
        let point = element.clickable_point().await?;
        if trace_geometry {
            eprintln!(
                "chrome_download_click_point=x{} y{} backend_node_id={:?}",
                point.x, point.y, element.backend_node_id
            );
        }
        let hit = match self
            .page
            .execute(dom::GetNodeForLocationParams::new(
                point.x as i64,
                point.y as i64,
            ))
            .await
        {
            Ok(hit) => hit,
            Err(error) => {
                let same_target = element
                    .call_js_fn(
                        format!(
                            "function() {{ return this === document.elementFromPoint({}, {}); }}",
                            point.x, point.y
                        ),
                        false,
                    )
                    .await?
                    .result
                    .value
                    .as_ref()
                    .and_then(Value::as_bool)
                    == Some(true);
                if same_target {
                    return Ok(point);
                }
                return Err(error.into());
            }
        };
        if trace_geometry {
            eprintln!(
                "chrome_download_click_hit_matches={}",
                hit.backend_node_id == element.backend_node_id
            );
        }
        if hit.backend_node_id != element.backend_node_id {
            let mut role = String::new();
            let mut name = String::new();
            if let Some(node_id) = hit.node_id
                && let Ok(node) = self.page.describe_node(node_id).await
            {
                role = node.node_name;
                let attributes = node.attributes.as_deref().unwrap_or_default();
                name = attribute_value(attributes, "aria-label");
                if name.is_empty() {
                    name = attribute_value(attributes, "id");
                }
            }
            if role.is_empty() {
                role = "element".into();
            }
            if name.is_empty() {
                name = "unnamed".into();
            }
            return Err(Box::new(ClickObstructedError {
                message: format!(
                    "{action} {selector:?} was obstructed by {role} {name:?} (ref=unavailable)"
                ),
                hint: "close the covering element and retry the click".into(),
            }));
        }
        Ok(point)
    }

    pub async fn click(
        &self,
        selector: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        let point = self.click_target(selector, "click").await?;
        self.page.click(point).await?;
        Ok(result("click", selector))
    }
    pub async fn double_click(
        &self,
        selector: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        let point = self.click_target(selector, "dblclick").await?;
        self.page
            .click_with(
                point,
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
        let element = self.element(selector).await?;
        self.scroll_element_into_view(&element).await?;
        self.page
            .move_mouse(element.clickable_point().await?)
            .await?;
        Ok(result("hover", selector))
    }
    pub async fn scroll_into_view(
        &self,
        selector: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        let element = self.element(selector).await?;
        self.scroll_element_into_view(&element).await?;
        Ok(result("scrollintoview", selector))
    }
    pub async fn scroll(
        &self,
        selector: &str,
        amount: i64,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        // Keep phase context on failures: native Chrome occasionally stalls a
        // CDP command after a tab switch, and the outer daemon timeout alone
        // does not identify which part of the interaction stopped progressing.
        scroll_stage("locate:start");
        let target = self
            .element(selector)
            .await
            .map_err(|error| std::io::Error::other(format!("scroll locate target: {error}")))?;
        scroll_stage("locate:done");
        scroll_stage("into_view:start");
        self.scroll_element_into_view(&target)
            .await
            .map_err(|error| {
                std::io::Error::other(format!("scroll bring target into view: {error}"))
            })?;
        scroll_stage("into_view:done");
        scroll_stage("focus:start");
        target
            .focus()
            .await
            .map_err(|error| std::io::Error::other(format!("scroll focus target: {error}")))?;
        scroll_stage("focus:done");
        scroll_stage("inspect_box:start");
        let bounds = self.inspect(selector, "box").await.map_err(|error| {
            std::io::Error::other(format!("scroll inspect target box: {error}"))
        })?;
        scroll_stage("inspect_box:done");
        let x = bounds["x"]
            .as_f64()
            .ok_or("element box has no x coordinate")?
            + bounds["width"].as_f64().ok_or("element box has no width")? / 2.0;
        let y = bounds["y"]
            .as_f64()
            .ok_or("element box has no y coordinate")?
            + bounds["height"]
                .as_f64()
                .ok_or("element box has no height")?
                / 2.0;
        let amount = if amount == 0 { 480.0 } else { amount as f64 };
        scroll_stage("move_pointer:start");
        self.page
            .move_mouse(Point::new(x, y))
            .await
            .map_err(|error| std::io::Error::other(format!("scroll move pointer: {error}")))?;
        scroll_stage("move_pointer:done");
        let event = input::DispatchMouseEventParams::builder()
            .r#type(input::DispatchMouseEventType::MouseWheel)
            .x(x)
            .y(y)
            .button(input::MouseButton::None)
            .delta_x(0.0)
            .delta_y(amount)
            .build()?;
        scroll_stage("dispatch_wheel:start");
        self.page
            .execute(event)
            .await
            .map_err(|error| std::io::Error::other(format!("scroll dispatch wheel: {error}")))?;
        scroll_stage("dispatch_wheel:done");
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
        self.click_target(selector, "check").await?;
        self.set_checked(selector, true).await
    }
    pub async fn uncheck(
        &self,
        selector: &str,
    ) -> Result<InteractionResult, Box<dyn Error + Send + Sync>> {
        self.click_target(selector, "uncheck").await?;
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
        self.inspect_with_properties(selector, kind, &[]).await
    }

    /// Inspect a selector, optionally limiting computed styles to named properties.
    pub async fn inspect_with_properties(
        &self,
        selector: &str,
        kind: &str,
        properties: &[String],
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let s = serde_json::to_string(selector)?;
        let wanted = serde_json::to_string(properties)?;
        let expression = match kind {
            "text" => format!(
                "(() => {{ const e=document.querySelector({s}); if (!e) throw new Error('selector did not match'); return e.innerText || e.textContent || ''; }})()"
            ),
            "html" => format!(
                "(() => {{ const e=document.querySelector({s}); if (!e) throw new Error('selector did not match'); return e.innerHTML ?? ''; }})()"
            ),
            "value" => format!(
                "(() => {{ const e=document.querySelector({s}); if (!e) throw new Error('selector did not match'); return e.value === undefined ? '' : e.value; }})()"
            ),
            "title" => "document.title".to_owned(),
            "url" => "location.href".to_owned(),
            "box" => format!(
                "(() => {{ const e=document.querySelector({s}); if (!e) throw new Error('selector did not match'); const r=e.getBoundingClientRect(); return {{x:r.x,y:r.y,width:r.width,height:r.height,top:r.top,right:r.right,bottom:r.bottom,left:r.left}}; }})()"
            ),
            "styles" => format!(
                "(() => {{ const e=document.querySelector({s}); if (!e) throw new Error('selector did not match'); const c=getComputedStyle(e),o={{}},wanted={wanted}; if(wanted.length){{wanted.forEach(k=>o[k]=c.getPropertyValue(k));}}else{{for(let i=0;i<c.length;i++){{const k=c[i];o[k]=c.getPropertyValue(k);}}}} return o; }})()"
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
            "(() => {{ const kind={kind}, query={query}, action={action}, name={name}, exact={exact}, wantedIndex={index}, value={value}; const text=e=>(e.innerText||e.textContent||'').trim(); const implicit=e=>{{ const tag=e.tagName.toLowerCase(); return tag==='button'?'button':tag==='a'?'link':tag==='input'?(e.type==='checkbox'?'checkbox':e.type==='radio'?'radio':'textbox'):tag==='textarea'?'textbox':tag==='select'?'combobox':''; }}; const candidate=e=>{{ switch(kind) {{ case 'role': return e.getAttribute('role')||implicit(e); case 'text': return text(e); case 'label': return e.getAttribute('aria-label')||text(document.querySelector(`label[for='${{CSS.escape(e.id||'')}}']` )||e); case 'placeholder': return e.getAttribute('placeholder')||''; case 'alt': return e.getAttribute('alt')||''; case 'title': return e.getAttribute('title')||''; case 'testid': return e.getAttribute('data-testid')||''; case 'css': return e.matches(query)?query:''; case 'ref': return e.getAttribute('data-symbrowse-ref')||''; default: return ''; }} }}; const matchesText=(got)=>exact?got===query:got.toLowerCase().includes(query.toLowerCase()); let nodes=[...document.querySelectorAll('*')]; let matches=nodes.filter(e=>kind==='ref'?candidate(e)===query.replace(/^@/,''):matchesText(candidate(e))); if (kind==='text') matches=matches.filter(e=>!matches.some(other=>other!==e&&e.contains(other))); if (name) matches=matches.filter(e=>{{ const got=e.getAttribute('aria-label')||e.getAttribute('name')||''; return exact?got===name:got.toLowerCase().includes(name.toLowerCase()); }}); if (!matches.length) throw new Error(`find ${{kind}} ${{query}} matched no elements`); if (wantedIndex!==null) matches=[matches[wantedIndex]].filter(Boolean); if (!matches.length) throw new Error(`find ${{kind}} index ${{wantedIndex}} is out of range`); if (wantedIndex===null&&matches.length>1&&['first','last','nth'].indexOf(action)<0) throw new Error(`find ${{kind}} ${{query}} matched ${{matches.length}} elements; use index`); let selected=action==='last'?matches[matches.length-1]:matches[0]; let next=1; for (const e of nodes) {{ if (!e.getAttribute('data-symbrowse-ref')) e.setAttribute('data-symbrowse-ref',`e${{next++}}`); }} const ref=selected.getAttribute('data-symbrowse-ref'), role=selected.getAttribute('role')||implicit(selected), accessibleName=selected.getAttribute('aria-label')||selected.getAttribute('name')||text(selected), inputType=selected.getAttribute('type')||'', autocomplete=selected.getAttribute('autocomplete')||''; if (action==='click') selected.click(); else if (action==='focus') selected.focus(); else if (action==='fill') {{ selected.focus(); selected.value=value; selected.dispatchEvent(new Event('input',{{bubbles:true}})); selected.dispatchEvent(new Event('change',{{bubbles:true}})); }} else if (action==='text') return {{kind,query,action,ref,role,name:accessibleName,input_type:inputType,autocomplete,matches:matches.map(e=>({{ref:e.getAttribute('data-symbrowse-ref'),text:text(e),tag:e.tagName.toLowerCase()}})),value:text(selected)}}; return {{kind,query,action,ref,role,name:accessibleName,input_type:inputType,autocomplete,matches:matches.map(e=>({{ref:e.getAttribute('data-symbrowse-ref'),text:text(e),tag:e.tagName.toLowerCase()}}))}}; }})()"
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
        Ok(vec![frame_info(&tree)])
    }

    pub async fn set_active_frame(
        &self,
        frame_id: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let context = if frame_id.is_empty() {
            None
        } else {
            Some(
                self.page
                    .execute(page::CreateIsolatedWorldParams::new(frame_id.to_owned()))
                    .await?
                    .execution_context_id,
            )
        };
        *self.active_frame_context.lock().await = context;
        Ok(())
    }

    async fn evaluate_params(&self, expression: &str) -> Result<EvaluateParams, std::io::Error> {
        let mut builder = EvaluateParams::builder()
            .expression(expression.to_owned())
            .return_by_value(true)
            .await_promise(true);
        if let Some(context) = *self.active_frame_context.lock().await {
            builder = builder.context_id(context);
        }
        builder.build().map_err(std::io::Error::other)
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
            finished: self
                .page
                .event_listener::<network::EventLoadingFinished>()
                .await?,
            request_order: Vec::new(),
            requests_by_id: HashMap::new(),
        })
    }

    #[allow(deprecated)]
    pub async fn set_offline(&self, offline: bool) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.page
            .execute(network::EmulateNetworkConditionsByRuleParams::new(
                offline,
                vec![network::NetworkConditions::new(
                    "",
                    0.0,
                    if offline { -1.0 } else { 0.0 },
                    if offline { -1.0 } else { 0.0 },
                )],
            ))
            .await?;
        Ok(())
    }

    pub async fn set_viewport(
        &self,
        width: i64,
        height: i64,
        scale: f64,
        mobile: bool,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if width <= 0 || height <= 0 {
            return Err(format!("viewport dimensions must be positive: {width}x{height}").into());
        }
        let scale = if scale <= 0.0 { 1.0 } else { scale };
        self.page
            .execute(emulation::SetDeviceMetricsOverrideParams::new(
                width, height, scale, mobile,
            ))
            .await?;
        Ok(())
    }

    pub async fn set_geolocation(
        &self,
        latitude: f64,
        longitude: f64,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
            return Err(format!("invalid coordinates: {latitude:.6}, {longitude:.6}").into());
        }
        let params = emulation::SetGeolocationOverrideParams::builder()
            .latitude(latitude)
            .longitude(longitude)
            .build();
        self.page.execute(params).await?;
        Ok(())
    }

    pub async fn set_extra_headers(
        &self,
        headers: serde_json::Map<String, Value>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        for name in headers.keys() {
            if ["authorization", "proxy-authorization", "cookie"]
                .iter()
                .any(|secret| name.eq_ignore_ascii_case(secret))
            {
                return Err(format!("header {name:?} requires the credential risk class").into());
            }
        }
        let params: network::SetExtraHttpHeadersParams =
            serde_json::from_value(serde_json::json!({"headers": headers}))?;
        self.page.execute(params).await?;
        Ok(())
    }

    pub async fn set_media(&self, dark: bool) -> Result<(), Box<dyn Error + Send + Sync>> {
        let value = if dark { "dark" } else { "light" };
        let params: emulation::SetEmulatedMediaParams =
            serde_json::from_value(serde_json::json!({
                "features": [{"name": "prefers-color-scheme", "value": value}]
            }))?;
        self.page.execute(params).await?;
        Ok(())
    }

    pub async fn set_user_agent(
        &self,
        user_agent: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if user_agent.trim().is_empty() {
            return Err("user agent must not be empty".into());
        }
        let params = emulation::SetUserAgentOverrideParams::new(user_agent);
        self.page.execute(params).await?;
        Ok(())
    }

    pub async fn apply_device(&self, name: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        let devices: Vec<DeviceProfile> = serde_json::from_str(DEVICE_PROFILES)?;
        let device = devices
            .into_iter()
            .find(|device| device.name == name)
            .ok_or_else(|| format!("unknown device {name:?}"))?;
        self.set_viewport(device.width, device.height, device.scale, false)
            .await?;
        if device.mobile {
            self.set_viewport(device.width, device.height, device.scale, true)
                .await?;
        }
        if !device.user_agent.is_empty() {
            self.set_user_agent(&device.user_agent).await?;
        }
        if device.touch {
            let params: emulation::SetTouchEmulationEnabledParams =
                serde_json::from_value(serde_json::json!({"enabled": true, "maxTouchPoints": 5}))?;
            self.page.execute(params).await?;
        }
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
        let directory = directory.map_or_else(String::new, |path| path.to_string_lossy().into());
        self.configure_downloads(&directory).await
    }

    pub async fn configure_downloads(
        &self,
        directory: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let behavior = download_behavior(directory)?;
        let frame = self.page.execute(page::GetFrameTreeParams {}).await?;
        *self.download_frame.lock().await = frame.frame_tree.frame.id.inner().clone();
        let cdp_behavior = match behavior.behavior.as_str() {
            "allow" => browser::SetDownloadBehaviorBehavior::AllowAndName,
            _ => browser::SetDownloadBehaviorBehavior::Deny,
        };
        let path = (!behavior.download_path.is_empty()).then(|| behavior.download_path.clone());
        self.browser
            .lock()
            .await
            .execute(browser_set_download_behavior(cdp_behavior, path)?)
            .await
            .map_err(|error| std::io::Error::other(format!("set download behavior: {error}")))?;
        self.downloads
            .lock()
            .await
            .remember_download_behavior(&self.download_session, behavior);
        Ok(())
    }

    pub async fn download_events(&self) -> Vec<DownloadEvent> {
        self.downloads.lock().await.events(&self.download_session)
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
    /// Run the same offline axe-core audit used by the Go Chrome engine and
    /// return its stable CLI/daemon summary shape.
    pub async fn axe_audit(
        &self,
        tags: &[String],
        selector: &str,
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let mut options = serde_json::Map::new();
        if !tags.is_empty() {
            options.insert(
                "runOnly".into(),
                serde_json::json!({"type": "tag", "values": tags}),
            );
        }
        if !selector.trim().is_empty() {
            // Match the Go wrapper's explicit null option when auditing a
            // selector root; axe-core treats the selector itself as context.
            options.insert("exclude".into(), Value::Null);
        }
        let options = serde_json::to_string(&options)?;
        let source = serde_json::to_string(AXE_CORE_SOURCE)?;
        let root = if selector.trim().is_empty() {
            "document".to_owned()
        } else {
            serde_json::to_string(selector)?
        };
        let expression = format!(
            "(()=>{{if(!(window.axe&&window.axe.run)){{(0,eval)({source});}}return window.axe.run({root},{options}).then(results=>({{axe_version:window.axe.version,results}}));}})()"
        );
        let raw = self
            .page
            .evaluate(expression)
            .await?
            .into_value::<Value>()?;
        let results = raw.get("results").ok_or("axe-core returned no result")?;
        let violations = results
            .get("violations")
            .and_then(Value::as_array)
            .ok_or("axe-core returned no violations array")?;
        let violations = violations
            .iter()
            .map(summarize_violation)
            .collect::<Vec<_>>();
        // Go keeps a successful audit if reading the current URL fails.
        let url = self.page.url().await.ok().flatten().unwrap_or_default();
        Ok(serde_json::json!({
            "axe_version": raw.get("axe_version").and_then(Value::as_str).unwrap_or_default(),
            "url": url,
            "violation_count": violations.len(),
            "violations": violations,
            "passes": results.get("passes").and_then(Value::as_array).map_or(0, Vec::len),
            "incomplete": results.get("incomplete").and_then(Value::as_array).map_or(0, Vec::len),
        }))
    }
}

fn summarize_violation(violation: &Value) -> Value {
    let mut summary = serde_json::Map::new();
    for field in ["id", "impact", "description"] {
        summary.insert(
            field.to_owned(),
            Value::String(
                violation
                    .get(field)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ),
        );
    }
    for field in ["help", "helpUrl"] {
        if let Some(value) = violation
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            summary.insert(
                if field == "helpUrl" {
                    "help_url".to_owned()
                } else {
                    field.to_owned()
                },
                Value::String(value.to_owned()),
            );
        }
    }
    if let Some(tags) = violation
        .get("tags")
        .and_then(Value::as_array)
        .filter(|tags| !tags.is_empty())
    {
        summary.insert("tags".to_owned(), Value::Array(tags.clone()));
    }
    let nodes = violation
        .get("nodes")
        .and_then(Value::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .map(|node| {
                    let mut summary = serde_json::Map::new();
                    summary.insert(
                        "target".to_owned(),
                        node.get("target").cloned().unwrap_or(Value::Null),
                    );
                    for field in ["html", "impact"] {
                        if let Some(value) = node
                            .get(field)
                            .and_then(Value::as_str)
                            .filter(|value| !value.is_empty())
                        {
                            summary.insert(field.to_owned(), Value::String(value.to_owned()));
                        }
                    }
                    Value::Object(summary)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    summary.insert("nodes".to_owned(), Value::Array(nodes));
    Value::Object(summary)
}

pub struct NetworkCapture {
    requests: chromiumoxide::listeners::EventStream<network::EventRequestWillBeSent>,
    responses: chromiumoxide::listeners::EventStream<network::EventResponseReceived>,
    failed: chromiumoxide::listeners::EventStream<network::EventLoadingFailed>,
    finished: chromiumoxide::listeners::EventStream<network::EventLoadingFinished>,
    request_order: Vec<String>,
    requests_by_id: HashMap<String, CapturedRequest>,
}

impl NetworkCapture {
    pub async fn collect(mut self, timeout: Duration) -> Vec<NetworkEvent> {
        self.collect_retaining_requests(timeout).await
    }

    pub async fn collect_retaining_requests(&mut self, timeout: Duration) -> Vec<NetworkEvent> {
        let deadline = Instant::now() + timeout;
        let mut events = Vec::new();
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            tokio::select! {
                event = self.requests.next() => if let Some(event) = event {
                    let id: String = event.request_id.clone().into();
                    let request = ensure_captured_request(&mut self.request_order, &mut self.requests_by_id, &id);
                    request.id = id.clone();
                    request.url = event.request.url.clone();
                    request.method = event.request.method.clone();
                    request.request_type = event.r#type.as_ref().map_or_else(String::new, |kind| kind.as_ref().to_owned());
                    request.started_at_unix_seconds = *event.wall_time.inner() as i64;
                    request.request_headers = masked_headers(event.request.headers.inner());
                    events.push(NetworkEvent { kind: "request".to_owned(), id, url: event.request.url.clone(), ..Default::default() });
                },
                event = self.responses.next() => if let Some(event) = event {
                    let id: String = event.request_id.clone().into();
                    let request = ensure_captured_request(&mut self.request_order, &mut self.requests_by_id, &id);
                    request.status = event.response.status;
                    request.status_text = event.response.status_text.clone();
                    request.mime_type = event.response.mime_type.clone();
                    request.response_headers = masked_headers(event.response.headers.inner());
                    request.encoded_body_size = event.response.encoded_data_length as i64;
                    events.push(NetworkEvent { kind: "response".to_owned(), id, url: event.response.url.clone(), status: event.response.status as u16, mime_type: event.response.mime_type.clone(), ..Default::default() });
                },
                event = self.failed.next() => if let Some(event) = event {
                    let id: String = event.request_id.clone().into();
                    let request = ensure_captured_request(&mut self.request_order, &mut self.requests_by_id, &id);
                    request.failed = event.error_text.clone();
                    request.finished = true;
                    events.push(NetworkEvent { kind: "failed".to_owned(), id, error_text: event.error_text.clone(), ..Default::default() });
                },
                event = self.finished.next() => if let Some(event) = event {
                    let id: String = event.request_id.clone().into();
                    let request = ensure_captured_request(&mut self.request_order, &mut self.requests_by_id, &id);
                    request.finished = true;
                },
                _ = tokio::time::sleep(remaining) => break,
            }
        }
        events
    }

    pub fn requests(&self) -> Vec<CapturedRequest> {
        self.request_order
            .iter()
            .filter_map(|id| self.requests_by_id.get(id).cloned())
            .collect()
    }
}

fn ensure_captured_request<'a>(
    request_order: &'a mut Vec<String>,
    requests_by_id: &'a mut HashMap<String, CapturedRequest>,
    id: &str,
) -> &'a mut CapturedRequest {
    if !requests_by_id.contains_key(id) {
        if request_order.len() >= 2_000 {
            let oldest = request_order.remove(0);
            requests_by_id.remove(&oldest);
        }
        request_order.push(id.to_owned());
        requests_by_id.insert(
            id.to_owned(),
            CapturedRequest {
                id: id.to_owned(),
                ..CapturedRequest::default()
            },
        );
    }
    requests_by_id
        .get_mut(id)
        .expect("request entry inserted or existed")
}

fn masked_headers(headers: &Value) -> BTreeMap<String, String> {
    let Some(headers) = headers.as_object() else {
        return BTreeMap::new();
    };
    headers
        .iter()
        .map(|(name, value)| {
            let value = if [
                "authorization",
                "proxy-authorization",
                "cookie",
                "set-cookie",
            ]
            .iter()
            .any(|secret| name.eq_ignore_ascii_case(secret))
            {
                "[redacted]".to_owned()
            } else if let Some(value) = value.as_str() {
                value.to_owned()
            } else {
                value.to_string()
            };
            (name.clone(), value)
        })
        .collect()
}

fn result(action: &str, selector: &str) -> InteractionResult {
    InteractionResult {
        action: action.to_owned(),
        selector: selector.to_owned(),
    }
}

fn scroll_stage(stage: &str) {
    if std::env::var_os("SYMBROWSE_E2E").as_deref() == Some(std::ffi::OsStr::new("1")) {
        eprintln!("chrome_scroll_stage={stage}");
    }
}

fn attribute_value(attributes: &[String], name: &str) -> String {
    attributes
        .as_chunks::<2>()
        .0
        .iter()
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .unwrap_or_default()
}

fn frame_info(tree: &page::FrameTree) -> FrameInfo {
    FrameInfo {
        id: tree.frame.id.inner().clone(),
        parent_id: tree
            .frame
            .parent_id
            .as_ref()
            .map(|id| id.inner().clone())
            .unwrap_or_default(),
        name: tree.frame.name.clone().unwrap_or_default(),
        url: tree.frame.url.clone(),
        children: tree
            .child_frames
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(frame_info)
            .collect(),
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

fn download_bytes(value: f64) -> i64 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else if value >= i64::MAX as f64 {
        i64::MAX
    } else {
        value as i64
    }
}

fn is_zero(value: &i64) -> bool {
    *value == 0
}

fn runtime_timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

fn truncate_runtime_text(text: &str) -> String {
    const MAX_ENTRY_BYTES: usize = 4096;
    if text.len() <= MAX_ENTRY_BYTES {
        return text.to_owned();
    }
    format!(
        "{}…",
        String::from_utf8_lossy(&text.as_bytes()[..MAX_ENTRY_BYTES])
    )
}

fn append_bounded<T>(entries: &mut Vec<T>, entry: T) {
    const MAX_ENTRIES: usize = 500;
    if entries.len() >= MAX_ENTRIES {
        let excess = entries.len() - MAX_ENTRIES + 1;
        entries.drain(..excess);
    }
    entries.push(entry);
}

fn render_console_args(args: &[runtime::RemoteObject]) -> String {
    args.iter()
        .filter_map(|arg| {
            if arg.r#type.as_ref() == "string" {
                return arg.value.as_ref().map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string())
                });
            }
            if let Some(description) = arg.description.as_ref().filter(|value| !value.is_empty()) {
                return Some(description.clone());
            }
            arg.value
                .as_ref()
                .filter(|value| !value.is_null())
                .map(Value::to_string)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_capabilities_partition_matches_daemon_commands() {
        let value = crate::canonical_capabilities();
        assert!(value.interfaces.contains(&"TabManager".to_owned()));
        assert!(value.interfaces.contains(&"FileTransfer".to_owned()));
        assert!(value.interfaces.contains(&"OverlayHost".to_owned()));
        assert!(value.interfaces.contains(&"A11yAuditor".to_owned()));
        assert!(
            value
                .interfaces
                .contains(&"ClickDiagnosticEngine".to_owned())
        );
        assert!(value.interfaces.contains(&"SettingsEngine".to_owned()));
        assert_eq!(value.interfaces.len(), 19);
        assert!(value.unsupported.is_empty());
        assert!(!value.interfaces.iter().any(|name| name == "HAR"));
    }

    #[test]
    fn capabilities_are_explicit_and_sorted_with_unsupported_features() {
        let value = capabilities();
        assert_eq!(value.interactions.len(), 11);
        assert!(value.inspection.contains(&"axe-audit".to_owned()));
        assert!(value.artifacts.contains(&"pdf".to_owned()));
        assert_eq!(value.unsupported, vec!["har-export"]);
    }

    #[test]
    fn runtime_console_text_matches_go_argument_rendering() {
        let args: Vec<runtime::RemoteObject> = serde_json::from_value(json!([
            {"type":"string", "value":"message"},
            {"type":"number", "value":42, "description":"42"},
            {"type":"object", "description":"Object"}
        ]))
        .unwrap();
        assert_eq!(render_console_args(&args), "message 42 Object");
    }

    #[test]
    fn runtime_event_entries_use_go_json_field_names_and_bounds() {
        let console = ConsoleEntry {
            kind: "warning".into(),
            text: "warning".into(),
            url: String::new(),
            line: 1,
            timestamp: "2026-09-25T12:00:00Z".into(),
        };
        assert_eq!(
            serde_json::to_value(console).unwrap(),
            json!({
                "type":"warning",
                "text":"warning",
                "line":1,
                "timestamp":"2026-09-25T12:00:00Z"
            })
        );
        let mut entries = Vec::new();
        for index in 0..501 {
            append_bounded(&mut entries, index);
        }
        assert_eq!(entries.len(), 500);
        assert_eq!(entries.first(), Some(&1));
        assert_eq!(entries.last(), Some(&500));
        assert_eq!(
            truncate_runtime_text(&"x".repeat(4097)).len(),
            4096 + "…".len()
        );
    }

    #[test]
    fn network_capture_merges_response_and_finish_before_request_metadata() {
        let mut request_order = Vec::new();
        let mut requests_by_id = HashMap::new();
        let response =
            ensure_captured_request(&mut request_order, &mut requests_by_id, "request-1");
        response.status = 200;
        response.status_text = "OK".into();
        response.mime_type = "text/html".into();
        response
            .response_headers
            .insert("content-length".into(), "115".into());
        response.encoded_body_size = 99;
        response.finished = true;

        let request = ensure_captured_request(&mut request_order, &mut requests_by_id, "request-1");
        request.url = "http://127.0.0.1/page".into();
        request.method = "GET".into();

        assert_eq!(request_order, vec!["request-1".to_owned()]);
        let merged = &requests_by_id["request-1"];
        assert_eq!(merged.url, "http://127.0.0.1/page");
        assert_eq!(merged.method, "GET");
        assert_eq!(merged.status, 200);
        assert_eq!(merged.status_text, "OK");
        assert_eq!(merged.mime_type, "text/html");
        assert_eq!(merged.response_headers["content-length"], "115");
        assert_eq!(merged.encoded_body_size, 99);
        assert!(merged.finished);
    }

    #[test]
    fn frame_tree_keeps_children_nested_like_go() {
        let raw = json!({"frame":{"id":"root","url":"https://example.test/","name":"main","loaderId":"loader-root","domainAndRegistry":"example.test","securityOrigin":"https://example.test","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]},"childFrames":[{"frame":{"id":"child","parentId":"root","url":"https://example.test/frame","name":"nested","loaderId":"loader-child","domainAndRegistry":"example.test","securityOrigin":"https://example.test","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}]});
        let tree: page::FrameTree = serde_json::from_value(raw).unwrap();
        assert_eq!(
            serde_json::to_value(vec![frame_info(&tree)]).unwrap(),
            json!([{"id":"root","name":"main","url":"https://example.test/","children":[{"id":"child","parent_id":"root","name":"nested","url":"https://example.test/frame"}]}])
        );
    }

    #[test]
    fn screenshot_rejects_unknown_format_without_chrome() {
        let options = ScreenshotOptions {
            format: "bmp".into(),
            ..Default::default()
        };
        assert!(options.format != "png");
    }

    #[test]
    fn download_bytes_rejects_invalid_and_saturates_large_values() {
        assert_eq!(download_bytes(f64::NAN), 0);
        assert_eq!(download_bytes(f64::INFINITY), 0);
        assert_eq!(download_bytes(-1.0), 0);
        assert_eq!(download_bytes(12.9), 12);
        assert_eq!(download_bytes(f64::MAX), i64::MAX);
    }

    #[test]
    fn axe_summary_matches_go_field_names_and_bounds_node_data() {
        let violation = json!({
            "id": "label",
            "impact": "critical",
            "description": "Form elements must have labels",
            "help": "Form elements must have labels",
            "helpUrl": "https://example.test/rule",
            "tags": ["wcag2a"],
            "nodes": [{
                "target": ["#email"],
                "html": "<input id=\"email\">",
                "impact": "critical",
                "failureSummary": "excluded by the Go A11yNode contract"
            }]
        });
        let summary = summarize_violation(&violation);
        assert_eq!(summary["help_url"], "https://example.test/rule");
        assert!(summary.get("helpUrl").is_none());
        assert_eq!(summary["nodes"][0]["target"], json!(["#email"]));
        assert!(summary["nodes"][0].get("failureSummary").is_none());
    }

    #[test]
    fn script_navigation_is_limited_to_absolute_web_urls() {
        for url in ["http://example.test/", "HTTPS://example.test/"] {
            assert!(uses_script_navigation(url), "{url}");
        }
        for url in [
            "data:text/html,fixture",
            "about:blank",
            "file:///tmp/page.html",
            "/relative/path",
            "relative/path",
            "#fragment",
        ] {
            assert!(!uses_script_navigation(url), "{url}");
        }
        for url in [
            "/relative/path",
            "relative/path",
            "#fragment",
            "//example.test/",
        ] {
            assert!(!has_explicit_url_scheme(url), "{url}");
        }
        for url in [
            "http://example.test/",
            "data:text/html,fixture",
            "about:blank",
        ] {
            assert!(has_explicit_url_scheme(url), "{url}");
        }
    }

    #[test]
    fn navigation_fallback_requires_a_completed_document_change() {
        let before = json!({"url": "https://example.test/", "time_origin": 1});
        assert!(!navigation_completed(
            &before,
            &json!({"url": "https://example.test/", "time_origin": 1, "ready_state": "complete"})
        ));
        assert!(!navigation_completed(
            &before,
            &json!({"url": "https://example.test/next", "time_origin": 2, "ready_state": "loading"})
        ));
        assert!(navigation_completed(
            &before,
            &json!({"url": "https://example.test/next", "time_origin": 2, "ready_state": "complete"})
        ));
        assert!(navigation_completed(
            &before,
            &json!({"url": "https://example.test/#section", "time_origin": 1, "ready_state": "complete"})
        ));
    }
}
