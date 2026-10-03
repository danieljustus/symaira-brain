//! Go-compatible cache and origin paths for daemon session metadata.

use std::{path::PathBuf, time::Duration};

// The Go registry owns browser profiles under the OS cache, independently of
// output/state cache settings and the legacy fixture seam.
pub(crate) fn default_session_cache_root() -> PathBuf {
    if cfg!(windows) {
        return std::env::var_os("LOCALAPPDATA")
            .filter(|p| !p.is_empty())
            .map_or_else(
                || std::env::temp_dir().join("symbrowse").join("sessions"),
                |path| PathBuf::from(path).join("symbrowse").join("sessions"),
            );
    }
    if cfg!(target_os = "macos") {
        return std::env::var_os("HOME")
            .filter(|p| !p.is_empty())
            .map_or_else(
                || std::env::temp_dir().join("symbrowse").join("sessions"),
                |home| PathBuf::from(home).join("Library/Caches/symbrowse/sessions"),
            );
    }
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME").filter(|p| !p.is_empty()) {
        let path = PathBuf::from(path);
        // Go UserCacheDir rejects relative XDG_CACHE_HOME instead of falling
        // back to HOME. The registry then uses TempDir for browser profiles.
        return if path.is_absolute() {
            path.join("symbrowse").join("sessions")
        } else {
            std::env::temp_dir().join("symbrowse").join("sessions")
        };
    }
    std::env::var_os("HOME")
        .filter(|p| !p.is_empty())
        .map_or_else(
            || std::env::temp_dir().join("symbrowse").join("sessions"),
            |home| PathBuf::from(home).join(".cache/symbrowse/sessions"),
        )
}
pub(crate) fn worktree_origin() -> String {
    let Ok(cwd) = std::env::current_dir() else {
        return String::new();
    };
    // Go asks git for the worktree root and falls back to the caller's cwd.
    // Bound child execution and reap the process before using that fallback.
    let fallback = || {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            go_json_text(cwd.as_os_str().as_bytes())
        }
        #[cfg(not(unix))]
        {
            cwd.to_string_lossy().into_owned()
        }
    };
    let Ok(mut child) = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return fallback();
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                return child.wait_with_output().map_or_else(
                    |_| fallback(),
                    |out| go_json_text(&out.stdout).trim().to_owned(),
                );
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return fallback();
            }
        }
    }
}

// Go encoding/json replaces each invalid UTF-8 byte, independently. Grouping
// an incomplete sequence would change the recorded worktree identity.
fn go_json_text(mut bytes: &[u8]) -> String {
    let mut out = String::new();
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                out.push_str(text);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                out.push_str(std::str::from_utf8(&bytes[..valid]).expect("valid prefix"));
                out.push('\u{fffd}');
                bytes = &bytes[valid + 1..];
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "spec_paths_tests.rs"]
mod tests;
