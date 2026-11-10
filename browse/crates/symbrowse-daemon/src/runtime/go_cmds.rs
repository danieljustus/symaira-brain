//! Go-parity Chrome commands: eval, cookies, storage, emulation, runtime
//! events and frames.  Each handler mirrors the Go engine's CDP sequence and
//! response shape (`internal/daemon/*_frames.go`, `internal/engine/**`).

use super::*;
use symbrowse_engine_chrome::{ConsoleEntry, ErrorEntry};

/// `Page.navigate` `errorText` that must fail the command; `net::ERR_ABORTED`
/// is the one value Go's polling contract treats as success.
fn navigation_error_text(result: &Value) -> Option<&str> {
    result["errorText"]
        .as_str()
        .filter(|text| !text.is_empty() && *text != "net::ERR_ABORTED")
}

fn failed(message: impl Into<String>) -> DaemonError {
    DaemonError {
        code: codes::OPERATION_FAILED.into(),
        message: redact_str(&message.into()),
        ..Default::default()
    }
}

/// Go `decodeArgs`: absent/null args are an error; the payload is decoded
/// into `T` with the Go-style `decode <cmd> arguments:` prefix on failure.
fn decode<T: serde::de::DeserializeOwned>(frame: &Frame) -> Result<T, DaemonError> {
    match frame.args.as_ref() {
        None | Some(Value::Null) => Err(failed("command arguments are required")),
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|error| failed(format!("decode {} arguments: {error}", frame.cmd))),
    }
}

/// Go `decodeOptionalArgs`: decode errors fall back to the zero value.
fn decode_optional<T: serde::de::DeserializeOwned + Default>(frame: &Frame) -> T {
    frame
        .args
        .as_ref()
        .filter(|value| !value.is_null())
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default()
}

/// Go encodes whole `float64` values without a fraction (`-1`, `52`).
fn go_number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        json!(value as i64)
    } else {
        json!(value)
    }
}

fn rfc3339_nanos(unix_nanos: i128) -> String {
    time::OffsetDateTime::from_unix_timestamp_nanos(unix_nanos)
        .ok()
        .and_then(|time| {
            time.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_default()
}

fn console_json(entry: &ConsoleEntry) -> Value {
    let mut value = json!({
        "type": entry.kind,
        "text": entry.text,
        "timestamp": rfc3339_nanos(entry.unix_nanos),
    });
    if !entry.url.is_empty() {
        value["url"] = json!(entry.url);
    }
    if entry.line != 0 {
        value["line"] = json!(entry.line);
    }
    value
}

fn error_json(entry: &ErrorEntry) -> Value {
    let mut value = json!({
        "text": entry.text,
        "timestamp": rfc3339_nanos(entry.unix_nanos),
    });
    if !entry.url.is_empty() {
        value["url"] = json!(entry.url);
    }
    if entry.line != 0 {
        value["line"] = json!(entry.line);
    }
    if !entry.stacktrace.is_empty() {
        value["stacktrace"] = json!(entry.stacktrace);
    }
    value
}

#[derive(serde::Deserialize)]
struct Device {
    name: String,
    width: i64,
    height: i64,
    scale: f64,
    mobile: bool,
    touch: bool,
    #[serde(default)]
    user_agent: String,
}

fn device_by_name(name: &str) -> Result<Device, DaemonError> {
    let devices: Vec<Device> = serde_json::from_str(include_str!("../devices.json"))
        .map_err(|error| failed(format!("decode device list: {error}")))?;
    devices
        .into_iter()
        .find(|device| device.name == name)
        .ok_or_else(|| failed(format!("unknown device {name:?}")))
}

fn storage_object(kind: &str) -> Result<&'static str, DaemonError> {
    match kind {
        "local" => Ok("localStorage"),
        "session" => Ok("sessionStorage"),
        other => Err(failed(format!("invalid storage kind {other:?}"))),
    }
}

fn cdp_failure(error: impl std::fmt::Display) -> DaemonError {
    failed(error.to_string())
}

/// Commands served with Go-equal shapes on the Chrome engine.
pub(super) const GO_CHROME_COMMANDS: &[&str] = &[
    "eval",
    "cookies.list",
    "cookies.set",
    "cookies.clear",
    "storage.list",
    "storage.set",
    "storage.clear",
    "set.viewport",
    "set.device",
    "set.geo",
    "set.offline",
    "set.headers",
    "set.media",
    "set.user-agent",
    "console.list",
    "console.clear",
    "errors.list",
    "errors.clear",
    "frame.tree",
    "frame.main",
    "frame.select",
    "open",
    "goto",
    "network.requests",
    "network.request",
    "network.route",
    "network.unroute",
    "network.har",
    "download.setdir",
    "downloads.list",
    "scroll",
    "trace.replay",
];

const NAVIGATION_STATE: &str = r#"(function(){const nav=performance.getEntriesByType("navigation")[0]; return {url:location.href,http_status:nav && nav.responseStatus || 0,ready_state:document.readyState,network_idle:document.readyState === "complete"};})()"#;

fn network_json(entry: &symbrowse_engine_chrome::NetworkRequestEntry) -> Value {
    let started = time::OffsetDateTime::from_unix_timestamp(entry.started_unix_secs)
        .ok()
        .and_then(|time| {
            time.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_default();
    let mut value = json!({
        "id": entry.id,
        "url": entry.url,
        "method": entry.method,
        "type": entry.kind,
        "status": entry.status,
        "started_at": started,
        "finished": entry.finished,
    });
    let object = value.as_object_mut().expect("object");
    if !entry.status_text.is_empty() {
        object.insert("status_text".into(), json!(entry.status_text));
    }
    if !entry.mime_type.is_empty() {
        object.insert("mime_type".into(), json!(entry.mime_type));
    }
    if !entry.failed.is_empty() {
        object.insert("failed".into(), json!(entry.failed));
    }
    if !entry.request_headers.is_empty() {
        object.insert("request_headers".into(), json!(entry.request_headers));
    }
    if !entry.response_headers.is_empty() {
        object.insert("response_headers".into(), json!(entry.response_headers));
    }
    if entry.encoded_body_size != 0 {
        object.insert("encoded_body_size".into(), json!(entry.encoded_body_size));
    }
    value
}

fn har_headers(headers: &std::collections::BTreeMap<String, String>) -> Value {
    json!(
        headers
            .iter()
            .map(|(name, value)| json!({"name": name, "value": value}))
            .collect::<Vec<_>>()
    )
}

fn har_document(requests: &[symbrowse_engine_chrome::NetworkRequestEntry]) -> Value {
    let entries: Vec<Value> = requests
        .iter()
        .map(|request| {
            let started = time::OffsetDateTime::from_unix_timestamp(request.started_unix_secs)
                .ok()
                .and_then(|time| {
                    time.format(&time::format_description::well_known::Rfc3339)
                        .ok()
                })
                .unwrap_or_default();
            json!({
                "startedDateTime": started,
                "time": 0,
                "request": {
                    "method": request.method,
                    "url": request.url,
                    "httpVersion": "HTTP/1.1",
                    "headers": har_headers(&request.request_headers),
                    "queryString": Value::Null,
                    "cookies": Value::Null,
                    "headersSize": -1,
                    "bodySize": -1,
                },
                "response": {
                    "status": request.status,
                    "statusText": request.status_text,
                    "httpVersion": "HTTP/1.1",
                    "headers": har_headers(&request.response_headers),
                    "cookies": Value::Null,
                    "content": {"size": request.encoded_body_size, "mimeType": request.mime_type},
                    "redirectURL": "",
                    "headersSize": -1,
                    "bodySize": request.encoded_body_size,
                },
                "cache": {},
                "timings": {"send": 0, "wait": 0, "receive": 0},
            })
        })
        .collect();
    json!({"log": {
        "version": "1.2",
        "creator": {"name": "symbrowse", "version": "0.1.0"},
        "pages": [{
            "startedDateTime": super::gate::now_rfc3339(),
            "id": "page_1",
            "title": "",
            "pageTimings": {},
        }],
        "entries": entries,
    }})
}

impl DispatchRuntime {
    fn live_page(&self) -> Option<ChromePage> {
        self.browser
            .lock()
            .ok()
            .and_then(|browser| browser.as_ref().map(|state| state.page.clone()))
    }

    /// Every open tab (the owned browser keeps one page per tab).
    fn live_pages(&self) -> Vec<ChromePage> {
        self.browser
            .lock()
            .ok()
            .and_then(|browser| {
                browser.as_ref().map(|state| {
                    let mut pages: Vec<ChromePage> =
                        state.tabs.iter().map(|tab| tab.page.clone()).collect();
                    pages.push(state.page.clone());
                    pages
                })
            })
            .unwrap_or_default()
    }

    /// Download events across tabs, deduplicated by GUID. Browser events are
    /// delivered to the session that enabled them, which need not be the
    /// active tab.
    fn download_events(&self) -> Vec<symbrowse_engine::files::DownloadEvent> {
        let mut seen = std::collections::BTreeSet::new();
        let mut events = Vec::new();
        for page in self.live_pages() {
            for event in page.download_events() {
                if seen.insert(event.guid.clone()) {
                    events.push(event);
                }
            }
        }
        events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        events
    }

    async fn go_origin(page: &ChromePage) -> String {
        match page.evaluate_expression("location.origin").await {
            Ok(result) if result["exception_text"] == json!("") => {
                result["value"].as_str().unwrap_or_default().to_owned()
            }
            _ => String::new(),
        }
    }

    pub(super) async fn go_chrome_command(
        &self,
        frame: &Frame,
        operation: &OperationContext,
    ) -> HandlerResult {
        // Network inspection is readable without launching a browser.
        if matches!(frame.cmd.as_str(), "network.requests" | "network.request")
            && self.live_page().is_none()
        {
            return Ok((Some(json!({"requests": [], "count": 0})), Vec::new()));
        }
        // Console/error buffers are readable without launching a browser.
        if matches!(
            frame.cmd.as_str(),
            "console.list" | "console.clear" | "errors.list" | "errors.clear"
        ) {
            let data = match self.live_page() {
                None => {
                    if frame.cmd.ends_with(".list") {
                        json!({"entries": [], "count": 0})
                    } else {
                        json!({"cleared": true})
                    }
                }
                Some(page) => match frame.cmd.as_str() {
                    "console.list" => {
                        let entries = page.console_entries();
                        json!({
                            "entries": if entries.is_empty() { Value::Null } else { json!(entries.iter().map(console_json).collect::<Vec<_>>()) },
                            "count": entries.len(),
                        })
                    }
                    "errors.list" => {
                        let entries = page.error_entries();
                        json!({
                            "entries": if entries.is_empty() { Value::Null } else { json!(entries.iter().map(error_json).collect::<Vec<_>>()) },
                            "count": entries.len(),
                        })
                    }
                    "console.clear" => {
                        page.clear_console();
                        json!({"cleared": true})
                    }
                    _ => {
                        page.clear_errors();
                        json!({"cleared": true})
                    }
                },
            };
            return Ok((Some(data), Vec::new()));
        }
        // Arguments are decoded before the browser is launched, like Go.
        let data = match frame.cmd.as_str() {
            "eval" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    expression: String,
                }
                let request: Request = decode(frame)?;
                if request.expression.trim().is_empty() {
                    return Err(failed("eval requires a non-empty expression"));
                }
                let page = self.ensure_browser(operation).await?;
                page.evaluate_expression(&request.expression)
                    .await
                    .map_err(cdp_failure)?
            }
            "cookies.list" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    urls: Vec<String>,
                }
                let request: Request = decode_optional(frame);
                let page = self.ensure_browser(operation).await?;
                let params = if request.urls.is_empty() {
                    json!({})
                } else {
                    json!({"urls": request.urls})
                };
                let result = page
                    .cdp("Network.getCookies", params)
                    .await
                    .map_err(|error| failed(format!("get cookies: {error}")))?;
                let cookies: Vec<Value> = result["cookies"]
                    .as_array()
                    .map(|cookies| {
                        cookies
                            .iter()
                            .map(|cookie| {
                                let mut out = json!({
                                    "name": cookie["name"],
                                    "value": cookie["value"],
                                    "domain": cookie["domain"],
                                    "path": cookie["path"],
                                    "expires": go_number(cookie["expires"].as_f64().unwrap_or(0.0)),
                                    "size": cookie["size"],
                                    "http_only": cookie["httpOnly"].as_bool().unwrap_or(false),
                                    "secure": cookie["secure"].as_bool().unwrap_or(false),
                                    "session": cookie["session"].as_bool().unwrap_or(false),
                                });
                                if let Some(same_site) =
                                    cookie["sameSite"].as_str().filter(|s| !s.is_empty())
                                {
                                    out["same_site"] = json!(same_site);
                                }
                                out
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                json!({"origin": Self::go_origin(&page).await, "cookies": cookies})
            }
            "cookies.set" => {
                #[derive(serde::Deserialize, Default)]
                struct Cookie {
                    #[serde(default)]
                    name: String,
                    #[serde(default)]
                    value: String,
                    #[serde(default)]
                    domain: String,
                    #[serde(default)]
                    path: String,
                    #[serde(default)]
                    expires: f64,
                    #[serde(default)]
                    http_only: bool,
                    #[serde(default)]
                    secure: bool,
                    #[serde(default)]
                    same_site: String,
                }
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    cookie: Cookie,
                    #[serde(default)]
                    url: String,
                }
                let request: Request = decode(frame)?;
                let cookie = request.cookie;
                if cookie.name.is_empty() {
                    return Err(failed("cookie name is required"));
                }
                let page = self.ensure_browser(operation).await?;
                let mut param = serde_json::Map::new();
                param.insert("name".into(), json!(cookie.name));
                param.insert("value".into(), json!(cookie.value));
                if !request.url.is_empty() {
                    param.insert("url".into(), json!(request.url));
                }
                if !cookie.domain.is_empty() {
                    param.insert("domain".into(), json!(cookie.domain));
                }
                if !cookie.path.is_empty() {
                    param.insert("path".into(), json!(cookie.path));
                }
                if cookie.http_only {
                    param.insert("httpOnly".into(), json!(true));
                }
                if cookie.secure {
                    param.insert("secure".into(), json!(true));
                }
                if !cookie.same_site.is_empty() {
                    param.insert("sameSite".into(), json!(cookie.same_site));
                }
                if cookie.expires > 0.0 {
                    param.insert("expires".into(), json!(cookie.expires.trunc()));
                }
                page.cdp(
                    "Network.setCookies",
                    json!({"cookies": [Value::Object(param)]}),
                )
                .await
                .map_err(|error| failed(format!("set cookie {:?}: {error}", cookie.name)))?;
                json!({"set": cookie.name})
            }
            "cookies.clear" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    name: String,
                    #[serde(default)]
                    url: String,
                }
                let request: Request = decode(frame)?;
                if request.name.is_empty() {
                    return Err(failed("cookie name is required"));
                }
                let page = self.ensure_browser(operation).await?;
                let mut params = json!({"name": request.name});
                if !request.url.is_empty() {
                    params["url"] = json!(request.url);
                }
                page.cdp("Network.deleteCookies", params)
                    .await
                    .map_err(|error| {
                        failed(format!("delete cookie {:?}: {error}", request.name))
                    })?;
                json!({"cleared": request.name})
            }
            "storage.list" | "storage.set" | "storage.clear" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    kind: String,
                    #[serde(default)]
                    key: String,
                    #[serde(default)]
                    value: String,
                }
                let request: Request = decode(frame)?;
                let object = storage_object(&request.kind)?;
                let page = self.ensure_browser(operation).await?;
                let kind = request.kind.clone();
                match frame.cmd.as_str() {
                    "storage.list" => {
                        let expression = format!(
                            "(function(){{\n\t\tconst s = window.{object};\n\t\tconst out = {{}};\n\t\tfor (let i = 0; i < s.length; i++) {{ const k = s.key(i); out[k] = s.getItem(k); }}\n\t\treturn {{origin: location.origin, items: out}};\n\t}})()"
                        );
                        let result = page
                            .evaluate_expression(&expression)
                            .await
                            .map_err(|error| failed(format!("read {kind} storage: {error}")))?;
                        if let Some(text) =
                            result["exception_text"].as_str().filter(|t| !t.is_empty())
                        {
                            return Err(failed(text));
                        }
                        let items = result["value"]["items"].clone();
                        json!({
                            "origin": Self::go_origin(&page).await,
                            "kind": kind,
                            "items": if items.is_object() { items } else { json!({}) },
                        })
                    }
                    "storage.set" => {
                        if request.key.trim().is_empty() {
                            return Err(failed("storage key is required"));
                        }
                        let expression = format!(
                            "(function(){{ const s = window.{object}; s.setItem({}, {}); return true; }})()",
                            serde_json::to_string(&request.key).map_err(cdp_failure)?,
                            serde_json::to_string(&request.value).map_err(cdp_failure)?
                        );
                        let result =
                            page.evaluate_expression(&expression)
                                .await
                                .map_err(|error| {
                                    failed(format!("set {kind} storage {:?}: {error}", request.key))
                                })?;
                        if let Some(text) =
                            result["exception_text"].as_str().filter(|t| !t.is_empty())
                        {
                            return Err(failed(format!(
                                "set {kind} storage {:?}: {text}",
                                request.key
                            )));
                        }
                        json!({"set": request.key})
                    }
                    _ => {
                        let expression =
                            format!("(function(){{ window.{object}.clear(); return true; }})()");
                        let result = page
                            .evaluate_expression(&expression)
                            .await
                            .map_err(|error| failed(format!("clear {kind} storage: {error}")))?;
                        if let Some(text) =
                            result["exception_text"].as_str().filter(|t| !t.is_empty())
                        {
                            return Err(failed(format!("clear {kind} storage: {text}")));
                        }
                        json!({"cleared": kind})
                    }
                }
            }
            "set.viewport" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    width: i64,
                    #[serde(default)]
                    height: i64,
                    #[serde(default)]
                    scale: f64,
                }
                let request: Request = decode(frame)?;
                let page = self.ensure_browser(operation).await?;
                Self::apply_viewport(&page, request.width, request.height, request.scale).await?;
                json!({"viewport": [request.width, request.height]})
            }
            "set.device" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    name: String,
                }
                let request: Request = decode(frame)?;
                let device = device_by_name(&request.name)?;
                let page = self.ensure_browser(operation).await?;
                Self::apply_viewport(&page, device.width, device.height, device.scale).await?;
                if device.mobile {
                    page.cdp(
                        "Emulation.setDeviceMetricsOverride",
                        json!({"width": device.width, "height": device.height, "deviceScaleFactor": device.scale, "mobile": true}),
                    )
                    .await
                    .map_err(|error| failed(format!("set mobile metrics: {error}")))?;
                }
                if !device.user_agent.is_empty() {
                    page.cdp(
                        "Emulation.setUserAgentOverride",
                        json!({"userAgent": device.user_agent}),
                    )
                    .await
                    .map_err(|error| failed(format!("set user agent: {error}")))?;
                }
                if device.touch {
                    page.cdp(
                        "Emulation.setTouchEmulationEnabled",
                        json!({"enabled": true, "maxTouchPoints": 5}),
                    )
                    .await
                    .map_err(|error| failed(format!("set touch emulation: {error}")))?;
                }
                json!({"device": device.name})
            }
            "set.geo" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    latitude: f64,
                    #[serde(default)]
                    longitude: f64,
                }
                let request: Request = decode(frame)?;
                let (lat, lon) = (request.latitude, request.longitude);
                if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                    return Err(failed(format!("invalid coordinates: {lat:.6}, {lon:.6}")));
                }
                let page = self.ensure_browser(operation).await?;
                page.cdp(
                    "Emulation.setGeolocationOverride",
                    json!({"latitude": lat, "longitude": lon}),
                )
                .await
                .map_err(|error| failed(format!("set geolocation: {error}")))?;
                json!({"geo": [go_number(lat), go_number(lon)]})
            }
            "set.offline" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    offline: bool,
                }
                let request: Request = decode_optional(frame);
                let page = self.ensure_browser(operation).await?;
                let throughput = if request.offline { -1 } else { 0 };
                page.cdp(
                    "Network.emulateNetworkConditionsByRule",
                    json!({"matchedNetworkConditions": [{
                        "urlPattern": "",
                        "latency": 0,
                        "downloadThroughput": throughput,
                        "uploadThroughput": throughput,
                    }]}),
                )
                .await
                .map_err(|error| failed(format!("set offline={}: {error}", request.offline)))?;
                json!({"offline": request.offline})
            }
            "set.headers" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    headers: std::collections::BTreeMap<String, String>,
                }
                let request: Request = decode(frame)?;
                for name in request.headers.keys() {
                    if ["authorization", "proxy-authorization", "cookie"]
                        .iter()
                        .any(|blocked| name.eq_ignore_ascii_case(blocked))
                    {
                        return Err(failed(format!(
                            "header {name:?} requires the credential risk class"
                        )));
                    }
                }
                let page = self.ensure_browser(operation).await?;
                page.cdp(
                    "Network.setExtraHTTPHeaders",
                    json!({"headers": request.headers}),
                )
                .await
                .map_err(|error| failed(format!("set extra headers: {error}")))?;
                json!({"headers_set": request.headers.len()})
            }
            "set.media" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    dark: bool,
                }
                let request: Request = decode(frame)?;
                let page = self.ensure_browser(operation).await?;
                let value = if request.dark { "dark" } else { "light" };
                page.cdp(
                    "Emulation.setEmulatedMedia",
                    json!({"features": [{"name": "prefers-color-scheme", "value": value}]}),
                )
                .await
                .map_err(|error| failed(format!("set media scheme: {error}")))?;
                json!({"media": value})
            }
            "set.user-agent" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    user_agent: String,
                }
                let request: Request = decode(frame)?;
                if request.user_agent.trim().is_empty() {
                    return Err(failed("user agent must not be empty"));
                }
                let page = self.ensure_browser(operation).await?;
                page.cdp(
                    "Emulation.setUserAgentOverride",
                    json!({"userAgent": request.user_agent}),
                )
                .await
                .map_err(|error| failed(format!("set user agent: {error}")))?;
                json!({"user_agent_set": true})
            }
            "open" | "goto" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    url: String,
                }
                let request: Request = decode(frame)?;
                if request.url.trim().is_empty() {
                    return Err(failed("navigation URL is required"));
                }
                let page = self.ensure_browser(operation).await?;
                let state = Self::go_navigate(&page, &frame.cmd, &request.url, operation).await?;
                json!({
                    "action": frame.cmd,
                    "url": state["url"],
                    "http_status": state["http_status"],
                })
            }
            "network.requests" => {
                let page = self.ensure_browser(operation).await?;
                let requests = page.network_requests();
                json!({
                    "requests": if requests.is_empty() { Value::Null } else { json!(requests.iter().map(network_json).collect::<Vec<_>>()) },
                    "count": requests.len(),
                })
            }
            "network.request" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    id: String,
                }
                let request: Request = decode(frame)?;
                let page = self.ensure_browser(operation).await?;
                match page.network_request(&request.id) {
                    Some(entry) => json!({"request": network_json(&entry)}),
                    None => {
                        return Err(DaemonError {
                            code: "network_request_not_found".into(),
                            message: format!("no captured request with id {:?}", request.id),
                            ..Default::default()
                        });
                    }
                }
            }
            "network.route" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    pattern: String,
                    #[serde(default)]
                    action: String,
                    #[serde(default)]
                    status: i64,
                    #[serde(default)]
                    body: Option<Value>,
                    #[serde(default)]
                    content_type: String,
                }
                let request: Request = decode(frame)?;
                let page = self.ensure_browser(operation).await?;
                page.route_requests(symbrowse_engine_chrome::NetworkRoute {
                    pattern: request.pattern.clone(),
                    action: request.action.clone(),
                    status: request.status,
                    body: request.body,
                    content_type: request.content_type,
                })
                .await
                .map_err(cdp_failure)?;
                json!({"routed": request.pattern, "action": request.action})
            }
            "network.unroute" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    pattern: String,
                }
                let request: Request = decode_optional(frame);
                let page = self.ensure_browser(operation).await?;
                let removed = page
                    .unroute_requests(&request.pattern)
                    .await
                    .map_err(cdp_failure)?;
                json!({"removed": removed})
            }
            "network.har" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    action: String,
                    #[serde(default)]
                    content: String,
                }
                let request: Request = decode(frame)?;
                let page = self.ensure_browser(operation).await?;
                match request.action.as_str() {
                    "start" => json!({"started": true}),
                    "stop" => {
                        if request.content == "all" {
                            return Err(failed(
                                "network har content=all (response bodies) is not implemented by the Rust daemon",
                            ));
                        }
                        let requests = page.network_requests();
                        json!({"har": har_document(&requests), "entries": requests.len()})
                    }
                    _ => {
                        return Err(DaemonError {
                            code: "invalid_har_action".into(),
                            message: "network har action must be \"start\" or \"stop\"".into(),
                            ..Default::default()
                        });
                    }
                }
            }
            "download.setdir" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    dir: String,
                }
                let request: Request = decode(frame)?;
                let page = self.ensure_browser(operation).await?;
                let directory = page
                    .set_download_dir(&request.dir)
                    .await
                    .map_err(|error| failed(format!("set download directory: {error}")))?;
                json!({"download_dir": directory})
            }
            "downloads.list" => {
                let events = self.download_events();
                json!({"count": events.len(), "downloads": events})
            }
            "scroll" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    selector: String,
                    #[serde(default)]
                    amount: i64,
                }
                let request: Request = decode(frame)?;
                if request.selector.trim().is_empty() {
                    return Err(DaemonError {
                        code: "invalid_interaction".into(),
                        message: "interaction selector is required".into(),
                        ..Default::default()
                    });
                }
                let page = self.ensure_browser(operation).await?;
                Self::go_scroll(&page, &request.selector, request.amount).await?;
                json!({"action": "scroll", "selector": request.selector})
            }
            "trace.replay" => {
                #[derive(serde::Deserialize, Default)]
                struct Step {
                    #[serde(default)]
                    command: String,
                    #[serde(default)]
                    selector: String,
                    #[serde(default)]
                    value: String,
                    #[serde(default)]
                    key: String,
                    #[serde(default)]
                    url: String,
                    #[serde(default)]
                    expected_url: String,
                }
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    steps: Vec<Step>,
                }
                let request: Request = decode(frame)?;
                if request.steps.is_empty() {
                    return Err(failed("trace contains no replayable steps"));
                }
                let page = self.ensure_browser(operation).await?;
                let (mut matched, mut deviated, mut failed_count) = (0, 0, 0);
                let mut outcomes = Vec::new();
                for (index, step) in request.steps.iter().enumerate() {
                    let mut outcome =
                        json!({"index": index, "command": step.command, "matched": false});
                    let note = |outcome: &mut Value, key: &str, value: &str| {
                        if !value.is_empty() {
                            outcome[key] = json!(value);
                        }
                    };
                    match step.command.as_str() {
                        "open" | "goto" => {
                            match Self::go_navigate(&page, "open", &step.url, operation).await {
                                Err(error) => note(&mut outcome, "error", &error.message),
                                Ok(state) => {
                                    let actual =
                                        state["url"].as_str().unwrap_or_default().to_owned();
                                    note(&mut outcome, "expected_url", &step.expected_url);
                                    note(&mut outcome, "actual_url", &actual);
                                    let normalize = |url: &str| {
                                        url.split('#')
                                            .next()
                                            .unwrap_or(url)
                                            .trim_end_matches('/')
                                            .to_owned()
                                    };
                                    outcome["matched"] =
                                        json!(normalize(&actual) == normalize(&step.expected_url));
                                }
                            }
                        }
                        "scroll" => match Self::go_scroll(&page, &step.selector, 1).await {
                            Ok(()) => outcome["matched"] = json!(true),
                            Err(error) => note(&mut outcome, "error", &error.message),
                        },
                        "click" | "dblclick" | "hover" | "focus" | "check" | "uncheck"
                        | "scrollintoview" | "fill" | "type" | "select" | "press" => {
                            let mut args = serde_json::Map::new();
                            let selector = if step.command == "press" && step.selector.is_empty() {
                                "body"
                            } else {
                                step.selector.as_str()
                            };
                            args.insert("selector".into(), json!(selector));
                            if !step.value.is_empty() {
                                args.insert("value".into(), json!(step.value));
                            }
                            if !step.key.is_empty() {
                                args.insert("key".into(), json!(step.key));
                            }
                            // Go resolves the CSS selector first; mirror its error.
                            if !selector.starts_with('@')
                                && let Err(error) = Self::go_resolve(&page, selector).await
                            {
                                note(&mut outcome, "error", &error.message);
                                failed_count += 1;
                                outcomes.push(outcome);
                                continue;
                            }
                            let inner = Frame {
                                cmd: step.command.clone(),
                                args: Some(Value::Object(args)),
                                session: frame.session.clone(),
                                ..Frame::default()
                            };
                            match Box::pin(self.dispatch_inner(inner, operation.clone())).await {
                                Ok(_) => outcome["matched"] = json!(true),
                                Err(error) => note(&mut outcome, "error", &error.message),
                            }
                        }
                        "auth.login" => note(
                            &mut outcome,
                            "error",
                            "credential step requires symvault re-resolution; replay it with auth login",
                        ),
                        other => note(
                            &mut outcome,
                            "error",
                            &format!("step command {other:?} is not replayable"),
                        ),
                    }
                    if outcome.get("error").is_some() {
                        failed_count += 1;
                    } else if outcome["matched"] == json!(true) {
                        matched += 1;
                    } else {
                        deviated += 1;
                    }
                    outcomes.push(outcome);
                }
                json!({
                    "total": outcomes.len(),
                    "matched": matched,
                    "deviated": deviated,
                    "failed": failed_count,
                    "outcomes": outcomes,
                })
            }
            "frame.tree" => {
                let page = self.ensure_browser(operation).await?;
                let result = page
                    .cdp("Page.getFrameTree", json!({}))
                    .await
                    .map_err(cdp_failure)?;
                json!({"frames": [frame_node(&result["frameTree"])]})
            }
            "frame.main" => {
                let page = self.ensure_browser(operation).await?;
                page.set_active_frame_context(0);
                json!({"frame": "main"})
            }
            "frame.select" => {
                #[derive(serde::Deserialize, Default)]
                struct Request {
                    #[serde(default)]
                    frame: String,
                }
                let request: Request = decode(frame)?;
                let page = self.ensure_browser(operation).await?;
                if request.frame.is_empty() {
                    page.set_active_frame_context(0);
                } else {
                    let result = page
                        .cdp(
                            "Page.createIsolatedWorld",
                            json!({"frameId": request.frame}),
                        )
                        .await
                        .map_err(|error| {
                            failed(format!(
                                "create isolated world for frame {}: {error}",
                                request.frame
                            ))
                        })?;
                    let context = result["executionContextId"].as_i64().unwrap_or(0);
                    if context == 0 {
                        return Err(failed(
                            "chrome returned an empty execution context for the frame",
                        ));
                    }
                    page.set_active_frame_context(context);
                }
                json!({"frame": request.frame})
            }
            other => {
                return Err(DaemonError {
                    code: codes::UNKNOWN_COMMAND.into(),
                    message: format!("command {other:?} is not implemented by the Rust daemon"),
                    ..Default::default()
                });
            }
        };
        Ok((Some(data), Vec::new()))
    }

    /// Go `NavigationService.navigate`: `Page.navigate`, then wait for
    /// `readyState == complete` and report the final URL/status.
    ///
    /// `net::ERR_ABORTED` is not an error (frozen Go oracle: an empty-404
    /// origin reports success).  Every other `errorText` (offline, blocked,
    /// refused, DNS) lands Chrome on `chrome-error://chromewebdata/`; that is
    /// returned as a typed failure instead of a success with `http_status: 0`,
    /// matching the engine-level `page_navigate_error` rule.
    pub(super) async fn go_navigate(
        page: &ChromePage,
        action: &str,
        url: &str,
        operation: &OperationContext,
    ) -> Result<Value, DaemonError> {
        let navigated = page
            .cdp("Page.navigate", json!({"url": url}))
            .await
            .map_err(|error| failed(format!("{action} {url:?}: {error}")))?;
        if let Some(text) = navigation_error_text(&navigated) {
            return Err(failed(format!("{action} {url:?}: {text}")));
        }
        loop {
            let state = page
                .evaluate_expression(NAVIGATION_STATE)
                .await
                .map_err(|error| failed(format!("read final navigation state: {error}")))?;
            if let Some(text) = state["exception_text"].as_str().filter(|t| !t.is_empty()) {
                return Err(failed(format!("read final navigation state: {text}")));
            }
            if state["value"]["ready_state"] == json!("complete") {
                return Ok(state["value"].clone());
            }
            if operation.is_cancelled() || operation.remaining().is_zero() {
                return Err(operation_timeout_error());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// Go `ResolveElement`: CSS selector -> backend node id.
    async fn go_resolve(page: &ChromePage, selector: &str) -> Result<i64, DaemonError> {
        let document = page
            .cdp("DOM.getDocument", json!({}))
            .await
            .map_err(cdp_failure)?;
        let root = document["root"]["nodeId"].as_i64().unwrap_or(0);
        if root == 0 {
            return Err(failed("DOM returned an empty document root"));
        }
        let queried = page
            .cdp(
                "DOM.querySelector",
                json!({"nodeId": root, "selector": selector}),
            )
            .await
            .map_err(|error| failed(format!("query selector {selector:?}: {error}")))?;
        let node = queried["nodeId"].as_i64().unwrap_or(0);
        if node == 0 {
            return Err(failed(format!(
                "selector {selector:?} did not match an element"
            )));
        }
        let described = page
            .cdp("DOM.describeNode", json!({"nodeId": node}))
            .await
            .map_err(cdp_failure)?;
        let backend = described["node"]["backendNodeId"].as_i64().unwrap_or(0);
        if backend == 0 {
            return Err(failed("DOM returned an empty backend node id"));
        }
        Ok(backend)
    }

    /// Go `NavigationService.Interact` for `scroll`: resolve, scroll into
    /// view, focus, then a mouse-wheel event at the element centre.
    async fn go_scroll(page: &ChromePage, selector: &str, amount: i64) -> Result<(), DaemonError> {
        if selector.trim().is_empty() {
            return Err(DaemonError {
                code: "invalid_interaction".into(),
                message: "interaction selector is required".into(),
                ..Default::default()
            });
        }
        if selector.trim().starts_with('@') {
            return Err(failed(
                "snapshot ref selectors are not supported by the Rust scroll command yet",
            ));
        }
        let backend = Self::go_resolve(page, selector).await?;
        page.cdp(
            "DOM.scrollIntoViewIfNeeded",
            json!({"backendNodeId": backend}),
        )
        .await
        .map_err(|error| {
            failed(format!(
                "scroll scroll target {selector:?} into view: {error}"
            ))
        })?;
        page.cdp("DOM.focus", json!({"backendNodeId": backend}))
            .await
            .map_err(|error| failed(format!("scroll {selector:?}: focus target: {error}")))?;
        let quads = page
            .cdp("DOM.getContentQuads", json!({"backendNodeId": backend}))
            .await
            .map_err(|error| failed(format!("scroll {selector:?}: {error}")))?;
        let quad: Vec<f64> = quads["quads"][0]
            .as_array()
            .map(|values| values.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        if quad.len() < 8 {
            return Err(failed(format!(
                "scroll {selector:?}: target has no visible content quad"
            )));
        }
        let (mut x, mut y) = (0.0, 0.0);
        for index in (0..8).step_by(2) {
            x += quad[index];
            y += quad[index + 1];
        }
        let amount = if amount == 0 { 480 } else { amount };
        page.cdp(
            "Input.dispatchMouseEvent",
            json!({"type": "mouseWheel", "x": x / 4.0, "y": y / 4.0, "deltaY": amount as f64}),
        )
        .await
        .map_err(|error| failed(format!("scroll {selector:?}: {error}")))?;
        Ok(())
    }

    async fn apply_viewport(
        page: &ChromePage,
        width: i64,
        height: i64,
        scale: f64,
    ) -> Result<(), DaemonError> {
        if width <= 0 || height <= 0 {
            return Err(failed(format!(
                "viewport dimensions must be positive: {width}x{height}"
            )));
        }
        let scale = if scale <= 0.0 { 1.0 } else { scale };
        page.cdp(
            "Emulation.setDeviceMetricsOverride",
            json!({"width": width, "height": height, "deviceScaleFactor": scale, "mobile": false}),
        )
        .await
        .map_err(|error| failed(format!("set viewport {width}x{height}: {error}")))?;
        Ok(())
    }
}

fn frame_node(node: &Value) -> Value {
    let frame = &node["frame"];
    let mut out = serde_json::Map::new();
    out.insert("id".into(), frame["id"].clone());
    for (key, source) in [("parent_id", "parentId"), ("name", "name"), ("url", "url")] {
        if let Some(text) = frame[source].as_str().filter(|text| !text.is_empty()) {
            out.insert(key.into(), json!(text));
        }
    }
    if let Some(children) = node["childFrames"].as_array().filter(|c| !c.is_empty()) {
        out.insert(
            "children".into(),
            Value::Array(children.iter().map(frame_node).collect()),
        );
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_error_text_follows_go_polling_contract() {
        for (result, expected) in [
            (json!({"frameId": "f"}), None),
            (json!({"frameId": "f", "errorText": ""}), None),
            (json!({"errorText": "net::ERR_ABORTED"}), None),
            (
                json!({"errorText": "net::ERR_INTERNET_DISCONNECTED"}),
                Some("net::ERR_INTERNET_DISCONNECTED"),
            ),
            (
                json!({"errorText": "net::ERR_BLOCKED_BY_CLIENT"}),
                Some("net::ERR_BLOCKED_BY_CLIENT"),
            ),
        ] {
            assert_eq!(navigation_error_text(&result), expected, "{result}");
        }
    }
}
