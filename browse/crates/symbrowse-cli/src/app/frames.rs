//! Handlers for every Go command. Each mirrors the corresponding `RunE`:
//! the same daemon frame (command + JSON args), the same text rendering and
//! the same error classification.

use serde_json::{Map, Value, json};
use symbrowse_core::{error::ErrorCode, go_value, output::Envelope};
use symbrowse_daemon::Response;

use super::{
    CliError, Ctx, Handled, Outcome, daemon_request, plain_request, request_no_autostart,
    response_error,
};

fn go_quote(value: &str) -> String {
    symbrowse_core::go_quote::quote(value)
}

fn session(ctx: &Ctx<'_>) -> String {
    ctx.flags.string("session", "default")
}

fn max_tokens(ctx: &Ctx<'_>) -> Option<i64> {
    if !ctx.flags.changed("max-tokens") {
        return None;
    }
    let value = ctx.flags.int("max-tokens", 0);
    (value > 0).then_some(value)
}

fn obj(entries: Vec<(&str, Value)>) -> Value {
    Value::Object(entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// `json.Marshal` of an int-typed Go field: integers stay integral.
fn int(value: i64) -> Value {
    Value::from(value)
}

fn not_native(name: &str) -> Outcome {
    Outcome::Fail(CliError::plain(format!(
        "symbrowse {name} is not implemented natively in the Rust CLI yet"
    )))
}

fn done(result: Handled) -> Outcome {
    match result {
        Ok(()) => Outcome::Done,
        Err(error) => Outcome::Fail(error),
    }
}

fn data_map(response: &Response) -> Map<String, Value> {
    match response.data.as_ref().map(go_value::to_go_any) {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn str_of(map: &Map<String, Value>, key: &str) -> String {
    map.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn go_int(map: &Map<String, Value>, key: &str) -> i64 {
    map.get(key).and_then(Value::as_f64).map_or(0, |v| v as i64)
}

// ---------------------------------------------------------------------------
// Custom `Args:` closures (they run before PersistentPreRunE).

pub fn custom_args(path: &[String], ctx_flags: &crate::cobra::FlagValues, args: &[String]) -> Result<(), CliError> {
    let key = path.join(" ");
    let n = args.len();
    let plain = |m: String| Err(CliError::plain(m));
    match key.as_str() {
        "find" if !(3..=4).contains(&n) => Err(CliError::invalid_args(
            "find requires kind, query, action, and an optional value",
        )),
        "cookies set" => {
            let expected = if ctx_flags.string("curl", "").is_empty() { 2 } else { 0 };
            if n != expected {
                return plain(crate::cobra::exact_args_error(expected, n));
            }
            Ok(())
        }
        "set device" => {
            if ctx_flags.bool("list") {
                if n > 0 {
                    return plain(format!("unknown command {} for \"symbrowse set device\"", go_quote(&args[0])));
                }
            } else if n != 1 {
                return plain(crate::cobra::exact_args_error(1, n));
            }
            Ok(())
        }
        "check" | "click" | "dblclick" | "fill" | "focus" | "hover" | "press" | "scroll"
        | "scrollintoview" | "select" | "type" | "uncheck" => {
            let name = path[0].as_str();
            let max = if matches!(name, "fill" | "type" | "press" | "select" | "scroll") { 2 } else { 1 };
            if n < 1 || n > max {
                return Err(CliError::invalid_args(format!(
                    "{name} requires a selector and optional value"
                )));
            }
            Ok(())
        }
        _ if path.len() == 2 && matches!(path[0].as_str(), "get" | "is") => {
            let (group, kind) = (path[0].as_str(), path[1].as_str());
            if matches!(kind, "title" | "url") {
                if n > 1 {
                    return Err(CliError::invalid_args(format!("get {kind} accepts at most one selector")));
                }
            } else if kind == "attr" {
                if n != 2 {
                    return Err(CliError::invalid_args("get attr requires a selector and attribute name"));
                }
            } else if kind == "styles" {
                if n < 1 {
                    return Err(CliError::invalid_args("get styles requires a selector"));
                }
            } else if n != 1 {
                return Err(CliError::invalid_args(format!("{group} {kind} requires a selector")));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Dispatch

pub fn dispatch(ctx: &mut Ctx<'_>) -> Outcome {
    let key = ctx.path.join(" ");
    let args = ctx.args.clone();
    match key.as_str() {
        "open" | "goto" | "back" | "forward" | "reload" => done(navigate(ctx, &key, &args)),
        "wait" => done(wait(ctx, &args)),
        "check" | "click" | "dblclick" | "fill" | "focus" | "hover" | "press" | "scroll"
        | "scrollintoview" | "select" | "type" | "uncheck" => done(interact(ctx, &key, &args)),
        "find" => done(find(ctx, &args)),
        "snapshot" => done(snapshot(ctx)),
        "screenshot" => done(screenshot(ctx, &args)),
        "eval" => done(eval(ctx, &args)),
        "console list" => done(console_list(ctx)),
        "console clear" => done(clear_frame(ctx, "console.clear", "console cleared")),
        "errors list" => done(errors_list(ctx)),
        "errors clear" => done(clear_frame(ctx, "errors.clear", "errors cleared")),
        "upload" => done(upload(ctx, &args)),
        "downloads" => done(downloads(ctx)),
        "network requests" => done(network_requests(ctx)),
        "network request" => done(network_request(ctx, &args)),
        "network route" => done(network_route(ctx, &args)),
        "network unroute" => done(network_unroute(ctx, &args)),
        "network har" => done(network_har(ctx, &args)),
        "session list" => done(no_autostart(ctx, "session.list")),
        "session info" => done(no_autostart(ctx, "session.info")),
        "daemon status" => done(no_autostart(ctx, "daemon.status")),
        "daemon stop" => done(no_autostart(ctx, "daemon.stop")),
        "tab list" => done(simple(ctx, "tab.list", None)),
        "tab new" => done(tab_new(ctx, &args)),
        "tab switch" => done(simple(ctx, "tab.switch", Some(json!({"tab": args[0]})))),
        "tab close" => done(simple(
            ctx,
            "tab.close",
            Some(json!({"tab": args.first().cloned().unwrap_or_default()})),
        )),
        "tab window window" => done(simple(ctx, "window.new", None)),
        "frame tree" => done(simple(ctx, "frame.tree", None)),
        "frame main" => done(simple(ctx, "frame.main", None)),
        "frame select" => done(simple(ctx, "frame.select", Some(json!({"frame": args[0]})))),
        "dialog status" => done(simple(ctx, "dialog.status", None)),
        "dialog accept" => done(simple(
            ctx,
            "dialog.accept",
            Some(json!({"text": args.first().cloned().unwrap_or_default()})),
        )),
        "dialog dismiss" => done(simple(ctx, "dialog.dismiss", None)),
        "dialog auto" => done(simple(ctx, "dialog.auto", Some(json!({"mode": args[0]})))),
        "auth login" => done(auth_login(ctx, &args)),
        "handoff" => done(handoff(ctx)),
        "journal tail" => done(journal(ctx, true)),
        "journal show" => done(journal(ctx, false)),
        "oob status" => done(oob_status(ctx)),
        "policy explain" => done(policy_explain(ctx, &args)),
        "state save" => done(state_named(ctx, "state.save", &args, "saved state")),
        "state load" => done(state_named(ctx, "state.load", &args, "loaded state")),
        "state clear" => done(state_named(ctx, "state.clear", &args, "cleared state")),
        "state show" => done(state_show(ctx, &args)),
        "state list" => done(state_list(ctx)),
        "state clean" => done(state_clean(ctx)),
        "cookies list" => done(cookies_list(ctx)),
        "cookies set" => done(cookies_set(ctx, &args)),
        "cookies clear" => done(cookies_clear(ctx, &args)),
        "storage get" => done(storage_get(ctx, &args)),
        "storage set" => done(storage_op(ctx, "storage.set", json!({"kind": args[0], "key": args[1], "value": args[2]}))),
        "storage clear" => done(storage_op(ctx, "storage.clear", json!({"kind": args[0]}))),
        "set viewport" => done(set_viewport(ctx, &args)),
        "set geo" => done(set_geo(ctx, &args)),
        "set offline" => done(set_offline(ctx, &args)),
        "set headers" => done(set_headers(ctx, &args)),
        "set media" => done(set_media(ctx, &args)),
        "set user-agent" => done(send_set(ctx, "set.user-agent", json!({"user_agent": args[0]}))),
        "set device" => done(set_device(ctx, &args)),
        "a11y" => done(a11y(ctx, &args)),
        "diff snapshot" => done(diff_snapshot(ctx)),
        "diff url" => done(diff_url(ctx, &args)),
        "diff screenshot" => done(diff_screenshot(ctx)),
        "flow record" => done(flow_record(ctx, &args)),
        k if k.starts_with("get ") || k.starts_with("is ") => done(inspect(ctx, &args)),
        other => crate::local::dispatch(ctx, other, &args),
    }
}

// ---------------------------------------------------------------------------
// Navigation / interaction

fn navigate(ctx: &mut Ctx<'_>, name: &str, args: &[String]) -> Handled {
    let opening = matches!(name, "open" | "goto");
    if opening && args.len() != 1 {
        return Err(CliError::wrapf(2, ErrorCode::InvalidArgs, "navigation URL is required"));
    }
    if !opening && !args.is_empty() {
        return Err(CliError::wrapf(
            2,
            ErrorCode::InvalidArgs,
            "this navigation command does not accept arguments",
        ));
    }
    let payload = (args.len() == 1).then(|| json!({"url": args[0]}));
    let response = daemon_request(&session(ctx), name, payload, None)?;
    if !response.success && response.error.is_none() {
        return Err(CliError::plain("navigation request failed"));
    }
    ctx.write_response(&response)
}

fn wait(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let ms = ctx.flags.int("ms", 0);
    let text = ctx.flags.string("text", "");
    let url = ctx.flags.string("url", "");
    let load = ctx.flags.string("load", "");
    let state = ctx.flags.string("state", "visible");
    let selected = usize::from(args.len() == 1)
        + usize::from(ms != 0)
        + usize::from(!text.is_empty())
        + usize::from(!url.is_empty())
        + usize::from(!load.is_empty());
    if selected != 1 {
        return Err(CliError::invalid_args("wait requires exactly one condition"));
    }
    let mut condition = Map::new();
    if args.len() == 1 {
        condition.insert("kind".into(), "selector".into());
        if !args[0].is_empty() {
            condition.insert("value".into(), args[0].clone().into());
        }
        if !state.is_empty() {
            condition.insert("state".into(), state.into());
        }
    } else if ms < 0 {
        return Err(CliError::invalid_args("--ms cannot be negative"));
    } else if ms != 0 {
        condition.insert("kind".into(), "milliseconds".into());
        condition.insert("duration".into(), int(ms.saturating_mul(1_000_000)));
    } else if !text.is_empty() {
        condition.insert("kind".into(), "text".into());
        condition.insert("value".into(), text.into());
    } else if !url.is_empty() {
        condition.insert("kind".into(), "url".into());
        condition.insert("value".into(), url.into());
    } else if matches!(load.as_str(), "load" | "domcontentloaded" | "networkidle") {
        condition.insert("kind".into(), "load".into());
        condition.insert("load_state".into(), load.into());
    } else {
        return Err(CliError::invalid_args(format!(
            "invalid --load state {}",
            go_quote(&load)
        )));
    }
    let response = daemon_request(&session(ctx), "wait", Some(Value::Object(condition)), None)?;
    if !response.success && response.error.is_none() {
        return Err(CliError::plain("navigation request failed"));
    }
    ctx.write_response(&response)
}

fn interact(ctx: &mut Ctx<'_>, name: &str, args: &[String]) -> Handled {
    let mut request = Map::new();
    request.insert("action".into(), name.into());
    request.insert("selector".into(), args[0].clone().into());
    if args.len() == 2 {
        match name {
            "scroll" => {
                let amount = parse_int10(&args[1]).map_err(|reason| {
                    CliError::invalid_args(format!(
                        "scroll amount: strconv.ParseInt: parsing {}: {reason}",
                        go_quote(&args[1])
                    ))
                })?;
                if amount != 0 {
                    request.insert("amount".into(), int(amount));
                }
            }
            "press" => {
                if !args[1].is_empty() {
                    request.insert("key".into(), args[1].clone().into());
                }
            }
            _ => {
                if !args[1].is_empty() {
                    request.insert("value".into(), args[1].clone().into());
                }
            }
        }
    }
    let response = daemon_request(&session(ctx), name, Some(Value::Object(request)), None)?;
    if !response.success {
        return Err(response_error(&response));
    }
    ctx.write_response(&response)
}

/// `strconv.ParseInt(s, 10, 64)`.
fn parse_int10(value: &str) -> Result<i64, &'static str> {
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err("invalid syntax");
    }
    value.parse::<i64>().map_err(|_| "value out of range")
}

fn find(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let mut request = Map::new();
    request.insert("kind".into(), args[0].clone().into());
    request.insert("query".into(), args[1].clone().into());
    request.insert("action".into(), args[2].clone().into());
    let name = ctx.flags.string("name", "");
    if !name.is_empty() {
        request.insert("name".into(), name.into());
    }
    if ctx.flags.bool("exact") {
        request.insert("exact".into(), true.into());
    }
    if args.len() == 4 {
        if args[2] == "nth" {
            let index = parse_int10(&args[3]).map_err(|reason| {
                CliError::invalid_args(format!(
                    "find nth index: strconv.Atoi: parsing {}: {reason}",
                    go_quote(&args[3])
                ))
            })?;
            if index != 0 {
                request.insert("index".into(), int(index));
            }
        } else if !args[3].is_empty() {
            request.insert("value".into(), args[3].clone().into());
        }
    }
    let response = daemon_request(&session(ctx), "find", Some(Value::Object(request)), None)?;
    if !response.success && response.error.is_none() {
        return Err(CliError::plain("find request failed"));
    }
    if !response.success {
        return Err(response_error(&response));
    }
    if ctx.structured() {
        return ctx.write_response(&response);
    }
    let data = data_map(&response);
    if str_of(&data, "action") == "text" {
        let line = str_of(&data, "value");
        ctx.println(&line);
    } else {
        let line = format!("@{}", str_of(&data, "ref"));
        ctx.println(&line);
    }
    Ok(())
}

fn inspect(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let group = ctx.path[0].clone();
    let kind = ctx.path[1].clone();
    let mut request = Map::new();
    request.insert("kind".into(), kind.clone().into());
    if let Some(selector) = args.first().filter(|s| !s.is_empty()) {
        request.insert("selector".into(), selector.clone().into());
    }
    if kind == "attr" {
        request.insert("attribute".into(), args[1].clone().into());
    }
    if kind == "styles" && args.len() > 1 {
        request.insert("properties".into(), args[1..].to_vec().into());
    }
    let response = daemon_request(
        &session(ctx),
        &format!("{group}.{kind}"),
        Some(Value::Object(request)),
        max_tokens(ctx),
    )?;
    if !response.success && response.error.is_none() {
        return Err(CliError::plain("inspection request failed"));
    }
    if !response.success {
        return Err(response_error(&response));
    }
    if ctx.structured() {
        return ctx.write_response(&response);
    }
    let data = data_map(&response);
    let Some(value) = data.get("value") else {
        return Err(CliError::plain("decode inspection value: unexpected end of JSON input"));
    };
    ctx.println(&go_value::sprint(value));
    if group == "is" {
        let Some(matched) = value.as_bool() else {
            return Err(CliError::plain("state inspection returned a non-boolean value"));
        };
        if !matched {
            return Err(CliError::wrapf(
                1,
                ErrorCode::InvalidArgs,
                format!("state {} is false", str_of(&data, "kind")),
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Snapshot / screenshot / eval / console / errors

fn snapshot(ctx: &mut Ctx<'_>) -> Handled {
    let mut request = Map::new();
    for (name, key) in [("interactive", "interactive"), ("compact", "compact"), ("urls", "urls")] {
        if ctx.flags.bool(name) {
            request.insert(key.into(), true.into());
        }
    }
    let depth = ctx.flags.int("depth", 0);
    if depth != 0 {
        request.insert("depth".into(), int(depth));
    }
    for (name, key) in [("selector", "selector"), ("since", "since")] {
        let value = ctx.flags.string(name, "");
        if !value.is_empty() {
            request.insert(key.into(), value.into());
        }
    }
    let since = ctx.flags.string("since", "");
    let diff = ctx.flags.bool("diff") || !since.is_empty();
    if diff {
        request.insert("diff".into(), true.into());
    }
    if ctx.flags.bool("no-injection-scan") {
        request.insert("no_injection_scan".into(), true.into());
    }
    let patterns = ctx.flags.string("injection-patterns", "");
    if !patterns.is_empty() {
        request.insert("injection_patterns".into(), patterns.into());
    }
    if ctx.flags.bool("no-injection-scan") {
        slog_warn(
            ctx,
            "injection scan disabled by --no-injection-scan; page output is not checked for prompt-injection vectors",
        );
    }
    let session_name = session(ctx);
    let mut response = daemon_request(&session_name, "snapshot", Some(Value::Object(request)), max_tokens(ctx))?;
    if !response.success {
        return Err(response_error(&response));
    }
    if ctx.flags.bool("content-boundaries") && !ctx.structured() {
        let origin = current_page_origin(&session_name)?;
        let boundary = symbrowse_core::injection_boundary::Boundary::new(origin)
            .map_err(|error| CliError::plain(error.to_string()))?;
        let text = go_value::sprint(&response.data.as_ref().map_or(Value::Null, go_value::to_go_any));
        response.data = Some(Value::String(boundary.wrap_text(&text)));
    }
    ctx.write_response(&response)
}

/// `slog.Warn` through the default text handler (stderr).
fn slog_warn(ctx: &mut Ctx<'_>, message: &str) {
    let now = time::OffsetDateTime::now_utc();
    let stamp = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.millisecond()
    );
    let line = format!("time={stamp} level=WARN msg={}\n", go_quote(message));
    ctx.err.extend_from_slice(line.as_bytes());
}

fn current_page_origin(session_name: &str) -> Result<String, CliError> {
    let response = daemon_request(session_name, "get.url", None, None)?;
    if !response.success {
        return Err(response_error(&response));
    }
    let data = data_map(&response);
    match data.get("value") {
        Some(Value::String(origin)) => Ok(origin.clone()),
        _ => Err(CliError::plain("json: cannot unmarshal into Go value of type string")),
    }
}

fn screenshot(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let mut payload = Map::new();
    payload.insert("full".into(), ctx.flags.bool("full").into());
    payload.insert("selector".into(), ctx.flags.string("selector", "").into());
    payload.insert("format".into(), ctx.flags.string("format", "png").into());
    payload.insert("quality".into(), int(ctx.flags.int("quality", 0)));
    payload.insert("dir".into(), ctx.flags.string("screenshot-dir", "").into());
    if args.len() == 1 {
        payload.insert("path".into(), args[0].clone().into());
    }
    let response = daemon_request(&session(ctx), "screenshot", Some(Value::Object(payload)), None)?;
    if !response.success {
        return Err(response_error(&response));
    }
    if ctx.structured() {
        return ctx.write_response(&response);
    }
    let data = data_map(&response);
    let line = format!(
        "screenshot saved to {} ({}x{}, {} bytes)",
        str_of(&data, "path"),
        go_int(&data, "width"),
        go_int(&data, "height"),
        go_int(&data, "bytes")
    );
    ctx.println(&line);
    Ok(())
}

fn eval(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let expression = eval_expression(ctx, args)?;
    let response = daemon_request(&session(ctx), "eval", Some(json!({"expression": expression})), None)?;
    if !response.success {
        return Err(response_error(&response));
    }
    let data = response.data.as_ref().map_or(Value::Null, go_value::to_go_any);
    if ctx.structured() {
        return ctx.write_ok(data);
    }
    let payload = data.as_object().cloned().unwrap_or_default();
    let exception = str_of(&payload, "exception_text");
    if !exception.is_empty() {
        return Err(CliError::plain(format!("eval threw: {exception}")));
    }
    match payload.get("value") {
        None | Some(Value::Null) => ctx.println("undefined"),
        Some(value) => {
            let raw = go_value::marshal(value);
            ctx.println(&raw);
        }
    }
    Ok(())
}

/// `evalExpression`: stdin, argv or base64.
fn eval_expression(ctx: &mut Ctx<'_>, args: &[String]) -> Result<String, CliError> {
    let from_stdin = ctx.flags.bool("stdin");
    let encoded = ctx.flags.bool("base64");
    let mut expression;
    if from_stdin {
        let mut raw = Vec::new();
        ctx.stdin
            .read_to_end(&mut raw)
            .map_err(|error| CliError::plain(format!("read expression from stdin: {error}")))?;
        expression = String::from_utf8_lossy(&raw).into_owned();
    } else if let Some(first) = args.first() {
        expression = first.clone();
    } else {
        return Err(CliError::invalid_args("eval requires an expression argument (or --stdin)"));
    }
    if encoded {
        let raw = go_base64_decode(&expression).map_err(|offset| {
            CliError::invalid_args(format!(
                "decode base64 expression: illegal base64 data at input byte {offset}"
            ))
        })?;
        expression = String::from_utf8_lossy(&raw).into_owned();
    }
    Ok(expression)
}

/// `base64.StdEncoding.DecodeString` (non-strict; `\r`/`\n` ignored).
pub fn go_base64_decode(input: &str) -> Result<Vec<u8>, usize> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let src = input.as_bytes();
    let map = |b: u8| ALPHABET.iter().position(|a| *a == b);
    let mut out = Vec::new();
    let mut si = 0;
    while si < src.len() {
        let mut dbuf = [0u8; 4];
        let mut dlen = 4;
        let mut j = 0;
        let mut trailing_err = None;
        while j < 4 {
            if si == src.len() {
                if j == 0 {
                    return Ok(out);
                }
                return Err(si - j);
            }
            let byte = src[si];
            si += 1;
            if let Some(value) = map(byte) {
                dbuf[j] = value as u8;
                j += 1;
                continue;
            }
            if byte == b'\n' || byte == b'\r' {
                continue;
            }
            if byte != b'=' {
                return Err(si - 1);
            }
            match j {
                0 | 1 => return Err(si - 1),
                2 => {
                    while si < src.len() && (src[si] == b'\n' || src[si] == b'\r') {
                        si += 1;
                    }
                    if si == src.len() {
                        return Err(src.len());
                    }
                    if src[si] != b'=' {
                        return Err(si - 1);
                    }
                    si += 1;
                }
                _ => {}
            }
            while si < src.len() && (src[si] == b'\n' || src[si] == b'\r') {
                si += 1;
            }
            if si < src.len() {
                trailing_err = Some(si);
            }
            dlen = j;
            break;
        }
        let val = (u32::from(dbuf[0]) << 18) | (u32::from(dbuf[1]) << 12) | (u32::from(dbuf[2]) << 6) | u32::from(dbuf[3]);
        let bytes = [(val >> 16) as u8, (val >> 8) as u8, val as u8];
        out.extend_from_slice(&bytes[..match dlen {
            4 => 3,
            3 => 2,
            _ => 1,
        }]);
        if let Some(offset) = trailing_err {
            return Err(offset);
        }
        if dlen < 4 {
            return Ok(out);
        }
    }
    Ok(out)
}

fn print_entries(ctx: &mut Ctx<'_>, response: &Response, errors: bool) -> Handled {
    let data = response.data.as_ref().map_or(Value::Null, go_value::to_go_any);
    if ctx.structured() {
        return ctx.write_ok(data);
    }
    let payload = data.as_object().cloned().unwrap_or_default();
    let entries = payload
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for entry in entries {
        let fields = entry.as_object().cloned().unwrap_or_default();
        let text = str_of(&fields, "text");
        if errors {
            ctx.println(&text);
            if let Some(Value::Array(frames)) = fields.get("stacktrace") {
                for frame in frames {
                    let line = format!("    at {}", go_value::sprint(frame));
                    ctx.println(&line);
                }
            }
        } else {
            let line = format!("[{}] {text}", str_of(&fields, "type"));
            ctx.println(&line);
        }
    }
    Ok(())
}

fn console_list(ctx: &mut Ctx<'_>) -> Handled {
    let response = daemon_request(&session(ctx), "console.list", None, max_tokens(ctx))?;
    if !response.success {
        return Err(response_error(&response));
    }
    print_entries(ctx, &response, false)
}

fn errors_list(ctx: &mut Ctx<'_>) -> Handled {
    let response = daemon_request(&session(ctx), "errors.list", None, max_tokens(ctx))?;
    if !response.success {
        return Err(response_error(&response));
    }
    print_entries(ctx, &response, true)
}

/// Commands that print a fixed line after success (`structuredOutput` first).
fn clear_frame(ctx: &mut Ctx<'_>, command: &str, message: &str) -> Handled {
    let response = daemon_request(&session(ctx), command, None, None)?;
    if ctx.structured() {
        return ctx.write_response(&response);
    }
    if !response.success {
        return Err(response_error(&response));
    }
    ctx.println(message);
    Ok(())
}

fn upload(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let payload = json!({"selector": args[0], "files": args[1..]});
    let response = daemon_request(&session(ctx), "upload", Some(payload), None)?;
    if ctx.structured() {
        return ctx.write_response(&response);
    }
    if !response.success {
        return Err(response_error(&response));
    }
    ctx.println(&format!("uploaded {} file(s)", args.len() - 1));
    Ok(())
}

fn downloads(ctx: &mut Ctx<'_>) -> Handled {
    let session_name = session(ctx);
    let dir = ctx.flags.string("dir", "");
    if !dir.is_empty() {
        let response = daemon_request(&session_name, "download.setdir", Some(json!({"dir": dir})), None)?;
        if !response.success {
            return Err(response_error(&response));
        }
        ctx.println(&format!("download directory set to {dir}"));
    }
    let response = daemon_request(&session_name, "downloads.list", None, None)?;
    if ctx.structured() {
        return ctx.write_response(&response);
    }
    if !response.success {
        return Err(response_error(&response));
    }
    let data = data_map(&response);
    for entry in data.get("downloads").and_then(Value::as_array).cloned().unwrap_or_default() {
        let fields = entry.as_object().cloned().unwrap_or_default();
        let state = str_of(&fields, "state");
        let mut line = format!("[{state}] {} ({})", str_of(&fields, "filename"), str_of(&fields, "url"));
        let sha = str_of(&fields, "sha256");
        if state == "completed" && !sha.is_empty() {
            line.push_str(&format!(" sha256:{sha}"));
        }
        ctx.println(&line);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// network

fn network_requests(ctx: &mut Ctx<'_>) -> Handled {
    let response = daemon_request(&session(ctx), "network.requests", None, max_tokens(ctx))?;
    if !response.success {
        return Err(response_error(&response));
    }
    let data = data_map(&response);
    let filter = ctx.flags.string("filter", "").to_lowercase();
    let kind = ctx.flags.string("type", "");
    let method = ctx.flags.string("method", "");
    let status = ctx.flags.int("status", 0);
    let mut filtered: Vec<Value> = Vec::new();
    for entry in data.get("requests").and_then(Value::as_array).cloned().unwrap_or_default() {
        let fields = entry.as_object().cloned().unwrap_or_default();
        if !filter.is_empty() && !str_of(&fields, "url").to_lowercase().contains(&filter) {
            continue;
        }
        if !kind.is_empty() && !str_of(&fields, "type").eq_ignore_ascii_case(&kind) {
            continue;
        }
        if !method.is_empty() && !str_of(&fields, "method").eq_ignore_ascii_case(&method) {
            continue;
        }
        if status > 0 && go_int(&fields, "status") != status {
            continue;
        }
        filtered.push(entry);
    }
    if ctx.structured() {
        let count = filtered.len() as i64;
        let requests = if filtered.is_empty() { Value::Null } else { Value::Array(filtered) };
        let value = obj(vec![("requests", requests), ("count", int(count))]);
        ctx.println(&go_value::marshal_indent(&value));
        return Ok(());
    }
    for entry in filtered {
        let fields = entry.as_object().cloned().unwrap_or_default();
        let line = format!(
            "{} {} {}",
            str_of(&fields, "method"),
            str_of(&fields, "url"),
            str_of(&fields, "type")
        );
        ctx.println(&line);
    }
    Ok(())
}

fn network_request(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let response = daemon_request(&session(ctx), "network.request", Some(json!({"id": args[0]})), None)?;
    if ctx.structured() {
        return ctx.write_response(&response);
    }
    if !response.success {
        return Err(response_error(&response));
    }
    let data = response.data.as_ref().map_or(Value::Null, go_value::to_go_any);
    ctx.println(&go_value::marshal_indent(&data));
    Ok(())
}

fn network_route(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let mut route = Map::new();
    route.insert("pattern".into(), args[0].clone().into());
    let action = if ctx.flags.bool("abort") {
        "abort"
    } else {
        "mock"
    };
    route.insert("action".into(), action.into());
    let mut invalid_body = false;
    if action == "mock" {
        route.insert("status".into(), int(ctx.flags.int("status", 200)));
        let body = ctx.flags.string("body", "");
        if !body.is_empty() {
            match serde_json::from_str::<Value>(&body) {
                Ok(value) => {
                    route.insert("body".into(), value);
                }
                Err(_) => invalid_body = true,
            }
        }
    }
    // json.Marshal fails for an invalid RawMessage; the Go code ignores the
    // error and sends a frame without arguments.
    let payload = (!invalid_body).then(|| Value::Object(route));
    let response = daemon_request(&session(ctx), "network.route", payload, None)?;
    if !response.success {
        return Err(response_error(&response));
    }
    ctx.println(&format!("routed {} ({action})", args[0]));
    Ok(())
}

fn network_unroute(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let payload = args.first().map_or_else(|| json!({}), |p| json!({"pattern": p}));
    let response = daemon_request(&session(ctx), "network.unroute", Some(payload), None)?;
    if !response.success {
        return Err(response_error(&response));
    }
    ctx.println("unrouted");
    Ok(())
}

fn network_har(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let content = ctx.flags.string("content", "none");
    let output = ctx.flags.string("output", "");
    let payload = json!({"action": args[0], "content": content});
    let response = daemon_request(&session(ctx), "network.har", Some(payload), None)?;
    if !response.success {
        return Err(response_error(&response));
    }
    if args[0] == "start" {
        ctx.println("HAR capture started");
        return Ok(());
    }
    let data = data_map(&response);
    let raw = go_value::marshal_indent(data.get("har").unwrap_or(&Value::Null));
    if !output.is_empty() {
        return write_private_file(&output, raw.as_bytes());
    }
    ctx.println(&raw);
    Ok(())
}

fn write_private_file(path: &str, data: &[u8]) -> Handled {
    use std::io::Write as _;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    options
        .open(path)
        .and_then(|mut file| file.write_all(data))
        .map_err(|error| CliError::plain(format!("open {path}: {}", go_io_error(&error))))
}

fn go_io_error(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "no such file or directory".into(),
        std::io::ErrorKind::PermissionDenied => "permission denied".into(),
        _ => error.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Frame-only commands

fn no_autostart(ctx: &mut Ctx<'_>, command: &str) -> Handled {
    let response = request_no_autostart(&session(ctx), command)?;
    ctx.write_response(&response)
}

fn simple(ctx: &mut Ctx<'_>, command: &str, args: Option<Value>) -> Handled {
    let response = plain_request(&session(ctx), command, args)?;
    ctx.write_response(&response)
}

fn tab_new(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let url = args.first().cloned().unwrap_or_default();
    let label = ctx.flags.string("label", "");
    simple(ctx, "tab.new", Some(json!({"label": label, "url": url})))
}

/// Shared tail of the `structured -> text` commands.
fn text_or_envelope(ctx: &mut Ctx<'_>, response: &Response, render: impl FnOnce(&mut Ctx<'_>) -> Handled) -> Handled {
    if ctx.structured() {
        return ctx.write_response(response);
    }
    if !response.success {
        return Err(response_error(response));
    }
    render(ctx)
}

fn auth_login(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let mut request = Map::new();
    request.insert("entry".into(), args[0].clone().into());
    let url = ctx.flags.string("url", "");
    if !url.is_empty() {
        request.insert("url".into(), url.into());
    }
    let response = daemon_request(&session(ctx), "auth.login", Some(Value::Object(request)), None)?;
    text_or_envelope(ctx, &response, |ctx| {
        ctx.println("credentials entered; press enter or submit the form to log in");
        Ok(())
    })
}

fn pretty_data(ctx: &mut Ctx<'_>, response: &Response) -> Handled {
    let data = response.data.as_ref().map_or(Value::Null, go_value::to_go_any);
    ctx.println(&go_value::marshal_indent(&data));
    Ok(())
}

fn handoff(ctx: &mut Ctx<'_>) -> Handled {
    let reason = ctx.flags.string("reason", "");
    if reason.is_empty() {
        return Err(CliError::invalid_args("handoff requires --reason"));
    }
    let mut request = Map::new();
    request.insert("reason".into(), reason.into());
    let timeout = ctx.flags.string("timeout", "5m");
    if !timeout.is_empty() {
        request.insert("timeout".into(), timeout.into());
    }
    let response = daemon_request(&session(ctx), "handoff", Some(Value::Object(request)), None)?;
    text_or_envelope(ctx, &response, |ctx| pretty_data(ctx, &response))
}

fn journal(ctx: &mut Ctx<'_>, tail: bool) -> Handled {
    let session_name = session(ctx);
    let mut request = Map::new();
    request.insert("session".into(), session_name.clone().into());
    if tail {
        request.insert("lines".into(), int(ctx.flags.int("lines", 10)));
    }
    let command = if tail { "journal.tail" } else { "journal.show" };
    let response = daemon_request(&session_name, command, Some(Value::Object(request)), None)?;
    text_or_envelope(ctx, &response, |ctx| {
        for line in journal_lines(&response, "\t") {
            ctx.println(&line);
        }
        Ok(())
    })
}

pub fn journal_lines(response: &Response, class_prefix: &str) -> Vec<String> {
    let data = data_map(response);
    let (class, decider) = if class_prefix == "\t" { ("", "") } else { ("class=", "decider=") };
    data.get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|entry| {
            let fields = entry.as_object().cloned().unwrap_or_default();
            format!(
                "{}\t{}\t{class}{}\t{decider}{}\t{}",
                str_of(&fields, "timestamp"),
                str_of(&fields, "command"),
                str_of(&fields, "risk_class"),
                str_of(&fields, "decider"),
                str_of(&fields, "result")
            )
        })
        .collect()
}

fn oob_status(ctx: &mut Ctx<'_>) -> Handled {
    let response = daemon_request(&session(ctx), "oob.status", None, None)?;
    text_or_envelope(ctx, &response, |ctx| {
        let data = data_map(&response);
        if data.get("active").and_then(Value::as_bool) != Some(true) {
            ctx.println("no pending oob prompt");
            return Ok(());
        }
        let prompt = data.get("prompt").and_then(Value::as_object).cloned().unwrap_or_default();
        let line = format!(
            "{}\t{}\t{}",
            str_of(&prompt, "id"),
            str_of(&prompt, "kind"),
            str_of(&prompt, "reason")
        );
        ctx.println(&line);
        Ok(())
    })
}

fn policy_explain(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let payload = json!({
        "command": args[0],
        "url": ctx.flags.string("url", ""),
        "mode": ctx.flags.string("mode", ""),
    });
    let response = daemon_request(&session(ctx), "policy.explain", Some(payload), None)?;
    text_or_envelope(ctx, &response, |ctx| {
        let data = data_map(&response);
        let mut explanation = str_of(&data, "explanation");
        if explanation.is_empty() {
            let raw = response.data.as_ref().map_or(Value::Null, go_value::to_go_any);
            explanation = go_value::marshal_indent(&raw);
        }
        ctx.println(&explanation);
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// state / cookies / storage / set

fn state_named(ctx: &mut Ctx<'_>, command: &str, args: &[String], label: &str) -> Handled {
    let response = daemon_request(&session(ctx), command, Some(json!({"name": args[0]})), None)?;
    text_or_envelope(ctx, &response, |ctx| {
        ctx.println(&format!("{label} {}", go_quote(&args[0])));
        Ok(())
    })
}

fn state_show(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let response = daemon_request(&session(ctx), "state.show", Some(json!({"name": args[0]})), None)?;
    text_or_envelope(ctx, &response, |ctx| pretty_data(ctx, &response))
}

fn state_list(ctx: &mut Ctx<'_>) -> Handled {
    let response = daemon_request(&session(ctx), "state.list", None, None)?;
    text_or_envelope(ctx, &response, |ctx| {
        let data = data_map(&response);
        for name in data.get("states").and_then(Value::as_array).cloned().unwrap_or_default() {
            ctx.println(name.as_str().unwrap_or_default());
        }
        Ok(())
    })
}

fn state_clean(ctx: &mut Ctx<'_>) -> Handled {
    let older = ctx.flags.string("older-than", "");
    let mut payload = None;
    if !older.is_empty() {
        let days = older
            .parse::<i64>()
            .ok()
            .filter(|d| *d >= 0 && older.bytes().all(|b| b.is_ascii_digit() || b == b'-' || b == b'+'))
            .ok_or_else(|| CliError::plain(format!("invalid day count {}", go_quote(&older))))?;
        payload = Some(json!({"older_than_days": days}));
    }
    let response = daemon_request(&session(ctx), "state.clean", payload, None)?;
    if !response.success {
        return Err(response_error(&response));
    }
    let data = data_map(&response);
    let removed: Vec<Value> = data.get("removed").and_then(Value::as_array).cloned().unwrap_or_default();
    if ctx.structured() {
        // stateCleanPayload{Removed []string `json:"removed"`}: a missing list is null.
        let list = if data.contains_key("removed") && data.get("removed") != Some(&Value::Null) {
            Value::Array(removed.iter().filter(|v| v.is_string()).cloned().collect())
        } else {
            Value::Null
        };
        return ctx.write_ok(obj(vec![("removed", list)]));
    }
    ctx.println(&format!("removed {} expired state(s)", removed.len()));
    Ok(())
}

fn mask_secret(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    if value.len() <= 8 {
        return "••••".into();
    }
    let bytes = value.as_bytes();
    format!(
        "{}••••{}",
        String::from_utf8_lossy(&bytes[..4]),
        String::from_utf8_lossy(&bytes[bytes.len() - 4..])
    )
}

fn reveal_cookie(name: &str, reveal: &str) -> bool {
    reveal == "all" || reveal.split(',').any(|allowed| allowed.trim() == name)
}

fn cookies_list(ctx: &mut Ctx<'_>) -> Handled {
    let response = daemon_request(&session(ctx), "cookies.list", None, None)?;
    if !response.success {
        return Err(response_error(&response));
    }
    let reveal = ctx.flags.string("reveal", "");
    let data = data_map(&response);
    let cookies = data.get("cookies").and_then(Value::as_array).cloned().unwrap_or_default();
    if ctx.structured() {
        let mut typed = Vec::new();
        for cookie in &cookies {
            let mut fields = cookie.as_object().cloned().unwrap_or_default();
            let name = str_of(&fields, "name");
            if !reveal_cookie(&name, &reveal) {
                let masked = mask_secret(&str_of(&fields, "value"));
                fields.insert("value".into(), masked.into());
            }
            typed.push(typed_cookie(&fields));
        }
        let mut doc = TypedDoc::new();
        doc.string("origin", &str_of(&data, "origin"));
        doc.raw_array("cookies", typed);
        return ctx.write_typed(&doc);
    }
    for cookie in cookies {
        let fields = cookie.as_object().cloned().unwrap_or_default();
        let name = str_of(&fields, "name");
        let mut value = str_of(&fields, "value");
        if !reveal_cookie(&name, &reveal) {
            value = mask_secret(&value);
        }
        let mut flags = Vec::new();
        for (key, label) in [("secure", "secure"), ("http_only", "httpOnly"), ("session", "session")] {
            if fields.get(key).and_then(Value::as_bool) == Some(true) {
                flags.push(label);
            }
        }
        let flags = if flags.is_empty() { "-".to_owned() } else { flags.join(",") };
        ctx.println(&format!(
            "{name}\t{value}\t{}\t{}\t{flags}",
            str_of(&fields, "domain"),
            str_of(&fields, "path")
        ));
    }
    Ok(())
}

/// `engine.Cookie` re-marshalled field by field (typed order, zero values).
fn typed_cookie(fields: &Map<String, Value>) -> TypedDoc {
    let mut doc = TypedDoc::new();
    doc.string("name", &str_of(fields, "name"));
    doc.string("value", &str_of(fields, "value"));
    doc.string("domain", &str_of(fields, "domain"));
    doc.string("path", &str_of(fields, "path"));
    doc.float("expires", fields.get("expires").and_then(Value::as_f64).unwrap_or(0.0));
    doc.int("size", fields.get("size").and_then(Value::as_f64).map_or(0, |v| v as i64));
    for key in ["http_only", "secure", "session"] {
        doc.bool(key, fields.get(key).and_then(Value::as_bool).unwrap_or(false));
    }
    doc.string_omitempty("same_site", &str_of(fields, "same_site"));
    doc
}

fn cookies_set(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let curl = ctx.flags.string("curl", "");
    if !curl.is_empty() {
        return import_curl_cookies(ctx, &curl);
    }
    let cookie = json!({
        "name": args[0], "value": args[1],
        "domain": ctx.flags.string("domain", ""), "path": ctx.flags.string("path", "/"),
        "expires": 0.0, "size": 0, "http_only": ctx.flags.bool("http-only"),
        "secure": ctx.flags.bool("secure"), "session": false,
    });
    let payload = json!({"cookie": cookie, "url": ctx.flags.string("url", "")});
    let response = daemon_request(&session(ctx), "cookies.set", Some(payload), None)?;
    text_or_envelope(ctx, &response, |ctx| {
        ctx.println("ok");
        Ok(())
    })
}

fn cookies_clear(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let payload = json!({"name": args[0], "url": ctx.flags.string("url", "")});
    let response = daemon_request(&session(ctx), "cookies.clear", Some(payload), None)?;
    text_or_envelope(ctx, &response, |ctx| {
        ctx.println("ok");
        Ok(())
    })
}

fn import_curl_cookies(ctx: &mut Ctx<'_>, path: &str) -> Handled {
    let text = std::fs::read(path).map_err(|error| {
        CliError::plain(format!("open curl cookie jar: open {path}: {}", go_io_error(&error)))
    })?;
    let session_name = session(ctx);
    let (mut imported, mut skipped) = (0, 0);
    for line in String::from_utf8_lossy(&text).lines() {
        let line = line.trim();
        let fields: Vec<&str> = line.split_whitespace().collect();
        if line.is_empty() || line.starts_with('#') || fields.len() < 7 {
            skipped += 1;
            continue;
        }
        let expires: f64 = fields[4].parse().unwrap_or(0.0);
        let cookie = json!({
            "name": fields[5], "value": fields[6], "domain": fields[0], "path": fields[2],
            "expires": expires, "size": 0, "http_only": false,
            "secure": fields[3].eq_ignore_ascii_case("TRUE"), "session": false,
        });
        let response = daemon_request(&session_name, "cookies.set", Some(json!({"cookie": cookie, "url": ""})), None)?;
        if response.success {
            imported += 1;
        } else {
            skipped += 1;
        }
    }
    ctx.println(&format!("imported {imported} cookie(s), skipped {skipped}"));
    Ok(())
}

fn storage_get(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let response = daemon_request(&session(ctx), "storage.list", Some(json!({"kind": args[0]})), None)?;
    text_or_envelope(ctx, &response, |ctx| {
        let data = data_map(&response);
        let items = data.get("items").and_then(Value::as_object).cloned().unwrap_or_default();
        if args.len() == 2 {
            return match items.get(&args[1]).and_then(Value::as_str) {
                Some(value) => {
                    ctx.println(value);
                    Ok(())
                }
                None => Err(CliError::plain(format!(
                    "key {} not found in {} storage of {}",
                    go_quote(&args[1]),
                    args[0],
                    str_of(&data, "origin")
                ))),
            };
        }
        // Go iterates the map in its (random) order; sorted output is one
        // valid ordering and the only deterministic one.
        for (key, value) in items {
            ctx.println(&format!("{key}\t{}", value.as_str().unwrap_or_default()));
        }
        Ok(())
    })
}

fn storage_op(ctx: &mut Ctx<'_>, command: &str, payload: Value) -> Handled {
    let response = daemon_request(&session(ctx), command, Some(payload), None)?;
    text_or_envelope(ctx, &response, |ctx| {
        ctx.println("ok");
        Ok(())
    })
}

fn send_set(ctx: &mut Ctx<'_>, command: &str, request: Value) -> Handled {
    let response = daemon_request(&session(ctx), command, Some(request), None)?;
    if !response.success {
        return Err(response_error(&response));
    }
    if ctx.structured() {
        return ctx.write_response(&response);
    }
    ctx.println("ok");
    Ok(())
}

fn go_parse_float_error(label: &str, value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let trimmed = lower.trim_start_matches(['+', '-']);
    if matches!(trimmed, "inf" | "infinity" | "nan") {
        return None;
    }
    match value.parse::<f64>() {
        Ok(v) if v.is_infinite() => Some(format!(
            "{label}strconv.ParseFloat: parsing {}: value out of range",
            go_quote(value)
        )),
        Ok(_) if !value.contains('_') && value.trim() == value => None,
        _ => Some(format!(
            "{label}strconv.ParseFloat: parsing {}: invalid syntax",
            go_quote(value)
        )),
    }
}

fn set_viewport(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let width = parse_int10(&args[0]).map_err(|reason| {
        CliError::invalid_args(format!("width: strconv.ParseInt: parsing {}: {reason}", go_quote(&args[0])))
    })?;
    let height = parse_int10(&args[1]).map_err(|reason| {
        CliError::invalid_args(format!("height: strconv.ParseInt: parsing {}: {reason}", go_quote(&args[1])))
    })?;
    let mut scale = 0.0;
    if args.len() == 3 {
        if let Some(message) = go_parse_float_error("scale: ", &args[2]) {
            return Err(CliError::invalid_args(message));
        }
        scale = args[2].parse().unwrap_or(f64::NAN);
    }
    let request = json!({"width": width, "height": height, "scale": scale_value(scale)});
    send_set(ctx, "set.viewport", request)
}

fn scale_value(value: f64) -> Value {
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}

fn set_geo(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    if let Some(message) = go_parse_float_error("latitude: ", &args[0]) {
        return Err(CliError::plain(message));
    }
    if let Some(message) = go_parse_float_error("longitude: ", &args[1]) {
        return Err(CliError::plain(message));
    }
    let latitude: f64 = args[0].parse().unwrap_or(f64::NAN);
    let longitude: f64 = args[1].parse().unwrap_or(f64::NAN);
    send_set(
        ctx,
        "set.geo",
        json!({"latitude": scale_value(latitude), "longitude": scale_value(longitude)}),
    )
}

fn set_offline(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let mut offline = true;
    if let Some(arg) = args.first() {
        match arg.to_lowercase().as_str() {
            "on" => offline = true,
            "off" => offline = false,
            _ => {
                return Err(CliError::invalid_args(format!(
                    "offline expects on or off, got {}",
                    go_quote(arg)
                )));
            }
        }
    }
    send_set(ctx, "set.offline", json!({"offline": offline}))
}

fn set_headers(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let wrap = |detail: String| CliError::invalid_args(format!("headers must be a JSON object: {detail}"));
    let scan = crate::gojson::scan(&args[0]).map_err(wrap)?;
    let mut headers = Map::new();
    match scan.top {
        crate::gojson::Kind::Null => {}
        crate::gojson::Kind::Object => {
            if let Some((key, kind)) = scan
                .members
                .iter()
                .find(|(_, kind)| *kind != crate::gojson::Kind::String)
            {
                return Err(wrap(format!(
                    "json: cannot unmarshal {} into Go struct field .{key} of type string",
                    kind.name()
                )));
            }
            if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&args[0]) {
                headers = map;
            }
        }
        other => {
            return Err(wrap(format!(
                "json: cannot unmarshal {} into Go value of type map[string]string",
                other.name()
            )));
        }
    }
    if headers.is_empty() {
        return Err(CliError::invalid_args("headers object must not be empty"));
    }
    send_set(ctx, "set.headers", json!({"headers": headers}))
}

fn set_media(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let dark = match args[0].to_lowercase().as_str() {
        "dark" => true,
        "light" => false,
        _ => {
            return Err(CliError::invalid_args(format!(
                "media expects dark or light, got {}",
                go_quote(&args[0])
            )));
        }
    };
    send_set(ctx, "set.media", json!({"dark": dark}))
}

fn set_device(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    if ctx.flags.bool("list") {
        return Err(CliError::plain("set device --list is not implemented natively in the Rust CLI yet"));
    }
    send_set(ctx, "set.device", json!({"name": args[0]}))
}

// ---------------------------------------------------------------------------
// a11y / diff / flow record

fn a11y(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let tags = ctx.flags.string("tags", "");
    let tag_list: Vec<Value> = if tags.trim().is_empty() {
        Vec::new()
    } else {
        tags.split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| Value::String(t.to_owned()))
            .collect()
    };
    let tag_value = if tag_list.is_empty() { Value::Null } else { Value::Array(tag_list) };
    let session_name = session(ctx);
    if args.len() == 1 {
        plain_request(&session_name, "open", Some(json!({"url": args[0]})))?;
    }
    let payload = obj(vec![("tags", tag_value), ("selector", ctx.flags.string("selector", "").into())]);
    let response = plain_request(&session_name, "a11y", Some(payload))?;
    ctx.write_response(&response)
}

/// `daemon.NewClient(ClientOptions{Session: ...})` has no socket path, so the
/// request fails before anything is sent.
fn missing_socket() -> CliError {
    CliError::plain("socket path is required")
}

fn read_baseline(path: &str) -> Result<String, CliError> {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|error| {
            CliError::plain(format!(
                "read baseline {}: open {path}: {}",
                go_quote(path),
                go_io_error(&error)
            ))
        })
}

fn diff_snapshot(ctx: &mut Ctx<'_>) -> Handled {
    let baseline = ctx.flags.string("baseline", "");
    if !baseline.is_empty() {
        let text = read_baseline(&baseline)?;
        if let Err(detail) = crate::gojson::scan(&text) {
            return Err(CliError::plain(format!(
                "decode baseline {}: {detail}",
                go_quote(&baseline)
            )));
        }
    }
    Err(missing_socket())
}

fn diff_screenshot(ctx: &mut Ctx<'_>) -> Handled {
    let baseline = ctx.flags.string("baseline", "");
    if baseline.is_empty() {
        return Err(CliError::invalid_args("--baseline <png> is required"));
    }
    std::fs::read(&baseline).map_err(|error| {
        CliError::plain(format!(
            "read baseline {}: open {baseline}: {}",
            go_quote(&baseline),
            go_io_error(&error)
        ))
    })?;
    Err(missing_socket())
}

fn diff_url(_ctx: &mut Ctx<'_>, _args: &[String]) -> Handled {
    Err(missing_socket())
}

fn flow_record(_ctx: &mut Ctx<'_>, _args: &[String]) -> Handled {
    Err(missing_socket())
}

// ---------------------------------------------------------------------------
// Typed (ordered) payloads

/// A Go struct payload: fields keep declaration order in JSON and use the
/// lower-cased Go field name in YAML.
pub struct TypedDoc {
    fields: Vec<(String, TypedValue)>,
}

enum TypedValue {
    /// String field; `omit` mirrors `json:",omitempty"` (YAML always prints it).
    Str { value: String, omit: bool },
    Int(i64),
    Float(f64),
    Bool(bool),
    Docs(Vec<TypedDoc>),
}

impl TypedDoc {
    pub fn new() -> Self {
        Self { fields: Vec::new() }
    }

    pub fn string(&mut self, key: &str, value: &str) {
        self.fields.push((key.to_owned(), TypedValue::Str { value: value.to_owned(), omit: false }));
    }

    pub fn string_omitempty(&mut self, key: &str, value: &str) {
        self.fields.push((key.to_owned(), TypedValue::Str { value: value.to_owned(), omit: true }));
    }

    pub fn int(&mut self, key: &str, value: i64) {
        self.fields.push((key.to_owned(), TypedValue::Int(value)));
    }

    pub fn float(&mut self, key: &str, value: f64) {
        self.fields.push((key.to_owned(), TypedValue::Float(value)));
    }

    pub fn bool(&mut self, key: &str, value: bool) {
        self.fields.push((key.to_owned(), TypedValue::Bool(value)));
    }

    pub fn raw_array(&mut self, key: &str, docs: Vec<TypedDoc>) {
        self.fields.push((key.to_owned(), TypedValue::Docs(docs)));
    }

    pub fn to_json(&self) -> String {
        let parts: Vec<String> = self
            .fields
            .iter()
            .filter_map(|(key, value)| {
                let rendered = match value {
                    TypedValue::Str { value, omit } => {
                        if *omit && value.is_empty() {
                            return None;
                        }
                        go_value::marshal_plain(&Value::String(value.clone()))
                    }
                    TypedValue::Int(number) => number.to_string(),
                    TypedValue::Float(number) => go_value::float_json(*number),
                    TypedValue::Bool(flag) => flag.to_string(),
                    TypedValue::Docs(docs) if docs.is_empty() => "null".to_owned(),
                    TypedValue::Docs(docs) => {
                        format!("[{}]", docs.iter().map(TypedDoc::to_json).collect::<Vec<_>>().join(","))
                    }
                };
                Some(format!("\"{key}\":{rendered}"))
            })
            .collect();
        format!("{{{}}}", parts.join(","))
    }

    fn yaml(&self, indent: usize, out: &mut String, first_prefix: Option<&str>) {
        let pad = " ".repeat(indent);
        for (index, (key, value)) in self.fields.iter().enumerate() {
            let lead = match (index, first_prefix) {
                (0, Some(prefix)) => prefix.to_owned(),
                _ => pad.clone(),
            };
            let name = key.replace('_', "");
            match value {
                TypedValue::Str { value, .. } => {
                    out.push_str(&format!("{lead}{name}: {}\n", yaml_string(value)));
                }
                TypedValue::Int(number) => out.push_str(&format!("{lead}{name}: {number}\n")),
                TypedValue::Float(number) => {
                    out.push_str(&format!("{lead}{name}: {}\n", go_value::float_v(*number)));
                }
                TypedValue::Bool(flag) => out.push_str(&format!("{lead}{name}: {flag}\n")),
                TypedValue::Docs(docs) if docs.is_empty() => out.push_str(&format!("{lead}{name}: []\n")),
                TypedValue::Docs(docs) => {
                    out.push_str(&format!("{lead}{name}:\n"));
                    for doc in docs {
                        doc.yaml(indent + 6, out, Some(&format!("{pad}    - ")));
                    }
                }
            }
        }
    }
}

fn yaml_string(value: &str) -> String {
    let mut envelope = Map::new();
    envelope.insert("v".into(), Value::String(value.to_owned()));
    let rendered = Envelope::ok(Value::Object(envelope), Vec::new())
        .render(symbrowse_core::output::Format::Yaml)
        .unwrap_or_default();
    rendered
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("v: ").map(str::to_owned))
        .unwrap_or_else(|| value.to_owned())
}

impl Ctx<'_> {
    /// Writes `output.OK(typedStruct)` in the selected format.
    pub fn write_typed(&mut self, doc: &TypedDoc) -> Handled {
        match self.format()? {
            symbrowse_core::output::Format::Json => {
                let text = format!("{{\"success\":true,\"data\":{}}}", doc.to_json());
                self.println(&text);
            }
            symbrowse_core::output::Format::Yaml => {
                let mut out = String::from("success: true\ndata:\n");
                doc.yaml(4, &mut out, None);
                out.push_str("warnings: []\nerror: null\n");
                self.print(&out);
            }
            symbrowse_core::output::Format::Text => {
                return Err(CliError::plain("typed payload has no text rendering"));
            }
        }
        Ok(())
    }
}
