//! Identity metadata retained from the frozen Go compatibility baseline.
use std::collections::BTreeMap;

// The Go 1.26.7 baseline reports its compiler runtime as identity metadata,
// rather than querying the OS version. Preserve that protocol value so the
// native port sends the same five headers without invoking a Go executable.
// This is an SDK compatibility value, not a claim about the running OS.
pub(super) const KIMI_COMPATIBILITY_VERSION: &str = "1.26.7";

pub(super) fn platform_label() -> &'static str {
    std::env::consts::OS
}

fn platform_display_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else {
        platform_label()
    }
}

pub(super) fn kimi_headers(device: Option<&str>) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::from([
        ("X-Msh-Platform".into(), platform_label().into()),
        ("X-Msh-Device-Name".into(), hostname()),
        ("X-Msh-Os-Version".into(), KIMI_COMPATIBILITY_VERSION.into()),
        (
            "X-Msh-Device-Model".into(),
            format!("{} {KIMI_COMPATIBILITY_VERSION}", platform_display_name()),
        ),
    ]);
    if let Some(device) = device.filter(|value| !value.is_empty()) {
        headers.insert("X-Msh-Device-Id".into(), device.into());
    }
    headers
}

#[cfg(unix)]
pub(super) fn hostname() -> String {
    rustix::system::uname()
        .nodename()
        .to_string_lossy()
        .into_owned()
}

#[cfg(windows)]
pub(super) fn hostname() -> String {
    hostname::get()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(not(any(unix, windows)))]
pub(super) fn hostname() -> String {
    String::new()
}

pub(super) fn copilot_url(host: Option<&str>) -> String {
    match host.filter(|value| !value.is_empty()) {
        None | Some("github.com") => "https://api.github.com/copilot_internal/user".into(),
        Some(host) => format!("https://{host}/api/v3/copilot_internal/user"),
    }
}
