use super::{Provider, Request};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{Read, Seek, SeekFrom};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub(super) fn request_for(
    provider: &Provider,
    source: &str,
    credential: &str,
    argument: Option<&str>,
) -> Request {
    let id = provider.id.as_str();
    let (method, url, body) = match (id, source) {
        ("claude", "api") => ("GET", "https://api.anthropic.com/v1/organizations/cost_report?bucket_width=1d&limit=7".into(), None),
        ("claude", _) => ("GET", "https://api.anthropic.com/api/oauth/usage".into(), None),
        ("codex", _) => ("GET", "https://chatgpt.com/backend-api/wham/usage".into(), None),
        ("copilot", _) => ("GET", "https://api.github.com/copilot_internal/user".into(), None),
        ("cursor", _) => ("GET", "https://cursor.com/api/usage-summary".into(), None),
        ("kimi", "web") => ("POST", "https://www.kimi.com/apiv2/kimi.gateway.billing.v1.BillingService/GetUsages".into(), None),
        ("kimi", _) => ("GET", format!("{}/coding/v1/usages", validated_base(provider.base_url.as_deref().unwrap_or("https://api.kimi.com"), "https://api.kimi.com")), None),
        ("moonshot", _) => ("GET", format!("https://api.moonshot.{}/v1/users/me/balance", provider.region), None),
        ("nous", _) => ("GET", format!("{}/api/oauth/account", validated_base(provider.base_url.as_deref().unwrap_or("https://portal.nousresearch.com"), "https://portal.nousresearch.com")), None),
        ("openrouter", _) => ("GET", format!("{}/auth/key", validated_base(provider.base_url.as_deref().unwrap_or("https://openrouter.ai/api/v1"), "https://openrouter.ai/api/v1")), None),
        ("opencode", "workspace_get") => ("GET", "https://opencode.ai/_server?id=def39973159c7f0483d8793a822b8dbb10d067e12c65455fcb4608459ba0234f".into(), None),
        ("opencode", "web_post") => ("POST", "https://opencode.ai/_server".into(), Some(format!("[\"{}\"]", argument.unwrap_or("")))),
        ("opencode", _) => ("GET", format!("https://opencode.ai/_server?args=%5B%22{}%22%5D&id=7abeebee372f304e050aaaf92be863f4a86490e382f8c79db68fd94040d691b4", encode_query_arg(argument.unwrap_or(""))), None),
        ("antigravity", "local") => {
            let raw = argument.unwrap_or("https://127.0.0.1:0/RetrieveUserQuotaSummary");
            let (base, endpoint) = raw.rsplit_once('/').unwrap_or((raw, "RetrieveUserQuotaSummary"));
            ("POST", format!("{base}/exa.language_server_pb.LanguageServerService/{endpoint}"), Some("{}".into()))
        }
        _ => ("GET", "https://127.0.0.1:0/usage".into(), None),
    };
    let mut headers = BTreeMap::new();
    if !(id == "claude" && source != "api") {
        headers.insert("Accept".into(), "application/json".into());
    }
    if !credential.is_empty() {
        if id == "cursor" || id == "opencode" {
            headers.insert("Cookie".into(), credential.into());
        } else {
            headers.insert("Authorization".into(), format!("Bearer {credential}"));
        }
    }
    if id == "claude" && source != "api" {
        headers.insert("anthropic-beta".into(), "oauth-2025-04-20".into());
    }
    if id == "openrouter" {
        headers.insert("X-Title".into(), "symbrain".into());
    }
    if id == "opencode" {
        let server_id = match source {
            "workspace_get" => "def39973159c7f0483d8793a822b8dbb10d067e12c65455fcb4608459ba0234f",
            _ => "7abeebee372f304e050aaaf92be863f4a86490e382f8c79db68fd94040d691b4",
        };
        headers.insert("X-Server-Id".into(), server_id.into());
        headers.insert(
            "X-Server-Instance".into(),
            "server-fn:00000000-0000-0000-0000-000000000000".into(),
        );
        headers.insert(
            "User-Agent".into(),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36".into(),
        );
        headers.insert("Origin".into(), "https://opencode.ai".into());
        headers.insert(
            "Referer".into(),
            if source == "workspace_get" {
                "https://opencode.ai".into()
            } else {
                format!(
                    "https://opencode.ai/workspace/{}/billing",
                    argument.unwrap_or("")
                )
            },
        );
        headers.insert(
            "Accept".into(),
            "text/javascript, application/json;q=0.9, */*;q=0.8".into(),
        );
    }
    if body.is_some() {
        headers.insert("Content-Type".into(), "application/json".into());
    }
    Request {
        provider_id: id.into(),
        method: method.into(),
        url,
        headers,
        body: body.map(String::into_bytes),
    }
}

fn encode_query_arg(value: &str) -> String {
    value.bytes().fold(String::new(), |mut out, byte| {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
        out
    })
}

pub(crate) fn validated_base(raw: &str, fallback: &str) -> String {
    if is_trusted_https(raw) {
        raw.trim_end_matches('/').to_string()
    } else {
        fallback.into()
    }
}
pub(crate) fn trusted_https_url(raw: &str, allow_loopback: bool) -> bool {
    if raw.is_empty() || raw.trim() != raw || raw.bytes().any(|byte| byte < 0x20 || byte == 0x7f) {
        return false;
    }
    let Some(authority_and_path) = raw.strip_prefix("https://") else {
        return false;
    };
    let authority_end = authority_and_path
        .find(['/', '?', '#'])
        .unwrap_or(authority_and_path.len());
    if authority_and_path.as_bytes().get(authority_end) == Some(&b'#') {
        return false;
    }
    let authority = &authority_and_path[..authority_end];
    let remainder = &authority_and_path[authority_end..];
    if authority.is_empty()
        || authority.contains('@')
        || authority.bytes().any(|byte| byte <= b' ' || byte == 0x7f)
        || authority.contains('%')
    {
        return false;
    }
    let path = remainder.split('?').next().unwrap_or(remainder);
    if !valid_percent_encoding(path) {
        return false;
    }
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        let Some(end) = bracketed.find(']') else {
            return false;
        };
        let host = &bracketed[..end];
        if host.parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        let remainder = &bracketed[end + 1..];
        if let Some(port) = remainder.strip_prefix(':') {
            if !valid_port(port) {
                return false;
            }
        } else if !remainder.is_empty() {
            return false;
        }
        host
    } else {
        if authority.matches(':').count() > 1 {
            return false;
        }
        if let Some((host, port)) = authority.rsplit_once(':') {
            if !valid_port(port) {
                return false;
            }
            host
        } else {
            authority
        }
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || host == "localhost"
        || host.ends_with(".localhost")
        || host == "local"
        || host.as_bytes().ends_with(b".local")
        || host.ends_with(".internal")
        || host.ends_with(".home.arpa")
    {
        return false;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return if allow_loopback {
            ip.is_loopback()
        } else {
            !is_private_ip(ip)
        };
    }
    !allow_loopback
}

fn valid_port(port: &str) -> bool {
    port.parse::<u16>().is_ok_and(|value| value != 0)
}

fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => {
            let octets = ip.octets();
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || octets[0] == 0
                || octets[0] == 100 && (64..=127).contains(&octets[1])
                || octets[0] == 192 && octets[1] == 0 && (octets[2] == 0 || octets[2] == 2)
                || octets[0] == 198
                    && ((18..=19).contains(&octets[1]) || octets[1] == 51 && octets[2] == 100)
                || octets[0] == 203 && octets[1] == 0 && octets[2] == 113
        }
        std::net::IpAddr::V6(ip) => {
            let octets = ip.octets();
            let segments = ip.segments();
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_unicast_link_local()
                || segments[0] & 0xfe00 == 0xfc00
                || (segments[..5] == [0, 0, 0, 0, 0]
                    && segments[5] == 0xffff
                    && is_private_ip(std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                        octets[12], octets[13], octets[14], octets[15],
                    ))))
        }
    }
}

fn valid_percent_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

fn is_trusted_https(raw: &str) -> bool {
    trusted_https_url(raw, false)
}

pub(super) fn command_output_with_cancel(
    cancel: &crate::Cancellation,
    command: &str,
    args: &[&str],
) -> Option<String> {
    const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
    let mut output = tempfile::NamedTempFile::new().ok()?;
    let child_stdout = output.as_file().try_clone().ok()?;
    let mut command_line = Command::new(command);
    command_line
        .args(args)
        .stdout(Stdio::from(child_stdout))
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command_line.process_group(0);
    }
    let mut child = command_line.spawn().ok()?;
    let budget = cancel
        .remaining()
        .unwrap_or(PROBE_TIMEOUT)
        .min(PROBE_TIMEOUT);
    let deadline = Instant::now() + budget;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if !cancel.is_cancelled() && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(2));
            }
            Ok(None) | Err(_) => {
                terminate_probe(&mut child);
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    output.as_file_mut().seek(SeekFrom::Start(0)).ok()?;
    let mut data = Vec::new();
    output
        .as_file_mut()
        .take(64 * 1024 + 1)
        .read_to_end(&mut data)
        .ok()?;
    (data.len() <= 64 * 1024).then(|| String::from_utf8_lossy(&data).into_owned())
}

fn terminate_probe(child: &mut std::process::Child) {
    #[cfg(unix)]
    if let Ok(raw_pid) = i32::try_from(child.id())
        && let Some(pid) = rustix::process::Pid::from_raw(raw_pid)
    {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub(super) fn normalize_workspace(raw: &str) -> String {
    raw.split_whitespace()
        .find(|x| x.starts_with("wrk_"))
        .map(|x| {
            x.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .to_string()
        })
        .unwrap_or_default()
}
pub(super) fn extract_workspace(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/workspaces/0/id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| {
            let workspace = normalize_workspace(&String::from_utf8_lossy(body));
            (!workspace.is_empty()).then_some(workspace)
        })
}

#[cfg(test)]
#[path = "provider_requests_tests.rs"]
mod tests;
