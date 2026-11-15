//! The Go-shaped command surface (`eval`, `cookies.list|set|clear`,
//! `storage.list|set|clear`, `network.capture|requests`) on the Firefox engine.
//!
//! The CLI sends these names to every engine. Firefox answers them from its own
//! WebDriver BiDi session (cookies through the `storage` module, response
//! capture through `network.responseCompleted`); nothing is routed to Chrome or
//! a generic HTTP stack. Commands this module does not know return `None` so
//! the caller keeps its explicit `unsupported` behavior.

use serde_json::{Map, Value, json};
use symbrowse_engine_firefox::FirefoxSession;

use super::{HandlerResult, firefox_error, malformed, runtime_error};
use crate::{DaemonError, Frame};

fn failed(message: impl Into<String>) -> DaemonError {
    runtime_error(message.into())
}

fn storage_object(kind: &str) -> Result<&'static str, DaemonError> {
    match kind {
        "local" => Ok("localStorage"),
        "session" => Ok("sessionStorage"),
        other => Err(failed(format!("invalid storage kind {other:?}"))),
    }
}

/// Host part of an absolute URL (no userinfo, no port).
fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = if authority.starts_with('[') {
        authority
            .split_once(']')
            .map(|(host, _)| format!("{host}]"))?
    } else {
        authority.split(':').next()?.to_owned()
    };
    (!host.is_empty()).then_some(host)
}

async fn origin(session: &mut FirefoxSession) -> String {
    match session.evaluate("location.origin").await {
        Ok(result) => result
            .value
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// BiDi cookie -> the Go `cookies list` entry shape.
fn cookie_json(cookie: &Value) -> Value {
    let value = cookie["value"]["value"]
        .as_str()
        .or_else(|| cookie["value"].as_str())
        .unwrap_or_default();
    let expiry = cookie.get("expiry").and_then(Value::as_f64);
    let mut out = json!({
        "name": cookie["name"],
        "value": value,
        "domain": cookie["domain"],
        "path": cookie["path"],
        "expires": expiry.unwrap_or(0.0),
        "size": cookie["size"],
        "http_only": cookie["httpOnly"].as_bool().unwrap_or(false),
        "secure": cookie["secure"].as_bool().unwrap_or(false),
        "session": expiry.is_none(),
    });
    if let Some(same_site) = cookie["sameSite"].as_str().filter(|s| !s.is_empty()) {
        out["same_site"] = json!(same_site);
    }
    out
}

fn decoded<'a>(args: &'a Map<String, Value>, name: &str) -> &'a str {
    args.get(name).and_then(Value::as_str).unwrap_or_default()
}

pub(super) async fn dispatch(
    session: &mut FirefoxSession,
    frame: &Frame,
    args: &Map<String, Value>,
) -> Option<HandlerResult> {
    let handled = match frame.cmd.as_str() {
        "eval" => eval(session, args).await,
        "cookies.list" => cookies_list(session).await,
        // The pre-existing raw BiDi form (`cookie.value` is an object) keeps
        // its own handler; the Go form carries plain string values.
        "cookies.set" if args["cookie"]["value"].is_string() => cookies_set(session, args).await,
        "cookies.clear" => cookies_clear(session, args).await,
        "storage.list" | "storage.clear" => storage(session, frame, args).await,
        "storage.set" if args.contains_key("kind") => storage(session, frame, args).await,
        "get.text" | "get.html" | "get.value" | "get.count" | "get.attr" => {
            inspect(session, frame, args).await
        }
        "network.capture" => network_capture(session).await,
        "network.requests" => network_requests(session).await,
        _ => return None,
    };
    Some(handled)
}

async fn eval(session: &mut FirefoxSession, args: &Map<String, Value>) -> HandlerResult {
    let expression = decoded(args, "expression");
    if expression.trim().is_empty() {
        return Err(failed("eval requires a non-empty expression"));
    }
    let result = session.evaluate(expression).await.map_err(firefox_error)?;
    Ok((
        Some(serde_json::to_value(result).map_err(runtime_error)?),
        Vec::new(),
    ))
}

async fn cookies_list(session: &mut FirefoxSession) -> HandlerResult {
    let raw = session.cookies().await.map_err(firefox_error)?;
    let cookies: Vec<Value> = raw["cookies"]
        .as_array()
        .map(|cookies| cookies.iter().map(cookie_json).collect())
        .unwrap_or_default();
    Ok((
        Some(json!({"origin": origin(session).await, "cookies": cookies})),
        Vec::new(),
    ))
}

async fn cookies_set(session: &mut FirefoxSession, args: &Map<String, Value>) -> HandlerResult {
    let cookie = &args["cookie"];
    let name = cookie["name"].as_str().unwrap_or_default();
    if name.is_empty() {
        return Err(failed("cookie name is required"));
    }
    let domain = match cookie["domain"].as_str().filter(|d| !d.is_empty()) {
        Some(domain) => domain.to_owned(),
        None => match host_of(decoded(args, "url")) {
            Some(host) => host,
            None => session
                .evaluate("location.hostname")
                .await
                .map_err(firefox_error)?
                .value
                .and_then(|value| value.as_str().map(str::to_owned))
                .filter(|host| !host.is_empty())
                .ok_or_else(|| failed("cookie domain or url is required"))?,
        },
    };
    let mut bidi = json!({
        "name": name,
        "value": {"type": "string", "value": cookie["value"]},
        "domain": domain,
        "path": cookie["path"].as_str().filter(|p| !p.is_empty()).unwrap_or("/"),
    });
    if cookie["http_only"].as_bool().unwrap_or(false) {
        bidi["httpOnly"] = json!(true);
    }
    if cookie["secure"].as_bool().unwrap_or(false) {
        bidi["secure"] = json!(true);
    }
    if let Some(same_site) = cookie["same_site"].as_str().filter(|s| !s.is_empty()) {
        bidi["sameSite"] = json!(same_site.to_lowercase());
    }
    if let Some(expires) = cookie["expires"].as_f64().filter(|e| *e > 0.0) {
        bidi["expiry"] = json!(expires.trunc());
    }
    session.set_cookie(bidi).await.map_err(firefox_error)?;
    Ok((Some(json!({"set": name})), Vec::new()))
}

async fn cookies_clear(session: &mut FirefoxSession, args: &Map<String, Value>) -> HandlerResult {
    let name = decoded(args, "name");
    if name.is_empty() {
        return Err(failed("cookie name is required"));
    }
    session.delete_cookies(name).await.map_err(firefox_error)?;
    Ok((Some(json!({"cleared": name})), Vec::new()))
}

async fn storage(
    session: &mut FirefoxSession,
    frame: &Frame,
    args: &Map<String, Value>,
) -> HandlerResult {
    let kind = decoded(args, "kind").to_owned();
    let object = storage_object(&kind)?;
    let key = decoded(args, "key");
    let value = decoded(args, "value");
    let expression = match frame.cmd.as_str() {
        "storage.list" => format!(
            "(function(){{ const s = window.{object}; const out = {{}}; for (let i = 0; i < s.length; i++) {{ const k = s.key(i); out[k] = s.getItem(k); }} return JSON.stringify(out); }})()"
        ),
        "storage.set" => {
            if key.trim().is_empty() {
                return Err(failed("storage key is required"));
            }
            format!(
                "(function(){{ window.{object}.setItem({}, {}); return true; }})()",
                serde_json::to_string(key).map_err(runtime_error)?,
                serde_json::to_string(value).map_err(runtime_error)?
            )
        }
        _ => format!("(function(){{ window.{object}.clear(); return true; }})()"),
    };
    let result = session.evaluate(&expression).await.map_err(firefox_error)?;
    if !result.exception_text.is_empty() {
        return Err(failed(format!(
            "{} {kind} storage: {}",
            frame.cmd.trim_start_matches("storage."),
            result.exception_text
        )));
    }
    let data = match frame.cmd.as_str() {
        "storage.list" => {
            let items = result
                .value
                .as_ref()
                .and_then(Value::as_str)
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({}));
            json!({"origin": origin(session).await, "kind": kind, "items": items})
        }
        "storage.set" => json!({"set": key}),
        _ => json!({"cleared": kind}),
    };
    Ok((Some(data), Vec::new()))
}

/// Read one value from the page's DOM; the result is the bare value like the
/// other engines' `get.*` answers.
async fn inspect(
    session: &mut FirefoxSession,
    frame: &Frame,
    args: &Map<String, Value>,
) -> HandlerResult {
    let selector = Some(decoded(args, "selector"))
        .filter(|selector| !selector.is_empty())
        .unwrap_or("body");
    let selector = serde_json::to_string(selector).map_err(runtime_error)?;
    let expression = match frame.cmd.as_str() {
        "get.count" => format!("document.querySelectorAll({selector}).length"),
        command => {
            let read = match command {
                "get.text" => "e.innerText ?? e.textContent".to_owned(),
                "get.html" => "e.outerHTML".to_owned(),
                "get.value" => "e.value".to_owned(),
                _ => {
                    let attribute = decoded(args, "attribute");
                    if attribute.is_empty() {
                        return Err(malformed("missing required argument \"attribute\""));
                    }
                    format!(
                        "e.getAttribute({})",
                        serde_json::to_string(attribute).map_err(runtime_error)?
                    )
                }
            };
            format!(
                "(() => {{ const e = document.querySelector({selector}); if (!e) throw new Error('selector did not match'); return {read}; }})()"
            )
        }
    };
    let result = session.evaluate(&expression).await.map_err(firefox_error)?;
    if !result.exception_text.is_empty() {
        return Err(failed(format!("{}: {}", frame.cmd, result.exception_text)));
    }
    Ok((Some(result.value.unwrap_or(Value::Null)), Vec::new()))
}

async fn network_capture(session: &mut FirefoxSession) -> HandlerResult {
    session
        .start_network_capture()
        .await
        .map_err(firefox_error)?;
    Ok((Some(json!({"started": true})), Vec::new()))
}

async fn network_requests(session: &mut FirefoxSession) -> HandlerResult {
    let requests = session
        .network_responses()
        .await
        .map_err(|error| match error {
            symbrowse_engine_firefox::FirefoxError::InvalidTarget(message) => malformed(message),
            other => firefox_error(other),
        })?;
    Ok((
        Some(json!({"requests": requests, "count": requests.len()})),
        Vec::new(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_of_strips_scheme_userinfo_port_and_path() {
        for (url, expected) in [
            ("http://127.0.0.1:8080/a?b#c", Some("127.0.0.1")),
            ("https://user@example.com/x", Some("example.com")),
            ("http://[::1]:9/", Some("[::1]")),
            ("about:blank", None),
            ("", None),
        ] {
            assert_eq!(host_of(url).as_deref(), expected, "{url}");
        }
    }

    #[test]
    fn bidi_cookie_maps_to_the_go_listing_shape() {
        let cookie = cookie_json(&json!({
            "name": "session", "value": {"type": "string", "value": "abc"},
            "domain": "127.0.0.1", "path": "/", "size": 10,
            "httpOnly": true, "secure": false, "sameSite": "lax"
        }));
        assert_eq!(cookie["value"], "abc");
        assert_eq!(cookie["http_only"], true);
        assert_eq!(cookie["session"], true);
        assert_eq!(cookie["same_site"], "lax");
        let persistent =
            cookie_json(&json!({"name":"a","value":{"type":"string","value":"b"},"expiry":10}));
        assert_eq!(persistent["session"], false);
    }
}
