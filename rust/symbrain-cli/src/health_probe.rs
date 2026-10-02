//! Bounded, credential-avoiding liveness probes for configured MCP servers.
//!
//! `probe_method` describes the liveness operations that actually ran. For
//! HTTP, the required `notifications/initialized` lifecycle notification is
//! not counted as a liveness operation. Diagnostics are fixed, bounded strings;
//! remote bodies, stderr, URLs, protocol details, and session identifiers never
//! cross the health-report boundary.

use std::io::{self, BufRead, BufReader};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use symbrain_broker::{BrokerError, Client, Options, SUPPORTED_PROTOCOL_VERSIONS};
use symbrain_harness::ServerInfo;
use ureq::{Agent, Error as UreqError};

pub(crate) const HEALTH_SCHEMA_VERSION: u32 = 1;
const PROBE_DEADLINE: Duration = Duration::from_secs(5);
const MAX_HTTP_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_HTTP_RESPONSE_HEADER_BYTES: usize = 16 * 1024;
const HTTP_ACCEPT: &str = "application/json, text/event-stream";
const HTTP_STREAMABLE_VERSION: &str = "2025-03-26";

/// New health outcome; the CLI separately retains its legacy `healthy` bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HealthOutcome {
    Healthy,
    Unhealthy,
    Unsupported,
}

#[derive(Debug, Clone)]
pub(crate) struct ProbeResult {
    pub(crate) outcome: HealthOutcome,
    pub(crate) probe_method: Option<&'static str>,
    pub(crate) latency_ms: Option<f64>,
    pub(crate) error: String,
}

impl ProbeResult {
    fn unsupported(error: &'static str) -> Self {
        Self {
            outcome: HealthOutcome::Unsupported,
            probe_method: None,
            latency_ms: None,
            error: error.to_owned(),
        }
    }

    fn unprobed_failure(error: &'static str) -> Self {
        Self {
            outcome: HealthOutcome::Unhealthy,
            probe_method: None,
            latency_ms: None,
            error: error.to_owned(),
        }
    }

    fn measured(
        outcome: HealthOutcome,
        probe_method: &'static str,
        started: Instant,
        error: &'static str,
    ) -> Self {
        let latency_ms = started.elapsed().as_secs_f64() * 1_000.0;
        Self {
            outcome,
            probe_method: Some(probe_method),
            latency_ms: Some(latency_ms),
            error: error.to_owned(),
        }
    }
}

/// Selects the supported transport and refuses configurations that would
/// require credential material or a transport this probe does not implement.
pub(crate) fn probe_server(server: &ServerInfo) -> ProbeResult {
    if !server.env_names.is_empty() {
        return ProbeResult::unsupported("server has configured environment values");
    }
    if arguments_look_credential_backed(&server.args) {
        return ProbeResult::unsupported("server arguments appear credential-backed");
    }

    if server.transport.eq_ignore_ascii_case("stdio") {
        if server.command.is_empty() {
            return ProbeResult::unsupported("stdio server has no configured command");
        }
        return probe_stdio(&server.command, &server.args);
    }

    if server.transport.eq_ignore_ascii_case("http") {
        if server.url.is_empty() {
            return ProbeResult::unsupported("HTTP server has no configured endpoint");
        }
        return probe_http(&server.url);
    }

    ProbeResult::unsupported("configured transport is not supported for health probing")
}

fn arguments_look_credential_backed(args: &[String]) -> bool {
    let sensitive_name = |value: &str| {
        let name = value
            .split_once('=')
            .map_or(value, |(name, _)| name)
            .trim_start_matches('-')
            .to_ascii_lowercase()
            .replace('_', "-");
        [
            "token",
            "api-key",
            "apikey",
            "secret",
            "password",
            "authorization",
            "credential",
            "auth",
            "bearer",
            "cookie",
            "header",
        ]
        .iter()
        .any(|marker| name == *marker || name.ends_with(&format!("-{marker}")))
    };

    args.iter().enumerate().any(|(index, argument)| {
        let lower = argument.to_ascii_lowercase();
        let has_secret_reference = lower.contains("${")
            || lower.contains("$env:")
            || lower.contains("secret://")
            || lower.contains("vault://")
            || lower.contains("authorization:")
            || lower.contains("bearer ")
            || lower.contains("basic ")
            || lower.contains("cookie:")
            || lower.contains("x-api-key");
        has_secret_reference
            || sensitive_name(argument)
            || index > 0 && sensitive_name(&args[index - 1])
    })
}

fn probe_stdio(command: &str, args: &[String]) -> ProbeResult {
    let path = match symbrain_broker::discover(command, "") {
        Ok(path) => path,
        Err(BrokerError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            return ProbeResult::unprobed_failure("server command was not found");
        }
        Err(_) => return ProbeResult::unprobed_failure("server command could not be resolved"),
    };

    let Ok(client) = Client::spawn(
        &path,
        Options {
            args: args.to_vec(),
            env: Some(safe_child_environment()),
            capture_stderr: true,
        },
    ) else {
        return ProbeResult::unprobed_failure("server process could not be started");
    };

    let started = Instant::now();
    let deadline = started + PROBE_DEADLINE;
    let initialize_timeout = remaining(deadline);
    if initialize_timeout.is_zero() {
        return ProbeResult::measured(
            HealthOutcome::Unhealthy,
            "initialize",
            started,
            "MCP probe timed out",
        );
    }
    if client.initialize(initialize_timeout).is_err() {
        return ProbeResult::measured(
            HealthOutcome::Unhealthy,
            "initialize",
            started,
            "MCP initialize failed",
        );
    }

    let ping_timeout = remaining(deadline);
    if ping_timeout.is_zero() {
        return ProbeResult::measured(
            HealthOutcome::Unhealthy,
            "initialize",
            started,
            "MCP probe timed out",
        );
    }
    match client.ping(ping_timeout) {
        Ok(()) => ProbeResult::measured(HealthOutcome::Healthy, "initialize+ping", started, ""),
        Err(BrokerError::Timeout { .. }) => ProbeResult::measured(
            HealthOutcome::Unhealthy,
            "initialize+ping",
            started,
            "MCP probe timed out",
        ),
        Err(_) => ProbeResult::measured(
            HealthOutcome::Unhealthy,
            "initialize+ping",
            started,
            "MCP ping failed",
        ),
    }
}

fn safe_child_environment() -> Vec<(String, String)> {
    const ALLOWLIST: &[&str] = &[
        "PATH",
        "HOME",
        "USERPROFILE",
        "SYSTEMROOT",
        "WINDIR",
        "TMP",
        "TEMP",
        "TMPDIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "APPDATA",
        "LOCALAPPDATA",
        "LANG",
        "LC_ALL",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ];
    ALLOWLIST
        .iter()
        .filter_map(|name| {
            std::env::var_os(name)
                .map(|value| ((*name).to_owned(), value.to_string_lossy().into_owned()))
        })
        .collect()
}

fn probe_http(url: &str) -> ProbeResult {
    if let Some(reason) = unsafe_http_endpoint(url) {
        return ProbeResult::unsupported(reason);
    }
    probe_http_with_timeout(url, PROBE_DEADLINE)
}

fn unsafe_http_endpoint(url: &str) -> Option<&'static str> {
    if url.len() > 4_096 {
        return Some("HTTP endpoint exceeds the safe size limit");
    }
    let lower = url.to_ascii_lowercase();
    let Some(scheme) = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"))
    else {
        return Some("HTTP endpoint is not an HTTP URL");
    };
    let authority = scheme.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() {
        return Some("HTTP endpoint has no host");
    }
    if authority.contains('@') {
        return Some("HTTP endpoint user information is not probed");
    }
    if url.contains('?') || url.contains('#') {
        return Some("HTTP endpoint query or fragment is not probed");
    }
    if lower.contains("${")
        || lower.contains("$env:")
        || lower.contains("secret://")
        || lower.contains("vault://")
        || lower.contains("%24")
        || lower.contains("%7b")
        || lower.contains("%7d")
    {
        return Some("HTTP endpoint contains a secret reference");
    }
    if ureq::http::Uri::try_from(url).is_err() {
        return Some("HTTP endpoint is not a valid HTTP URL");
    }
    None
}

// Keep the full three-step lifecycle visible so one deadline spans all requests.
#[allow(clippy::too_many_lines)]
fn probe_http_with_timeout(url: &str, timeout: Duration) -> ProbeResult {
    let started = Instant::now();
    let deadline = started + timeout;
    let protocol_version = SUPPORTED_PROTOCOL_VERSIONS
        .last()
        .copied()
        .unwrap_or("2024-11-05");
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": protocol_version,
            "capabilities": {},
            "clientInfo": {"name": "symbrain", "version": env!("CARGO_PKG_VERSION")}
        }
    });
    let initialize_reply = match send_http(url, &initialize, None, None, deadline, true) {
        Ok(response) => response,
        Err(failure) => return http_failure(failure, "initialize", started),
    };
    let Some(initialize_result) =
        rpc_result(&initialize_reply.body, &initialize_reply.content_type, 1)
    else {
        return ProbeResult::measured(
            HealthOutcome::Unhealthy,
            "initialize",
            started,
            "MCP HTTP initialize reply was invalid",
        );
    };
    let Some(negotiated_version) = initialize_result
        .get("protocolVersion")
        .and_then(Value::as_str)
    else {
        return ProbeResult::measured(
            HealthOutcome::Unhealthy,
            "initialize",
            started,
            "MCP HTTP initialize reply was invalid",
        );
    };
    if !SUPPORTED_PROTOCOL_VERSIONS.contains(&negotiated_version) {
        return ProbeResult::measured(
            HealthOutcome::Unsupported,
            "initialize",
            started,
            "MCP server negotiated an unsupported protocol version",
        );
    }
    if negotiated_version < HTTP_STREAMABLE_VERSION {
        return ProbeResult::measured(
            HealthOutcome::Unsupported,
            "initialize",
            started,
            "legacy HTTP and SSE transport is unsupported",
        );
    }

    let initialized = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    match send_http(
        url,
        &initialized,
        Some(negotiated_version),
        initialize_reply.session_id.as_deref(),
        deadline,
        false,
    ) {
        Ok(response) if response.status == 202 => {}
        Ok(_) => {
            return ProbeResult::measured(
                HealthOutcome::Unhealthy,
                "initialize",
                started,
                "MCP HTTP initialized notification was rejected",
            );
        }
        Err(failure) => return http_failure(failure, "initialize", started),
    }

    let ping = json!({"jsonrpc": "2.0", "id": 2, "method": "ping"});
    let ping_reply = match send_http(
        url,
        &ping,
        Some(negotiated_version),
        initialize_reply.session_id.as_deref(),
        deadline,
        true,
    ) {
        Ok(response) => response,
        Err(failure) => return http_failure(failure, "initialize+ping", started),
    };
    if rpc_result(&ping_reply.body, &ping_reply.content_type, 2).is_none() {
        return ProbeResult::measured(
            HealthOutcome::Unhealthy,
            "initialize+ping",
            started,
            "MCP HTTP ping reply was invalid",
        );
    }
    ProbeResult::measured(HealthOutcome::Healthy, "initialize+ping", started, "")
}

struct HttpResponse {
    status: u16,
    content_type: String,
    body: Vec<u8>,
    session_id: Option<String>,
}

#[derive(Clone, Copy)]
enum HttpFailure {
    Timeout,
    Connection,
    Redirect,
    AuthenticationRequired,
    UnsupportedEndpoint,
    UnsupportedContentType,
    OversizedResponse,
    InvalidReply,
}

fn send_http(
    url: &str,
    payload: &Value,
    protocol_version: Option<&str>,
    session_id: Option<&str>,
    deadline: Instant,
    expect_reply: bool,
) -> Result<HttpResponse, HttpFailure> {
    let timeout = remaining(deadline);
    if timeout.is_zero() {
        return Err(HttpFailure::Timeout);
    }
    let agent: Agent = Agent::config_builder()
        .proxy(None)
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .max_response_header_size(MAX_HTTP_RESPONSE_HEADER_BYTES)
        .timeout_global(Some(timeout))
        .build()
        .into();
    let body = serde_json::to_vec(payload).map_err(|_| HttpFailure::InvalidReply)?;
    let mut request = agent
        .post(url)
        .header("accept", HTTP_ACCEPT)
        .content_type("application/json");
    if let Some(version) = protocol_version {
        request = request.header("MCP-Protocol-Version", version);
    }
    if let Some(session) = session_id {
        request = request.header("Mcp-Session-Id", session);
    }
    let mut response = request
        .send(body.as_slice())
        .map_err(|error| map_ureq_error(&error))?;
    let status = response.status().as_u16();
    if (300..400).contains(&status) {
        return Err(HttpFailure::Redirect);
    }
    if status == 401 || status == 403 {
        return Err(HttpFailure::AuthenticationRequired);
    }
    if status == 404 || status == 405 || status == 415 || status == 501 {
        return Err(HttpFailure::UnsupportedEndpoint);
    }
    if !response.status().is_success() {
        return Err(HttpFailure::Connection);
    }
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map_or_else(String::new, ToOwned::to_owned);
    let session_id = response
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    if !expect_reply {
        return Ok(HttpResponse {
            status,
            content_type,
            body: Vec::new(),
            session_id,
        });
    }
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if media_type != "application/json" && media_type != "text/event-stream" {
        return Err(HttpFailure::UnsupportedContentType);
    }
    let body = if media_type == "application/json" {
        response
            .body_mut()
            .with_config()
            .limit(MAX_HTTP_RESPONSE_BYTES as u64)
            .read_to_vec()
            .map_err(|error| map_ureq_error(&error))?
    } else {
        read_sse_reply(response.body_mut(), 64 * 1024).map_err(|error| map_io_error(&error))?
    };
    Ok(HttpResponse {
        status,
        content_type: media_type,
        body,
        session_id,
    })
}

fn read_sse_reply(body: &mut ureq::Body, limit: usize) -> io::Result<Vec<u8>> {
    let reader = body.with_config().limit(limit as u64).reader();
    let mut reader = BufReader::with_capacity(1024, reader);
    let mut frame = Vec::new();
    let mut consumed = 0_usize;
    loop {
        let available = reader.fill_buf().map_err(|error| {
            if consumed >= limit {
                return io::Error::new(io::ErrorKind::InvalidData, "response body limit exceeded");
            }
            error
        })?;
        if available.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "event stream ended before the RPC reply",
            ));
        }
        let take = available
            .len()
            .min(limit.saturating_sub(consumed).saturating_add(1));
        frame.extend_from_slice(&available[..take]);
        reader.consume(take);
        consumed = consumed.saturating_add(take);
        if consumed > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "response body limit exceeded",
            ));
        }
        while let Some((event, end)) = take_sse_event(&frame) {
            if let Some(value) = parse_sse_event(event)
                && value.get("id").and_then(Value::as_i64).is_some()
            {
                return serde_json::to_vec(&value)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid RPC reply"));
            }
            frame.drain(..end);
        }
    }
}

fn take_sse_event(frame: &[u8]) -> Option<(&[u8], usize)> {
    for index in 0..frame.len() {
        if frame.get(index..index + 4) == Some(b"\r\n\r\n") {
            return Some((&frame[..index], index + 4));
        }
        if frame.get(index..index + 2) == Some(b"\n\n") {
            return Some((&frame[..index], index + 2));
        }
    }
    None
}

fn parse_sse_event(event: &[u8]) -> Option<Value> {
    let event = std::str::from_utf8(event).ok()?;
    let mut data = String::new();
    for line in event.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let Some(value) = line.strip_prefix("data:") else {
            continue;
        };
        if !data.is_empty() {
            data.push('\n');
        }
        data.push_str(value.strip_prefix(' ').unwrap_or(value));
    }
    serde_json::from_str(&data).ok()
}

fn rpc_result(body: &[u8], content_type: &str, wanted_id: i64) -> Option<Value> {
    let value = if content_type.eq_ignore_ascii_case("text/event-stream") {
        serde_json::from_slice(body)
            .ok()
            .or_else(|| parse_sse_event(body))?
    } else {
        serde_json::from_slice(body).ok()?
    };
    if value.get("jsonrpc")?.as_str()? != "2.0"
        || value.get("id")?.as_i64()? != wanted_id
        || value.get("error").is_some()
    {
        return None;
    }
    value.get("result").cloned()
}

fn http_failure(failure: HttpFailure, method: &'static str, started: Instant) -> ProbeResult {
    let (outcome, diagnostic) = match failure {
        HttpFailure::Timeout => (HealthOutcome::Unhealthy, "MCP HTTP probe timed out"),
        HttpFailure::Connection => (HealthOutcome::Unhealthy, "MCP HTTP request failed"),
        HttpFailure::Redirect => (HealthOutcome::Unsupported, "MCP HTTP redirect was refused"),
        HttpFailure::AuthenticationRequired => (
            HealthOutcome::Unsupported,
            "MCP HTTP endpoint requires authentication",
        ),
        HttpFailure::UnsupportedEndpoint => (
            HealthOutcome::Unsupported,
            "MCP HTTP endpoint does not support Streamable HTTP",
        ),
        HttpFailure::UnsupportedContentType => (
            HealthOutcome::Unsupported,
            "MCP HTTP response content type is unsupported",
        ),
        HttpFailure::OversizedResponse => (
            HealthOutcome::Unhealthy,
            "MCP HTTP response exceeded the size limit",
        ),
        HttpFailure::InvalidReply => (
            HealthOutcome::Unhealthy,
            "MCP HTTP response was not a valid protocol reply",
        ),
    };
    ProbeResult::measured(outcome, method, started, diagnostic)
}

fn map_ureq_error(error: &UreqError) -> HttpFailure {
    match error {
        UreqError::Timeout(_) => HttpFailure::Timeout,
        UreqError::BodyExceedsLimit(_) => HttpFailure::OversizedResponse,
        _ => HttpFailure::Connection,
    }
}

fn map_io_error(error: &io::Error) -> HttpFailure {
    if error.kind() == io::ErrorKind::TimedOut {
        return HttpFailure::Timeout;
    }
    if let Some(inner) = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<UreqError>())
    {
        return map_ureq_error(inner);
    }
    if error.kind() == io::ErrorKind::InvalidData {
        HttpFailure::InvalidReply
    } else {
        HttpFailure::Connection
    }
}

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_transports_and_credentials_never_get_probe_metadata() {
        let mut server = ServerInfo {
            name: "test".into(),
            transport: "http".into(),
            url: "https://example.test/mcp?token=canary".into(),
            ..ServerInfo::default()
        };
        let result = probe_server(&server);
        assert_eq!(result.outcome, HealthOutcome::Unsupported);
        assert!(result.probe_method.is_none());
        assert!(result.latency_ms.is_none());
        assert!(!result.error.contains("canary"));

        server.url = "https://user:secret@example.test/mcp".into();
        let result = probe_server(&server);
        assert_eq!(result.outcome, HealthOutcome::Unsupported);
        assert!(result.latency_ms.is_none());
        assert!(!result.error.contains("secret"));

        server.url = "http://127.0.0.1/mcp".into();
        server.env_names.push("API_TOKEN".into());
        let result = probe_server(&server);
        assert_eq!(result.outcome, HealthOutcome::Unsupported);
        assert!(result.probe_method.is_none());
        assert!(result.latency_ms.is_none());

        server.env_names.clear();
        server.args = vec!["--header".into(), "Authorization: Bearer canary".into()];
        let result = probe_server(&server);
        assert_eq!(result.outcome, HealthOutcome::Unsupported);
        assert!(result.probe_method.is_none());
        assert!(result.latency_ms.is_none());
        assert!(!result.error.contains("canary"));
    }

    #[test]
    fn stdio_argument_credential_guards_are_conservative() {
        assert!(arguments_look_credential_backed(&[
            "--api-key".into(),
            "canary".into()
        ]));
        assert!(arguments_look_credential_backed(&["${TOKEN}".into()]));
        assert!(!arguments_look_credential_backed(&[
            "mcp".into(),
            "--verbose".into()
        ]));
    }

    #[test]
    fn sse_reply_parser_accepts_jsonrpc_data_and_ignores_other_events() {
        let mut frame =
            b"event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\r\n\r\n"
                .to_vec();
        let (event, end) = take_sse_event(&frame).expect("complete CRLF event");
        assert_eq!(parse_sse_event(event).unwrap()["id"], 1);
        frame.drain(..end);
        frame.extend_from_slice(b"data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}\n\n");
        let (event, _) = take_sse_event(&frame).expect("complete LF event");
        assert_eq!(parse_sse_event(event).unwrap()["id"], 2);
    }

    #[test]
    fn latency_keeps_sub_millisecond_measurement_as_a_fraction() {
        let started = Instant::now();
        let result = ProbeResult::measured(HealthOutcome::Healthy, "initialize+ping", started, "");
        assert!(result.latency_ms.is_some_and(|latency| latency >= 0.0));
        assert_eq!(result.probe_method, Some("initialize+ping"));
    }

    #[derive(Debug)]
    struct CapturedRequest {
        headers: std::collections::HashMap<String, String>,
        body: Value,
    }

    #[derive(Debug, Clone)]
    struct FixtureResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        delay: Duration,
    }

    impl FixtureResponse {
        // Fixture callers pass short-lived json! values for readability.
        #[allow(clippy::needless_pass_by_value)]
        fn json(status: u16, body: Value) -> Self {
            Self {
                status,
                headers: vec![("content-type".into(), "application/json".into())],
                body: serde_json::to_vec(&body).unwrap(),
                delay: Duration::ZERO,
            }
        }
    }

    fn http_fixture(
        count: usize,
        handler: impl Fn(&CapturedRequest) -> FixtureResponse + Send + 'static,
    ) -> (String, std::thread::JoinHandle<Vec<CapturedRequest>>) {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let mut captured = Vec::new();
            for _ in 0..count {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                let mut headers = std::collections::HashMap::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line == "\n" || line.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = line.trim_end().split_once(':') {
                        headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
                    }
                }
                let length = headers
                    .get("content-length")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or_default();
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let request = CapturedRequest {
                    headers,
                    body: serde_json::from_slice(&body).unwrap(),
                };
                let response = handler(&request);
                std::thread::sleep(response.delay);
                let reason = match response.status {
                    200 => "OK",
                    202 => "Accepted",
                    302 => "Found",
                    401 => "Unauthorized",
                    403 => "Forbidden",
                    404 => "Not Found",
                    500 => "Internal Server Error",
                    _ => "Fixture",
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    response.status,
                    reason,
                    response.body.len()
                );
                for (name, value) in response.headers {
                    let _ = write!(stream, "{name}: {value}\r\n");
                }
                let _ = write!(stream, "\r\n");
                let _ = stream.write_all(&response.body);
                captured.push(request);
            }
            captured
        });
        (format!("http://{address}/mcp"), thread)
    }

    fn streamable_fixture(
        ping_reply: FixtureResponse,
    ) -> (String, std::thread::JoinHandle<Vec<CapturedRequest>>) {
        http_fixture(3, move |request| match request.body["method"].as_str() {
            Some("initialize") => {
                let mut response = FixtureResponse::json(
                    200,
                    json!({"jsonrpc":"2.0","id":1,"result":{
                        "protocolVersion":"2025-03-26",
                        "capabilities":{},
                        "serverInfo":{"name":"fixture","version":"1"}
                    }}),
                );
                response
                    .headers
                    .push(("Mcp-Session-Id".into(), "session-canary".into()));
                response
            }
            Some("notifications/initialized") => FixtureResponse::json(202, json!({})),
            Some("ping") => ping_reply.clone(),
            _ => FixtureResponse::json(400, json!({})),
        })
    }

    #[test]
    fn streamable_http_negotiates_protocol_and_pings_with_session() {
        let ping = FixtureResponse::json(200, json!({"jsonrpc":"2.0","id":2,"result":{}}));
        let (url, server) = streamable_fixture(ping);
        let result = probe_http_with_timeout(&url, Duration::from_secs(2));
        let requests = server.join().unwrap();
        assert_eq!(result.outcome, HealthOutcome::Healthy, "{}", result.error);
        assert_eq!(result.probe_method, Some("initialize+ping"));
        assert!(result.latency_ms.is_some_and(|ms| ms >= 0.0));
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].body["method"], "initialize");
        assert_eq!(
            requests[0].body["params"]["protocolVersion"].as_str(),
            SUPPORTED_PROTOCOL_VERSIONS.last().copied()
        );
        assert!(!requests[0].headers.contains_key("mcp-protocol-version"));
        assert_eq!(requests[1].body["method"], "notifications/initialized");
        assert_eq!(requests[1].headers["mcp-protocol-version"], "2025-03-26");
        assert_eq!(requests[2].body["method"], "ping");
        assert_eq!(requests[2].body["id"], 2);
        assert_eq!(requests[2].headers["mcp-session-id"], "session-canary");
        assert!(!result.error.contains("session-canary"));
    }

    #[test]
    fn streamable_http_accepts_bounded_sse_rpc_replies() {
        let event = format!(
            "event: message\ndata: {}\n\n",
            json!({"jsonrpc":"2.0","id":2,"result":{}})
        );
        let ping = FixtureResponse {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
            body: event.into_bytes(),
            delay: Duration::ZERO,
        };
        let (url, server) = streamable_fixture(ping);
        let result = probe_http_with_timeout(&url, Duration::from_secs(2));
        let _ = server.join().unwrap();
        assert_eq!(result.outcome, HealthOutcome::Healthy, "{}", result.error);
    }

    #[test]
    fn streamable_http_rejects_malformed_and_error_replies_without_leaking_body() {
        for body in [
            b"not-json".to_vec(),
            serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"error":{
                "code":-1,"message":"response-canary"
            }}))
            .unwrap(),
        ] {
            let (url, server) = http_fixture(1, move |_| FixtureResponse {
                status: 200,
                headers: vec![("content-type".into(), "application/json".into())],
                body: body.clone(),
                delay: Duration::ZERO,
            });
            let result = probe_http_with_timeout(&url, Duration::from_secs(2));
            let _ = server.join().unwrap();
            assert_eq!(result.outcome, HealthOutcome::Unhealthy);
            assert_eq!(result.probe_method, Some("initialize"));
            assert!(result.latency_ms.is_some());
            assert!(!result.error.contains("response-canary"));
        }
    }

    #[test]
    fn http_deadline_and_redirect_refusal_are_measured_and_redacted() {
        let (url, server) = http_fixture(1, |_| {
            let mut response = FixtureResponse::json(200, json!({}));
            response.delay = Duration::from_millis(250);
            response
        });
        let started = Instant::now();
        let result = probe_http_with_timeout(&url, Duration::from_millis(60));
        let elapsed = started.elapsed();
        let _ = server.join().unwrap();
        assert_eq!(result.outcome, HealthOutcome::Unhealthy);
        assert_eq!(result.error, "MCP HTTP probe timed out");
        assert!(result.latency_ms.is_some());
        assert!(
            elapsed < Duration::from_secs(1),
            "probe elapsed {elapsed:?}"
        );

        let (url, server) = http_fixture(1, |_| {
            let mut response = FixtureResponse::json(302, json!({}));
            response
                .headers
                .push(("location".into(), "http://127.0.0.1/leak-canary".into()));
            response
        });
        let result = probe_http_with_timeout(&url, Duration::from_secs(1));
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(result.outcome, HealthOutcome::Unsupported);
        assert_eq!(result.error, "MCP HTTP redirect was refused");
        assert!(!result.error.contains("leak-canary"));
        assert!(result.latency_ms.is_some());

        let (url, server) = http_fixture(1, |_| FixtureResponse::json(401, json!({})));
        let result = probe_http_with_timeout(&url, Duration::from_secs(1));
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].headers.contains_key("authorization"));
        assert_eq!(result.outcome, HealthOutcome::Unsupported);
        assert_eq!(result.error, "MCP HTTP endpoint requires authentication");
        assert!(result.latency_ms.is_some());

        let (url, server) = http_fixture(1, |_| FixtureResponse {
            status: 200,
            headers: vec![("content-type".into(), "application/json".into())],
            body: vec![b' '; MAX_HTTP_RESPONSE_BYTES + 1],
            delay: Duration::ZERO,
        });
        let result = probe_http_with_timeout(&url, Duration::from_secs(1));
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(result.outcome, HealthOutcome::Unhealthy);
        assert_eq!(result.error, "MCP HTTP response exceeded the size limit");
        assert!(result.latency_ms.is_some());
    }
}
