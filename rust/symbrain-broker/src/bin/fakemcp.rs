//! Test-only MCP child used by broker integration tests.

#![deny(unsafe_code)]

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Deserialize)]
struct ToolDef {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    input_schema: Option<Value>,
    #[serde(default)]
    behavior: String,
    #[serde(default)]
    annotations: Option<Value>,
}

fn default_tools() -> Vec<ToolDef> {
    [
        ("echo", "echoes call arguments back", "echo"),
        ("crash", "exits without responding", "crash"),
        ("slow", "sleeps before responding", "slow"),
        ("toolerror", "returns a tool-level error", "toolerror"),
    ]
    .into_iter()
    .map(|(name, description, behavior)| ToolDef {
        name: name.to_string(),
        description: description.to_string(),
        input_schema: None,
        behavior: behavior.to_string(),
        annotations: None,
    })
    .collect()
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).is_some_and(|arg| arg == "--hang-child") {
        if let Ok(marker) = std::env::var("FAKEMCP_DESCENDANT_MARKER") {
            record_pid(&marker);
        }
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }
    if args.get(1).is_some_and(|arg| arg == "version")
        && args.get(2).is_some_and(|arg| arg == "--json")
    {
        println!("{{\"version\":\"{}\"}}", version());
        return;
    }

    if let Ok(message) = std::env::var("FAKEMCP_STDERR") {
        eprintln!("{message}");
    }

    if let Ok(marker) = std::env::var("FAKEMCP_SPAWN_MARKER") {
        record_pid(&marker);
    }
    if std::env::var_os("FAKEMCP_DESCENDANT_MARKER").is_some() {
        spawn_descendant();
    }

    let tools = tools();
    let by_name = tools
        .iter()
        .map(|tool| (tool.name.clone(), tool.clone()))
        .collect::<BTreeMap<_, _>>();
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        handle(
            &mut stdout,
            &tools,
            &by_name,
            id,
            method,
            request.get("params"),
        );
    }
}

fn handle(
    writer: &mut dyn Write,
    tools: &[ToolDef],
    by_name: &BTreeMap<String, ToolDef>,
    id: Value,
    method: &str,
    params: Option<&Value>,
) {
    match method {
        "initialize" => {
            sleep_env("FAKEMCP_INIT_DELAY_MS", 0);
            let protocol = std::env::var("FAKEMCP_PROTOCOL_VERSION")
                .unwrap_or_else(|_| "2024-11-05".to_string());
            write_result(
                writer,
                id,
                json!({
                    "protocolVersion": protocol,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "fakemcp", "version": version()}
                }),
            );
        }
        "tools/list" => {
            let listed = tools
                .iter()
                .map(|tool| {
                    let mut value = json!({
                        "name": tool.name,
                        "description": tool.description,
                    });
                    if let Some(schema) = &tool.input_schema {
                        value["inputSchema"] = schema.clone();
                    }
                    if let Some(annotations) = &tool.annotations {
                        value["annotations"] = annotations.clone();
                    }
                    value
                })
                .collect::<Vec<_>>();
            write_result(writer, id, json!({"tools": listed}));
        }
        "tools/call" => handle_tool_call(writer, by_name, id, params),
        _ => write_error(writer, id, -32601, &format!("Method not found: {method}")),
    }
}

fn handle_tool_call(
    writer: &mut dyn Write,
    tools: &BTreeMap<String, ToolDef>,
    id: Value,
    params: Option<&Value>,
) {
    let name = params
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(tool) = tools.get(name) else {
        write_error(writer, id, -32601, &format!("Unknown tool: {name}"));
        return;
    };
    let arguments = params
        .and_then(|value| value.get("arguments"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    match tool.behavior.as_str() {
        "crash" => std::process::exit(1),
        "slow" => {
            sleep_env("FAKEMCP_SLOW_MS", 2000);
            write_tool_text(writer, id, &arguments.to_string(), false);
        }
        "toolerror" => write_tool_text(
            writer,
            id,
            &format!("toolerror: intentional failure for {name}"),
            true,
        ),
        _ => write_tool_text(writer, id, &arguments.to_string(), false),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn write_result(writer: &mut dyn Write, id: Value, result: Value) {
    write_line(
        writer,
        &json!({"jsonrpc": "2.0", "id": id, "result": result}),
    );
}

fn write_tool_text(writer: &mut dyn Write, id: Value, text: &str, is_error: bool) {
    write_result(
        writer,
        id,
        json!({"content": [{"type": "text", "text": text}], "isError": is_error}),
    );
}

#[allow(clippy::needless_pass_by_value)]
fn write_error(writer: &mut dyn Write, id: Value, code: i64, message: &str) {
    write_line(
        writer,
        &json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    );
}

fn write_line(writer: &mut dyn Write, value: &Value) {
    if serde_json::to_writer(&mut *writer, value).is_ok() {
        let _ = writer.write_all(b"\n");
        let _ = writer.flush();
    }
}

fn tools() -> Vec<ToolDef> {
    std::env::var("FAKEMCP_TOOLS")
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(default_tools)
}

fn version() -> String {
    std::env::var("FAKEMCP_VERSION").unwrap_or_else(|_| "0.0.0-fakemcp".to_string())
}

fn sleep_env(name: &str, fallback_ms: u64) {
    let milliseconds = std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(fallback_ms);
    if milliseconds > 0 {
        std::thread::sleep(Duration::from_millis(milliseconds));
    }
}

fn record_pid(path: &str) {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    if let Ok(mut file) = options.open(path) {
        let _ = writeln!(file, "{}", std::process::id());
    }
}

fn spawn_descendant() {
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let _ = Command::new(executable)
        .arg("--hang-child")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}
