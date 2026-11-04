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
        ("grok", _) => ("GET", "https://cli-chat-proxy.grok.com/v1/billing?format=credits".into(), None),
        ("kimi", "web") => ("POST", "https://www.kimi.com/apiv2/kimi.gateway.billing.v1.BillingService/GetUsages".into(), None),
        ("kimi", _) => ("GET", go_request_url(&format!("{}/coding/v1/usages", checked_base(provider, "https://api.kimi.com").trim_end_matches('/'))), None),
        ("moonshot", _) => ("GET", format!("https://api.moonshot.{}/v1/users/me/balance", provider.region), None),
        ("nous", _) => ("GET", go_request_url(&format!("{}/api/oauth/account", checked_base(provider, "https://portal.nousresearch.com").trim_end_matches('/'))), None),
        ("openrouter", "credits" | "key") => ("GET", go_request_url(&format!("{}/{}", checked_base(provider, "https://openrouter.ai/api/v1"), source)), None),
        ("openrouter", _) => ("GET", go_request_url(&format!("{}/auth/key", checked_base(provider, "https://openrouter.ai/api/v1"))), None),
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
    let mut header_bytes = BTreeMap::new();
    if id == "kimi" && source == "cli" {
        headers.extend(kimi_headers(provider.device_id.as_deref()));
        if let Some(bytes) = provider.device_id_bytes.clone() {
            header_bytes.insert("X-Msh-Device-Id".into(), bytes);
        }
    }
    if id == "grok" {
        headers.insert("X-XAI-Token-Auth".into(), "xai-grok-cli".into());
        headers.insert("User-Agent".into(), "symbrain".into());
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
        header_bytes,
        provider_id: id.into(),
        method: method.into(),
        url,
        headers,
        body: body.map(String::into_bytes),
    }
}

pub(crate) fn validated_base(raw: &str, fallback: &str) -> String {
    if is_trusted_https(raw) {
        // Go's `validatedUsageBase` trims exactly one trailing slash.
        raw.strip_suffix('/').unwrap_or(raw).to_string()
    } else {
        fallback.into()
    }
}
pub(crate) fn trusted_https_url(raw: &str, allow_loopback: bool) -> bool {
    crate::go_url::trusted_https(raw, allow_loopback, is_private_ip)
}

/// The provider's stored base (already trimmed once by `validated_base` when it
/// was read from the environment) if it is still trusted, else the default.
fn checked_base<'a>(provider: &'a Provider, default: &'a str) -> &'a str {
    provider
        .base_url
        .as_deref()
        .filter(|base| trusted_https_url(base, false))
        .unwrap_or(default)
}

/// The URL string Go's `http.NewRequest` + `URL.String()` produces; the
/// composed URL is only ever built from a base that passed the trust check.
fn go_request_url(composed: &str) -> String {
    crate::go_url::request_url(composed).unwrap_or_else(|| composed.to_owned())
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
