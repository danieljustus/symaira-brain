use super::{Provider, Request};
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
        ("opencode", "workspace_post") => ("POST", "https://opencode.ai/_server".into(), Some("[]".into())),
        ("opencode", "web_post") => ("POST", "https://opencode.ai/_server".into(), Some(go_json_string_array(argument.unwrap_or("")))),
        ("opencode", _) => ("GET", format!("https://opencode.ai/_server?args={}&id=7abeebee372f304e050aaaf92be863f4a86490e382f8c79db68fd94040d691b4", encode_query_arg(&go_json_string_array(argument.unwrap_or("")))), None),
        ("antigravity", "local") => {
            let raw = argument.unwrap_or("https://127.0.0.1:0/RetrieveUserQuotaSummary");
            let (base, endpoint) = raw.rsplit_once('/').unwrap_or((raw, "RetrieveUserQuotaSummary"));
            ("POST", format!("{base}/exa.language_server_pb.LanguageServerService/{endpoint}"), Some("{}".into()))
        }
        _ => ("GET", "https://127.0.0.1:0/usage".into(), None),
    };
    let mut headers = BTreeMap::new();
    if id != "claude" {
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
        // The shipped code sets this through Go's `http.Header.Set`, which
        // canonicalizes the name on the wire (`Anthropic-Beta`). Header names
        // are case-insensitive, so the port keeps the source spelling and the
        // oracle tests compare names case-insensitively.
        headers.insert("anthropic-beta".into(), "oauth-2025-04-20".into());
    }
    if id == "kimi" && source != "web" {
        // Identity metadata the Kimi Code CLI sends with its token. The
        // shipped implementation also sends `X-Msh-Os-Version` and
        // `X-Msh-Device-Model`, filled with the *Go runtime version* (see
        // internal/usage/kimi.go: it reports no OS fact and the endpoint does
        // not gate on it), so the port omits those two instead of inventing a
        // value; scripts/usage-oracle records the difference.
        headers.insert("X-Msh-Platform".into(), platform_label().into());
        let host = hostname();
        if !host.is_empty() {
            headers.insert("X-Msh-Device-Name".into(), host);
        }
        if let Some(device) = provider.device_id.as_deref().filter(|v| !v.is_empty()) {
            headers.insert("X-Msh-Device-Id".into(), device.into());
        }
    }
    if id == "openrouter" {
        headers.insert("X-Title".into(), "symbrain".into());
    }
    if id == "opencode" {
        let server_id = match source {
            "workspace_get" | "workspace_post" => {
                "def39973159c7f0483d8793a822b8dbb10d067e12c65455fcb4608459ba0234f"
            }
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
            if matches!(source, "workspace_get" | "workspace_post") {
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

/// The platform label the Kimi Code CLI uses in `X-Msh-Platform`. The shipped
/// implementation maps Go's `darwin` to `macos`; Rust's OS constant already
/// reads `macos`, so no mapping is needed.
fn platform_label() -> &'static str {
    std::env::consts::OS
}

/// The machine's host name, for `X-Msh-Device-Name`. Returns an empty string
/// when the platform cannot report it; the header is then left out.
#[cfg(unix)]
fn hostname() -> String {
    rustix::system::uname()
        .nodename()
        .to_string_lossy()
        .into_owned()
}

#[cfg(not(unix))]
fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

fn encode_query_arg(value: &str) -> String {
    value.bytes().fold(String::new(), |mut out, byte| {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else if byte == b' ' {
            // Go's url.QueryEscape (used by url.Values.Encode) renders a space
            // as '+', not %20.
            out.push('+');
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
        out
    })
}

/// Serializes one request argument the way the shipped code's
/// `json.Marshal([]any{arg})` does for the `args` query value and the POST
/// body: compact JSON whose strings carry Go's HTML escapes (`&`, `<`, `>`)
/// and U+2028/U+2029 escapes, which `serde_json` leaves alone. None of those
/// characters can occur in the JSON structure itself, so escaping them after
/// serialization cannot alter non-string content.
fn go_json_string_array(value: &str) -> String {
    let mut encoded =
        serde_json::to_string(&[value]).expect("serializing a string slice cannot fail");
    encoded = encoded
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e");
    encoded = encoded
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    encoded
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

#[path = "provider_opencode_workspace.rs"]
mod opencode_workspace;
pub(super) use opencode_workspace::{extract_workspace, looks_signed_out, normalize_workspace};

#[cfg(test)]
#[path = "provider_requests_tests.rs"]
mod tests;
