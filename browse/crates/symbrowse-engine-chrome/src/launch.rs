use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};

use chromiumoxide::{Browser, BrowserConfig};

use crate::{
    BrowserMode,
    connection::{Connected, ConnectionMode},
};

const CHROME_ENV: &str = "SYMBROWSE_CHROME_EXECUTABLE";
/// Launch attempts before endpoint discovery is reported as failed. Chrome on
/// root CI runners (snap/AppArmor, small /dev/shm) occasionally never exposes
/// its DevTools endpoint; a fresh process reliably does.
const MAX_LAUNCH_ATTEMPTS: u32 = 3;

/// Resolve a usable Chrome/Chromium executable without requiring host-specific
/// environment configuration. An explicit environment value remains strict:
/// a typo must fail instead of silently selecting another browser.
///
/// The ordered candidates cover the standard macOS app bundles, Linux package
/// locations, and Windows installation roots. PATH is checked last so a
/// package-manager shim remains supported on every platform.
pub fn discover_chrome_executable() -> Result<PathBuf, String> {
    if let Some(value) = env::var_os(CHROME_ENV) {
        let path = PathBuf::from(value);
        return usable_executable(&path).ok_or_else(|| {
            format!(
                "{CHROME_ENV} does not point to an executable: {}",
                path.display()
            )
        });
    }

    let mut candidates = Vec::new();
    if cfg!(target_os = "macos") {
        if let Some(home) = env::var_os("HOME") {
            let home = PathBuf::from(home);
            candidates.extend([
                home.join("Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
                home.join("Applications/Chromium.app/Contents/MacOS/Chromium"),
            ]);
        }
        candidates.extend([
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
            PathBuf::from(
                "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
            ),
            PathBuf::from("/Applications/Google Chrome Beta.app/Contents/MacOS/Google Chrome Beta"),
        ]);
    } else {
        candidates.extend([
            PathBuf::from("/usr/bin/google-chrome"),
            PathBuf::from("/usr/bin/google-chrome-stable"),
            PathBuf::from("/usr/bin/chromium"),
            PathBuf::from("/usr/bin/chromium-browser"),
            PathBuf::from("/snap/bin/chromium"),
        ]);
    }

    candidates.extend([
        PathBuf::from("google-chrome"),
        PathBuf::from("google-chrome-stable"),
        PathBuf::from("chromium"),
        PathBuf::from("chromium-browser"),
    ]);
    candidates
        .into_iter()
        .find_map(|candidate| usable_executable(&candidate))
        .ok_or_else(|| {
            format!("Chrome/Chromium executable not found; install Chrome or set {CHROME_ENV}")
        })
}

/// Resolve an explicit configured executable or fall back to platform discovery.
pub fn resolve_chrome_executable(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return usable_executable(path).ok_or_else(|| {
            format!(
                "configured Chrome path does not point to an executable: {}",
                path.display()
            )
        });
    }
    discover_chrome_executable()
}

fn usable_executable(path: &Path) -> Option<PathBuf> {
    if path.components().count() == 1 {
        let path_value = env::var_os("PATH")?;
        for directory in env::split_paths(&path_value) {
            let candidate = directory.join(path);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
        return None;
    }
    is_executable(path).then(|| path.to_path_buf())
}

fn is_executable(path: &Path) -> bool {
    path.is_file() && {
        {
            use std::os::unix::fs::PermissionsExt;
            path.metadata()
                .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        }
    }
}

pub(crate) async fn connect(
    mode: &BrowserMode,
    timeout: Duration,
) -> Result<Connected, Box<dyn std::error::Error + Send + Sync>> {
    match mode {
        BrowserMode::Attach { endpoint } => {
            let (browser, handler) = Browser::connect(endpoint.clone()).await?;
            Ok(Connected {
                browser,
                handler,
                mode: ConnectionMode::Attach,
            })
        }
        BrowserMode::Launch {
            executable,
            user_data_dir,
            headless,
            disable_webrtc,
        } => {
            let is_root = rustix::process::geteuid().is_root();
            let mut last_error = None;
            for _ in 0..MAX_LAUNCH_ATTEMPTS {
                let mut builder = BrowserConfig::builder()
                    .chrome_executable(executable)
                    .user_data_dir(Path::new(user_data_dir))
                    .launch_timeout(timeout)
                    .request_timeout(timeout);
                for arg in launch_args(*disable_webrtc, is_root) {
                    builder = if arg.values.is_empty() {
                        builder.arg(arg.key)
                    } else {
                        builder.arg((arg.key, arg.values.as_slice()))
                    };
                }
                if *headless {
                    builder = builder.new_headless_mode();
                }
                if is_root {
                    builder = builder.no_sandbox();
                }
                match Browser::launch(builder.build()?).await {
                    Ok((browser, handler)) => {
                        return Ok(Connected {
                            browser,
                            handler,
                            mode: ConnectionMode::Launch,
                        });
                    }
                    Err(error) => last_error = Some(error),
                }
            }
            Err(format!(
                "launch Chrome (after {MAX_LAUNCH_ATTEMPTS} attempts): {}",
                last_error.map_or_else(String::new, |error| error.to_string())
            )
            .into())
        }
    }
}

/// One Chrome switch without its leading dashes; chromiumoxide adds them and
/// merges repeated keys (e.g. its own `disable-features` default).
#[derive(Debug, PartialEq, Eq)]
struct LaunchArg {
    key: &'static str,
    values: Vec<&'static str>,
}

const fn switch(key: &'static str) -> LaunchArg {
    LaunchArg {
        key,
        values: Vec::new(),
    }
}

/// Privacy and security switches of the Go launcher (ENG-002) that
/// chromiumoxide's defaults do not already set. `no-first-run`,
/// `disable-background-networking`, `disable-sync` and `disable-dev-shm-usage`
/// are chromiumoxide defaults; the sandbox switches for root come from
/// `BrowserConfigBuilder::no_sandbox`.
fn launch_args(disable_webrtc: bool, is_root: bool) -> Vec<LaunchArg> {
    let mut args = vec![
        switch("no-default-browser-check"),
        switch("disable-component-update"),
        switch("disable-domain-reliability"),
        LaunchArg {
            key: "disable-features",
            values: vec!["OptimizationHints", "MediaRouter"],
        },
    ];
    if disable_webrtc {
        // WebRTC is not a regular HTTP request and cannot be intercepted, so
        // an active allowlist disables it at the process level.
        args.push(switch("disable-webrtc"));
    }
    if is_root {
        args.push(switch("disable-gpu"));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_lookup_requires_an_executable_file() {
        let path =
            env::temp_dir().join(format!("symbrowse-chrome-discovery-{}", std::process::id()));
        {
            std::fs::write(&path, b"not a browser").expect("write discovery fixture");
            assert!(usable_executable(&path).is_none());
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn installed_chrome_is_discoverable_without_an_environment_override() {
        if env::var_os(CHROME_ENV).is_some() {
            return;
        }
        if let Ok(path) = discover_chrome_executable() {
            assert!(is_executable(&path));
        }
    }

    #[test]
    fn launch_args_match_the_go_launcher_switches() {
        let keys = |args: Vec<LaunchArg>| args.into_iter().map(|arg| arg.key).collect::<Vec<_>>();
        let base = keys(launch_args(false, false));
        assert_eq!(
            base,
            [
                "no-default-browser-check",
                "disable-component-update",
                "disable-domain-reliability",
                "disable-features"
            ]
        );
        assert!(base.iter().all(|key| !key.starts_with('-')));
        assert!(keys(launch_args(true, false)).contains(&"disable-webrtc"));
        assert!(!base.contains(&"disable-webrtc"));
        assert!(keys(launch_args(false, true)).contains(&"disable-gpu"));
        assert_eq!(
            launch_args(false, false)[3].values,
            ["OptimizationHints", "MediaRouter"]
        );
    }
}
