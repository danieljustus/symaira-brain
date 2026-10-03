use super::provider_identity::{copilot_url, kimi_headers};
#[cfg(test)]
use super::provider_identity::{hostname, platform_label};
use super::{Provider, Request};
use std::collections::BTreeMap;
use std::env;
#[path = "provider_request_encoding.rs"]
mod encoding;
use encoding::{encode_query_arg, go_json_string_array};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
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
        ("copilot", _) => ("GET", copilot_url(provider.enterprise_host.as_deref()), None),
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
    if id == "kimi" && source == "cli" {
        headers.extend(kimi_headers(provider.device_id.as_deref()));
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
    const MAX_PROBE_OUTPUT_BYTES: u64 = 64 * 1024;
    let command_path = resolve_probe_tool(command)?;
    let mut output = tempfile::NamedTempFile::new().ok()?;
    let child_stdout = output.as_file().try_clone().ok()?;
    let mut command_line = Command::new(command_path);
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
                match output.as_file().metadata() {
                    Ok(metadata) if metadata.len() <= MAX_PROBE_OUTPUT_BYTES => {}
                    Ok(_) | Err(_) => {
                        terminate_probe(&mut child);
                        return None;
                    }
                }
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
    if output.as_file().metadata().ok()?.len() > MAX_PROBE_OUTPUT_BYTES {
        return None;
    }
    output.as_file_mut().seek(SeekFrom::Start(0)).ok()?;
    let mut data = Vec::new();
    output
        .as_file_mut()
        .take(MAX_PROBE_OUTPUT_BYTES + 1)
        .read_to_end(&mut data)
        .ok()?;
    (u64::try_from(data.len()).ok()? <= MAX_PROBE_OUTPUT_BYTES)
        .then(|| String::from_utf8_lossy(&data).into_owned())
}

fn resolve_probe_tool(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let search_paths = env::split_paths(&path);
    resolve_probe_tool_from(name, search_paths, &env::current_dir().ok()?)
}

fn resolve_probe_tool_from(
    name: &str,
    search_paths: impl IntoIterator<Item = PathBuf>,
    current_dir: &Path,
) -> Option<PathBuf> {
    let basename = Path::new(name).file_name()?.to_str()?;
    if basename != name {
        return None;
    }
    for directory in search_paths {
        if !directory.is_absolute() {
            let candidate = current_dir.join(directory).join(name);
            let Ok(metadata) = candidate.metadata() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 == 0 {
                    continue;
                }
            }
            // Go's exec.LookPath returns an ErrDot error when PATH would
            // select an executable through a relative entry. Do not silently
            // continue to a later absolute entry in that case.
            return None;
        }
        let candidate = directory.join(name);
        let Ok(metadata) = candidate.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        return Some(candidate);
    }
    None
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
