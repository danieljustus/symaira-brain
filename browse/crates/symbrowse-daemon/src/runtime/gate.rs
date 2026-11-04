//! Policy gate, out-of-band approval, action journal, flow recorder and the
//! `policy.explain` / `journal.*` / `oob.*` / `handoff` / `flow.record.*`
//! commands of the Go daemon (`cmd/symbrowse/daemon.go` handler switch).

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use symbrowse_core::{
    journal,
    oob::{self, Kind, Status},
    policy::{self as cpolicy, Decision, Mode, Policy, RiskClass},
    policy_guard::{Guard, GuardInput},
};

use crate::{DaemonError, Frame, OperationContext, SessionSpec, codes};

/// Commands the Go handler routes through `DecideAndConfirm` and the journal.
pub(super) const GATED: &[&str] = &[
    "fetch.url",
    "fetch.batch",
    "wayback.snapshots",
    "open",
    "goto",
    "back",
    "forward",
    "reload",
    "wait",
    "snapshot",
    "read",
    "find",
    "a11y",
    "screenshot",
    "click",
    "dblclick",
    "fill",
    "type",
    "press",
    "hover",
    "focus",
    "select",
    "check",
    "uncheck",
    "scroll",
    "scrollintoview",
    "get.text",
    "get.html",
    "get.value",
    "get.attr",
    "get.title",
    "get.url",
    "get.count",
    "get.box",
    "get.styles",
    "is.visible",
    "is.enabled",
    "is.checked",
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
    "eval",
    "console.list",
    "console.clear",
    "errors.list",
    "errors.clear",
    "network.requests",
    "network.request",
    "network.route",
    "network.unroute",
    "network.har",
    "upload",
    "downloads.list",
    "download.setdir",
];

/// Commands the flow recorder keeps (Go `isRecordableCommand`).
const RECORDABLE: &[&str] = &[
    "open",
    "goto",
    "click",
    "dblclick",
    "fill",
    "type",
    "press",
    "hover",
    "focus",
    "select",
    "check",
    "uncheck",
    "wait",
    "snapshot",
    "scroll",
    "scrollintoview",
];

#[derive(Default)]
struct Recorder {
    active: bool,
    actions: Vec<Value>,
    index: i64,
}

pub(super) struct GoRuntime {
    policy: Policy,
    mode: Mode,
    guard: Option<Guard>,
    pub(super) oob: oob::Manager,
    journal: Option<journal::Store>,
    journal_dir: PathBuf,
    recorders: Mutex<BTreeMap<String, Recorder>>,
    approval_timeout: Duration,
}

pub(super) fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Port of the Go daemon's tolerant `hostOfURL` (not the policy package's).
fn host_of_url(raw: &str) -> String {
    let mut rest = raw;
    let bytes = raw.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'/' {
            rest = raw.get(index + 2..).unwrap_or("");
            break;
        }
    }
    for (index, byte) in rest.as_bytes().iter().enumerate() {
        if *byte == b'/' || *byte == b':' {
            return rest[..index].to_owned();
        }
    }
    rest.to_owned()
}

fn frame_url(frame: &Frame) -> String {
    let Some(args) = frame.args.as_ref().and_then(Value::as_object) else {
        return String::new();
    };
    if let Some(url) = args.get("url").and_then(Value::as_str)
        && !url.is_empty()
    {
        return url.to_owned();
    }
    args.get("urls")
        .and_then(Value::as_array)
        .and_then(|urls| urls.first())
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn failed(message: impl Into<String>) -> DaemonError {
    DaemonError {
        code: codes::OPERATION_FAILED.into(),
        message: message.into(),
        ..Default::default()
    }
}

fn decode_args(frame: &Frame) -> Result<&Value, DaemonError> {
    match frame.args.as_ref() {
        None | Some(Value::Null) => Err(failed("command arguments are required")),
        Some(value) => Ok(value),
    }
}

fn error_kind(error: &DaemonError) -> String {
    if error.code.is_empty() || error.code == codes::OPERATION_FAILED {
        "failed".into()
    } else {
        error.code.clone()
    }
}

impl GoRuntime {
    pub(super) fn new(spec: &SessionSpec) -> Self {
        let policy_path = spec.state_dir.join("policy.toml");
        let policy = Policy::load(&policy_path).unwrap_or_else(|_| Policy {
            rules: Vec::new(),
            source: policy_path.display().to_string(),
        });
        let mode = if std::env::var("SYMBROWSE_MCP").as_deref() == Ok("1") {
            Mode::Mcp
        } else {
            Mode::Tty
        };
        let journal_dir = spec.state_dir.join("journal");
        let journal = journal::Store::new(
            &journal_dir,
            &spec.session,
            journal::Redactor::standard(),
            String::new(),
        )
        .ok();
        let approval_timeout = std::env::var("SYMBROWSE_APPROVAL_TIMEOUT")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .map_or(Duration::from_secs(60), Duration::from_secs);
        Self {
            policy,
            mode,
            guard: Guard::detect().filter(Guard::active),
            oob: oob::Manager::new(),
            journal,
            journal_dir,
            recorders: Mutex::new(BTreeMap::new()),
            approval_timeout,
        }
    }

    pub(super) fn is_gated(command: &str) -> bool {
        GATED.contains(&command)
    }

    fn decide(
        &self,
        command: &str,
        url: &str,
        mode: Mode,
    ) -> Result<(Decision, &'static str, String), DaemonError> {
        let class = cpolicy::classify(command).map_err(|error| failed(error.to_string()))?;
        let host = host_of_url(url);
        if let Some(guard) = &self.guard {
            return Ok(
                match guard.decide(&GuardInput {
                    command: command.to_owned(),
                    class,
                    domain: host,
                    warnings: Vec::new(),
                }) {
                    Err(error) => (Decision::Deny, "guard", format!("guard failure: {error}")),
                    Ok(outcome) => {
                        let reason = if outcome.reason.is_empty() {
                            "guard".to_owned()
                        } else {
                            format!("guard:{}", outcome.reason)
                        };
                        (outcome.decision, "guard", reason)
                    }
                },
            );
        }
        let (decision, origin) = self.policy.decide(class, &host, mode);
        Ok((decision, "policy", origin))
    }

    /// Go `OOBRuntime.DecideAndConfirm`: `(allowed, decision, decider)`.
    pub(super) async fn decide_and_confirm(
        &self,
        command: &str,
        url: &str,
        operation: &OperationContext,
    ) -> Result<(bool, Decision, String), DaemonError> {
        let (decision, decider, _) = self.decide(command, url, self.mode)?;
        match decision {
            Decision::Allow => Ok((true, decision, decider.to_owned())),
            Decision::Deny => Ok((false, decision, decider.to_owned())),
            Decision::Confirm => {
                let class = cpolicy::classify(command).unwrap_or(RiskClass::Interact);
                let reason = format!("{command} ({class}) gegen {url}");
                let prompt = self.oob.create(
                    Kind::Approval,
                    "Symaira Browse: Freigabe nötig",
                    reason,
                    self.approval_timeout,
                    now_rfc3339(),
                );
                notify(&prompt);
                let status = self
                    .wait_for(&prompt.id, self.approval_timeout, operation)
                    .await;
                let allowed = status == Some(Status::Completed);
                Ok((
                    allowed,
                    decision,
                    if allowed { "human" } else { decider }.to_owned(),
                ))
            }
        }
    }

    /// Wait for a prompt without blocking cancellation: poll the manager so a
    /// cancelled operation stops waiting promptly. Timeout resolves to deny.
    async fn wait_for(
        &self,
        id: &str,
        timeout: Duration,
        operation: &OperationContext,
    ) -> Option<Status> {
        let started = Instant::now();
        loop {
            let prompt = self.oob.get(id)?;
            if prompt.status != Status::Pending {
                return Some(prompt.status);
            }
            if started.elapsed() >= timeout {
                return self.oob.expire(id).map(|prompt| prompt.status);
            }
            if operation.is_cancelled() || operation.remaining().is_zero() {
                return self.oob.cancel(id, "cancelled").map(|prompt| prompt.status);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub(super) fn gate_denied(command: &str, decision: Decision) -> DaemonError {
        DaemonError {
            code: codes::PEER_DENIED.into(),
            message: format!("policy decision {decision} denied {command}"),
            ..Default::default()
        }
    }

    /// Append one journal entry; failures become a response warning message.
    pub(super) fn journal_append(
        &self,
        frame: &Frame,
        decider: &str,
        url_before: String,
        url_after: String,
        duration_ms: u64,
        outcome: &Result<(), DaemonError>,
    ) -> Option<String> {
        let store = self.journal.as_ref()?;
        let entry = journal::Entry {
            timestamp: now_rfc3339(),
            session: frame.session.clone(),
            command: frame.cmd.clone(),
            args: frame.args.clone().filter(|args| !args.is_null()),
            url_before,
            url_after,
            risk_class: cpolicy::class_for_command(&frame.cmd)
                .map_or_else(|| "unknown".to_owned(), |class| class.as_str().to_owned()),
            decider: decider.to_owned(),
            duration_ms: (duration_ms > 0).then_some(duration_ms),
            result: match outcome {
                Ok(()) => "ok".into(),
                Err(error) => format!("error:{}", error_kind(error)),
            },
            ..journal::Entry::default()
        };
        store
            .append(entry)
            .err()
            .map(|error| format!("journal append failed: {error}"))
    }

    pub(super) fn frame_journal_url(frame: &Frame) -> String {
        frame_url(frame)
    }

    // ---- flow.record.* ---------------------------------------------------

    pub(super) fn record_command(&self, frame: &Frame) -> Result<Value, DaemonError> {
        let mut recorders = self
            .recorders
            .lock()
            .map_err(|_| failed("recorder lock poisoned"))?;
        let recorder = recorders.entry(frame.session.clone()).or_default();
        match frame.cmd.as_str() {
            "flow.record.start" => {
                recorder.actions.clear();
                recorder.index = 0;
                recorder.active = true;
                Ok(json!({"recording": true, "session": frame.session}))
            }
            "flow.record.stop" => {
                recorder.active = false;
                Ok(json!({
                    "recording": false,
                    "session": frame.session,
                    "actions": recorder.actions,
                }))
            }
            "flow.record.status" => Ok(json!({
                "recording": recorder.active,
                "session": frame.session,
                "actions": recorder.actions.len(),
            })),
            other => Err(failed(format!("unknown recording command {other:?}"))),
        }
    }

    /// Go `recordFrame`: keep navigation/interaction frames while recording.
    pub(super) fn record_frame(&self, frame: &Frame) {
        if !RECORDABLE.contains(&frame.cmd.as_str()) {
            return;
        }
        let Ok(mut recorders) = self.recorders.lock() else {
            return;
        };
        let Some(recorder) = recorders.get_mut(&frame.session) else {
            return;
        };
        if !recorder.active {
            return;
        }
        let args = frame.args.as_ref().and_then(Value::as_object);
        let text = |key: &str| {
            args.and_then(|args| args.get(key))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let (url, selector, value) = (text("url"), text("selector"), text("value"));
        let (selector, value) = if !url.is_empty() {
            (url, String::new())
        } else if !selector.is_empty() {
            (selector, value)
        } else {
            (String::new(), value)
        };
        let mut action = serde_json::Map::new();
        action.insert("index".into(), json!(recorder.index));
        action.insert("command".into(), json!(frame.cmd));
        if !selector.is_empty() {
            action.insert("selector".into(), json!(selector));
        }
        if !value.is_empty() {
            action.insert("value".into(), json!(value));
        }
        recorder.index += 1;
        recorder.actions.push(Value::Object(action));
    }

    // ---- policy.explain --------------------------------------------------

    pub(super) fn policy_explain(&self, frame: &Frame) -> Result<Value, DaemonError> {
        let args = decode_args(frame)?;
        let text = |key: &str| {
            args.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let (command, url, mode_text) = (text("command"), text("url"), text("mode"));
        let mode = match mode_text.as_str() {
            "" => self.mode,
            "mcp" => Mode::Mcp,
            _ => Mode::Tty,
        };
        let mut explanation = self
            .policy
            .explain(&command, &url, mode)
            .map_err(|error| failed(error.to_string()))?;
        let (_, decider, reason) = self.decide(&command, &url, mode)?;
        let guard = self
            .guard
            .as_ref()
            .map_or_else(|| "not configured".to_owned(), Guard::command);
        explanation.push_str(&format!(
            "\ndecider:   {decider}\nguard:     {guard}\nreason:    {reason}"
        ));
        Ok(json!({
            "explanation": explanation,
            "source": self.policy.source,
            "decider": decider,
            "guard_active": self.guard.is_some(),
        }))
    }

    // ---- journal.* ---------------------------------------------------------

    pub(super) fn journal_command(&self, frame: &Frame) -> Result<Value, DaemonError> {
        let store = self
            .journal
            .as_ref()
            .ok_or_else(|| failed("journal is not enabled"))?;
        let args = frame.args.as_ref().and_then(Value::as_object);
        let requested = args
            .and_then(|args| args.get("session"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (store, session) = if requested.is_empty() {
            (store.clone(), frame_session(store, &self.journal_dir))
        } else {
            let reopened = journal::Store::new(
                &self.journal_dir,
                requested,
                journal::Redactor::standard(),
                String::new(),
            )
            .map_err(|error| failed(error.to_string()))?;
            (reopened, requested.to_owned())
        };
        let entries = match frame.cmd.as_str() {
            "journal.tail" => {
                let lines = args
                    .and_then(|args| args.get("lines"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
                store.tail(lines)
            }
            _ => store.read(),
        }
        .map_err(|error| failed(error.to_string()))?;
        Ok(json!({
            "schema_version": journal::SCHEMA_VERSION,
            "session": session,
            "entries": if entries.is_empty() { Value::Null } else { json!(entries) },
        }))
    }

    // ---- oob.* / handoff ---------------------------------------------------

    pub(super) fn oob_command(&self, frame: &Frame) -> Result<Value, DaemonError> {
        match frame.cmd.as_str() {
            "oob.status" => {
                let active = self.oob.active();
                Ok(match active {
                    Some(prompt) => json!({"active": true, "prompt": prompt_json(&prompt)}),
                    None => json!({"active": false}),
                })
            }
            "oob.complete" => {
                let id = string_arg(decode_args(frame)?, "id");
                self.finish_prompt(&id, |manager, id| manager.complete(id, None))
            }
            "oob.cancel" => {
                let args = decode_args(frame)?;
                let (id, reason) = (string_arg(args, "id"), string_arg(args, "reason"));
                self.finish_prompt(&id, |manager, id| manager.cancel(id, reason.clone()))
            }
            other => Err(failed(format!("unknown oob command {other:?}"))),
        }
    }

    fn finish_prompt(
        &self,
        id: &str,
        finish: impl Fn(&oob::Manager, &str) -> Option<oob::Prompt>,
    ) -> Result<Value, DaemonError> {
        let Some(existing) = self.oob.get(id) else {
            return Err(failed(format!("oob prompt {id:?} not found")));
        };
        match finish(&self.oob, id) {
            Some(prompt) => Ok(prompt_json(&prompt)),
            None => Err(failed(format!(
                "oob prompt {id:?} is already {}",
                status_name(existing.status)
            ))),
        }
    }

    pub(super) async fn handoff(
        &self,
        frame: &Frame,
        operation: &OperationContext,
    ) -> Result<Value, DaemonError> {
        let args = decode_args(frame)?;
        let reason = string_arg(args, "reason");
        let timeout_text = string_arg(args, "timeout");
        let timeout = if timeout_text.is_empty() {
            Duration::from_secs(300)
        } else {
            parse_go_duration(&timeout_text)
                .ok_or_else(|| failed(format!("invalid timeout {timeout_text:?}")))?
        };
        let prompt = self.oob.create(
            Kind::Handoff,
            "Symaira Browse: der Agent wartet",
            reason,
            timeout,
            now_rfc3339(),
        );
        notify(&prompt);
        let started = Instant::now();
        let status = self.wait_for(&prompt.id, timeout, operation).await;
        let finished = self.oob.get(&prompt.id);
        let mut payload = serde_json::Map::new();
        payload.insert(
            "status".into(),
            json!(status.map_or("timeout", status_name)),
        );
        payload.insert("prompt_id".into(), json!(prompt.id));
        payload.insert(
            "duration_ms".into(),
            json!(started.elapsed().as_millis() as u64),
        );
        if let Some(Value::Object(result)) = finished.and_then(|prompt| prompt.result) {
            for (key, value) in result {
                payload.insert(key, value);
            }
        }
        if status == Some(Status::Timeout) {
            return Err(DaemonError {
                code: codes::HANDOFF_TIMEOUT.into(),
                message: format!(
                    "handoff timed out for session {}",
                    crate::session_quote::quote(&frame.session)
                ),
                ..Default::default()
            });
        }
        Ok(Value::Object(payload))
    }
}

/// The Go `oob.Prompt` wire shape (`timeout` in nanoseconds, optional result).
fn prompt_json(prompt: &oob::Prompt) -> Value {
    let mut value = json!({
        "id": prompt.id,
        "kind": prompt.kind,
        "title": prompt.title,
        "reason": prompt.reason,
        "status": prompt.status,
        "created_at": prompt.created_at,
    });
    if prompt.timeout_ms > 0 {
        value["timeout"] = json!(prompt.timeout_ms.saturating_mul(1_000_000));
    }
    if let Some(result) = &prompt.result {
        value["result"] = result.clone();
    }
    value
}

fn frame_session(store: &journal::Store, _dir: &std::path::Path) -> String {
    store
        .path()
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn string_arg(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Pending => "pending",
        Status::Completed => "completed",
        Status::Cancelled => "cancelled",
        Status::Timeout => "timeout",
    }
}

/// Minimal `time.ParseDuration`: sequences of `<number><unit>` with
/// ns, us, µs, ms, s, m, h units.
pub(super) fn parse_go_duration(text: &str) -> Option<Duration> {
    let mut rest = text.trim();
    if rest.is_empty() {
        return None;
    }
    let mut total = 0f64;
    while !rest.is_empty() {
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let number: f64 = rest[..end].parse().ok()?;
        rest = &rest[end..];
        let unit_end = rest
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(rest.len());
        let scale = match &rest[..unit_end] {
            "ns" => 1e-9,
            "us" | "µs" => 1e-6,
            "ms" => 1e-3,
            "s" => 1.0,
            "m" => 60.0,
            "h" => 3600.0,
            _ => return None,
        };
        total += number * scale;
        rest = &rest[unit_end..];
    }
    Some(Duration::from_secs_f64(total))
}

/// Fire-and-forget platform notification (macOS `osascript`). Failures never
/// block the prompt. `SYMBROWSE_OOB_NOTIFY=off` disables it for tests.
fn notify(prompt: &oob::Prompt) {
    if !cfg!(target_os = "macos") || std::env::var("SYMBROWSE_OOB_NOTIFY").as_deref() == Ok("off") {
        return;
    }
    let command = oob::notification_command(prompt);
    std::thread::spawn(move || {
        let _ = std::process::Command::new(&command.program)
            .args(&command.args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    });
}
