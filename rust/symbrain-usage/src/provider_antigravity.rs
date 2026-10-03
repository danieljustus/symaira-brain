//! Bounded local Antigravity observations and request walk.
use super::{Provider, Request, UsageError, request_for, status_error};
use crate::{
    UsageSnapshot,
    parser::parse_snapshot,
    transport::{Cancellation, Transport},
};
use chrono::Utc;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AntigravityCandidate {
    pub(super) pid: u32,
    pub(super) csrf_token: Option<String>,
}

pub(super) fn parse_antigravity_candidates(process_list: &str) -> Vec<AntigravityCandidate> {
    let mut candidates = Vec::new();
    for line in process_list.split('\n') {
        let line = line.trim_start_matches(' ');
        let Some((pid, command)) = line.split_once(' ') else {
            continue;
        };
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        let command = command.trim_start_matches(' ');
        let lower = command.to_ascii_lowercase();
        let server = (lower.contains("language_server") || lower.contains("language-server"))
            && (lower.contains("antigravity")
                || (lower.contains("--app_data_dir") && command.contains("antigravity")));
        let cli = lower.contains("/agy")
            || lower.contains("antigravity-cli")
            || lower.contains("antigravity_cli");
        if !server && !cli {
            continue;
        }
        let csrf_token = command.find("--csrf_token").and_then(|index| {
            command[index + "--csrf_token".len()..]
                .trim_start_matches(' ')
                .split(' ')
                .next()
                .filter(|token| !token.is_empty())
                .map(str::to_owned)
        });
        candidates.push(AntigravityCandidate { pid, csrf_token });
    }
    candidates
}

pub(super) fn parse_antigravity_ports(port_list: &str) -> Vec<u32> {
    let mut ports = Vec::new();
    for line in port_list
        .split('\n')
        .filter(|line| line.contains("(LISTEN)"))
    {
        let bytes = line.as_bytes();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte != b':' {
                continue;
            }
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() && end - start < 5 {
                end += 1;
            }
            if end == start || (end < bytes.len() && !bytes[end].is_ascii_whitespace()) {
                continue;
            }
            let Ok(port_text) = std::str::from_utf8(&bytes[start..end]) else {
                continue;
            };
            let Ok(port) = port_text.parse::<u32>() else {
                continue;
            };
            if !ports.contains(&port) {
                ports.push(port);
            }
            break;
        }
    }
    ports
}

pub(super) fn fetch_antigravity_from_observations(
    provider: &Provider,
    transport: &Arc<dyn Transport>,
    cancel: &Cancellation,
    process_list: &str,
    mut ports_for_pid: impl FnMut(u32) -> Option<String>,
) -> Result<UsageSnapshot, UsageError> {
    let candidates = parse_antigravity_candidates(process_list);
    if candidates.is_empty() {
        return Err(UsageError::not_running());
    }
    let mut last_error = UsageError::not_running();
    for candidate in candidates {
        // Go refuses to pass PIDs above its deliberately bounded probe limit
        // to lsof; keep the same ceiling before invoking the local tool.
        if candidate.pid > i32::MAX as u32 {
            continue;
        }
        let Some(port_output) = ports_for_pid(candidate.pid) else {
            continue;
        };
        for port in parse_antigravity_ports(&port_output) {
            match fetch_antigravity_from_port(
                provider,
                transport,
                cancel,
                port,
                candidate.csrf_token.as_deref(),
            ) {
                Ok(snapshot) => return Ok(snapshot),
                Err(error) => last_error = error,
            }
        }
    }
    Err(last_error)
}

fn fetch_antigravity_from_port(
    provider: &Provider,
    transport: &Arc<dyn Transport>,
    cancel: &Cancellation,
    port: u32,
    csrf_token: Option<&str>,
) -> Result<UsageSnapshot, UsageError> {
    let connect = antigravity_request(provider, port, "GetUnleashData", csrf_token);
    let response = transport
        .request_with_cancel(connect, cancel)
        .map_err(|_| {
            UsageError::exact(format!(
                "Antigravity probe failed: connect probe failed on port {port}"
            ))
        })?;
    if !(200..300).contains(&response.status) {
        return Err(UsageError::exact(format!(
            "Antigravity probe failed: connect probe failed on port {port}"
        )));
    }

    let mut last_error = UsageError::parse(&provider.id, "all quota endpoints failed");
    for endpoint in [
        "RetrieveUserQuotaSummary",
        "GetUserStatus",
        "GetCommandModelConfigs",
    ] {
        let request = antigravity_request(provider, port, endpoint, csrf_token);
        match transport.request_with_cancel(request, cancel) {
            Ok(response) if (200..300).contains(&response.status) => {
                match parse_snapshot(&provider.id, "local", &response.body, Utc::now()) {
                    Ok(snapshot) => return Ok(snapshot),
                    Err(_) => {
                        last_error = UsageError::exact(format!(
                            "Antigravity returned an unreadable response: {endpoint} returned unreadable data"
                        ));
                    }
                }
            }
            Ok(response) => {
                last_error = status_error(&provider.id, response.status, &response.headers);
            }
            Err(error) => last_error = UsageError::transport(&provider.id, &error),
        }
    }
    Err(last_error)
}

fn antigravity_request(
    provider: &Provider,
    port: u32,
    endpoint: &str,
    csrf_token: Option<&str>,
) -> Request {
    let mut request = request_for(
        provider,
        "local",
        "",
        Some(&format!("https://127.0.0.1:{port}/{endpoint}")),
    );
    if endpoint == "GetUnleashData" {
        request.body = None;
        request.headers.remove("Accept");
        request.headers.remove("Content-Type");
        request
            .headers
            .insert("Connect-Protocol-Version".into(), "1".into());
    }
    if let Some(token) = csrf_token {
        request
            .headers
            .insert("X-Codeium-Csrf-Token".into(), token.into());
    }
    request
}
