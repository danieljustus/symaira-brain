//! Runtime path admission reads only relevant OS values, never `env::vars()`.
use crate::registry::PathKind;
use crate::{ConfigLocation, HarnessError};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

fn value(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|v| !v.is_empty())
}
fn home() -> Result<OsString, HarnessError> {
    value(symbrain_core::go_path::home_variable())
        .ok_or_else(|| HarnessError::Unsupported("unable to resolve home directory".into()))
}
fn join(parts: &[&OsStr]) -> PathBuf {
    symbrain_core::go_path::join(parts)
}

pub(super) fn location(kind: PathKind) -> Result<ConfigLocation, HarnessError> {
    let target_os = std::env::consts::OS;
    let relative_path = crate::relative_path(kind, target_os);
    let trusted_root = match kind {
        PathKind::Home(_) => join(&[&home()?]),
        PathKind::Xdg(_) => config_root()?,
        PathKind::ClaudeDesktop => {
            if cfg!(target_os = "macos") {
                join(&[&home()?])
            } else if cfg!(windows) {
                value("APPDATA").map_or_else(
                    || {
                        home().map(|home| {
                            join(&[&home, OsStr::new("AppData"), OsStr::new("Roaming")])
                        })
                    },
                    |path| Ok(join(&[&path])),
                )?
            } else {
                config_root()?
            }
        }
        PathKind::Unsupported => {
            return Err(HarnessError::Unsupported(
                "harness does not support MCP installation".into(),
            ));
        }
    };
    let path = join(&[trusted_root.as_os_str(), relative_path.as_os_str()]);
    Ok(ConfigLocation {
        path,
        trusted_root,
        relative_path,
    })
}
fn config_root() -> Result<PathBuf, HarnessError> {
    // The harness registry has its own historical XDG contract: any nonempty
    // value is used. Do not apply Brain configkit's absolute-XDG admission here.
    value("XDG_CONFIG_HOME").map_or_else(
        || home().map(|home| join(&[&home, OsStr::new(".config")])),
        |path| Ok(join(&[&path])),
    )
}
