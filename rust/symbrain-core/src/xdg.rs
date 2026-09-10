//! Directory and path resolution for configuration, data, and cache files
//! adhering to the XDG Base Directory Specification and Symaira conventions.

use std::env;
use std::path::{Path, PathBuf};

/// Application name namespace for Symaira Brain files.
pub const APP_NAME: &str = "symbrain";

/// Canonical global configuration file name.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Target platform family for home directory resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    /// Unix family (Linux, macOS, BSDs): uses `HOME` only.
    Unix,
    /// Windows family: uses `USERPROFILE` only.
    Windows,
}

impl Platform {
    /// Returns the runtime platform target.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

/// Resolves user home directory given a platform and environment lookup function.
///
/// Matches Go's `os.UserHomeDir()` platform behavior:
/// - Windows uses `USERPROFILE` then `HOMEDRIVE` + `HOMEPATH`.
/// - Unix uses `HOME` only.
pub fn resolve_home_dir_with<F>(platform: Platform, mut get_env: F) -> Option<PathBuf>
where
    F: FnMut(&str) -> Option<PathBuf>,
{
    match platform {
        Platform::Unix => get_env("HOME").filter(|p| !p.as_os_str().is_empty()),
        Platform::Windows => {
            if let Some(profile) = get_env("USERPROFILE").filter(|p| !p.as_os_str().is_empty()) {
                Some(profile)
            } else {
                let drive = get_env("HOMEDRIVE").filter(|p| !p.as_os_str().is_empty());
                let path = get_env("HOMEPATH").filter(|p| !p.as_os_str().is_empty());
                match (drive, path) {
                    (Some(d), Some(p)) => {
                        let mut combined = d.into_os_string();
                        combined.push(p.as_os_str());
                        let res = PathBuf::from(combined);
                        if res.as_os_str().is_empty() {
                            None
                        } else {
                            Some(res)
                        }
                    }
                    _ => None,
                }
            }
        }
    }
}

/// Resolves user home directory using the current platform and process environment.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    resolve_home_dir_with(Platform::current(), env_path)
}

/// Resolves the global configuration file path (`<config_dir>/config.toml`).
///
/// If `xdg_config_home` is set to an absolute path, it is used.
/// Otherwise, falls back to `<home>/.config/symbrain/config.toml`.
/// Relative paths in `xdg_config_home` are ignored per the XDG Base Directory Specification.
/// When neither a usable `xdg_config_home` nor a usable `home` is available,
/// falls back to `.config/symbrain/config.toml` matching `configkit.DefaultPath`.
#[must_use]
pub fn resolve_config_path(xdg_config_home: Option<&Path>, home: Option<&Path>) -> PathBuf {
    if let Some(xdg) = xdg_config_home.filter(|p| p.is_absolute()) {
        return xdg.join(APP_NAME).join(CONFIG_FILE_NAME);
    }
    if let Some(h) = home.filter(|h| !h.as_os_str().is_empty()) {
        h.join(".config").join(APP_NAME).join(CONFIG_FILE_NAME)
    } else {
        Path::new(".config").join(APP_NAME).join(CONFIG_FILE_NAME)
    }
}

/// Resolves the configuration directory (`<base>/symbrain`).
#[must_use]
pub fn resolve_config_dir(xdg_config_home: Option<&Path>, home: Option<&Path>) -> PathBuf {
    resolve_config_path(xdg_config_home, home)
        .parent()
        .map_or_else(
            || PathBuf::from(".config").join(APP_NAME),
            Path::to_path_buf,
        )
}

/// Resolves the profiles directory (`<config_dir>/profiles`).
#[must_use]
pub fn resolve_profiles_dir(xdg_config_home: Option<&Path>, home: Option<&Path>) -> PathBuf {
    resolve_config_dir(xdg_config_home, home).join("profiles")
}

/// Resolves the data directory (`<data_base>/symbrain`).
///
/// Honors `xdg_data_home` (accepting relative paths per Go implementation),
/// falling back to `<home>/.local/share/symbrain`.
#[must_use]
pub fn resolve_data_dir(xdg_data_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(xdg) = xdg_data_home.filter(|p| !p.as_os_str().is_empty()) {
        return Some(xdg.join(APP_NAME));
    }
    home.filter(|h| !h.as_os_str().is_empty())
        .map(|h| h.join(".local").join("share").join(APP_NAME))
}

/// Resolves the audit log directory (`<data_dir>/audit`).
#[must_use]
pub fn resolve_audit_dir(xdg_data_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    resolve_data_dir(xdg_data_home, home).map(|dir| dir.join("audit"))
}

/// Resolves the patterns/recipes directory (`<data_dir>/recipes`).
///
/// Named "recipes" on disk for backwards compatibility with prior stores.
#[must_use]
pub fn resolve_patterns_dir(xdg_data_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    resolve_data_dir(xdg_data_home, home).map(|dir| dir.join("recipes"))
}

/// Resolves the cache directory (`<cache_base>/symbrain`).
///
/// Honors `xdg_cache_home` (accepting relative paths per Go implementation),
/// falling back to `<home>/.cache/symbrain`.
#[must_use]
pub fn resolve_cache_dir(xdg_cache_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(xdg) = xdg_cache_home.filter(|p| !p.as_os_str().is_empty()) {
        return Some(xdg.join(APP_NAME));
    }
    home.filter(|h| !h.as_os_str().is_empty())
        .map(|h| h.join(".cache").join(APP_NAME))
}

/// Resolves the managed binaries directory (`<home>/.symaira/bin`).
#[must_use]
pub fn resolve_managed_bin_dir(home: Option<&Path>) -> Option<PathBuf> {
    home.filter(|h| !h.as_os_str().is_empty())
        .map(|h| h.join(".symaira").join("bin"))
}

fn env_path(var_name: &str) -> Option<PathBuf> {
    env::var_os(var_name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Resolves the global config file path using current process environment variables.
#[must_use]
pub fn config_path() -> PathBuf {
    let xdg = env_path("XDG_CONFIG_HOME");
    let home = home_dir();
    resolve_config_path(xdg.as_deref(), home.as_deref())
}

/// Resolves the global config directory using current process environment variables.
#[must_use]
pub fn config_dir() -> PathBuf {
    let xdg = env_path("XDG_CONFIG_HOME");
    let home = home_dir();
    resolve_config_dir(xdg.as_deref(), home.as_deref())
}

/// Resolves the profiles directory using current process environment variables.
#[must_use]
pub fn profiles_dir() -> PathBuf {
    let xdg = env_path("XDG_CONFIG_HOME");
    let home = home_dir();
    resolve_profiles_dir(xdg.as_deref(), home.as_deref())
}

/// Resolves the data directory using current process environment variables.
#[must_use]
pub fn data_dir() -> Option<PathBuf> {
    let xdg = env_path("XDG_DATA_HOME");
    let home = home_dir();
    resolve_data_dir(xdg.as_deref(), home.as_deref())
}

/// Resolves the audit directory using current process environment variables.
#[must_use]
pub fn audit_dir() -> Option<PathBuf> {
    let xdg = env_path("XDG_DATA_HOME");
    let home = home_dir();
    resolve_audit_dir(xdg.as_deref(), home.as_deref())
}

/// Resolves the patterns directory using current process environment variables.
#[must_use]
pub fn patterns_dir() -> Option<PathBuf> {
    let xdg = env_path("XDG_DATA_HOME");
    let home = home_dir();
    resolve_patterns_dir(xdg.as_deref(), home.as_deref())
}

/// Resolves the cache directory using current process environment variables.
#[must_use]
pub fn cache_dir() -> Option<PathBuf> {
    let xdg = env_path("XDG_CACHE_HOME");
    let home = home_dir();
    resolve_cache_dir(xdg.as_deref(), home.as_deref())
}

/// Resolves the managed binaries directory using current process environment variables.
#[must_use]
pub fn managed_bin_dir() -> Option<PathBuf> {
    let home = home_dir();
    resolve_managed_bin_dir(home.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_abs(p: &str) -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(format!("C:{}", p.replace('/', "\\")))
        }
        #[cfg(not(windows))]
        {
            PathBuf::from(p)
        }
    }

    #[test]
    fn config_path_honors_absolute_xdg_config_home() {
        let xdg = test_abs("/custom/config");
        let home = test_abs("/home/user");
        let path = resolve_config_path(Some(&xdg), Some(&home));
        assert_eq!(
            path,
            test_abs("/custom/config")
                .join("symbrain")
                .join("config.toml")
        );
    }

    #[test]
    fn config_path_falls_back_to_home_when_xdg_unset() {
        let home = test_abs("/home/user");
        let path = resolve_config_path(None, Some(&home));
        assert_eq!(
            path,
            test_abs("/home/user")
                .join(".config")
                .join("symbrain")
                .join("config.toml")
        );
    }

    #[test]
    fn config_path_falls_back_to_home_when_xdg_empty() {
        let xdg = Path::new("");
        let home = test_abs("/home/user");
        let path = resolve_config_path(Some(xdg), Some(&home));
        assert_eq!(
            path,
            test_abs("/home/user")
                .join(".config")
                .join("symbrain")
                .join("config.toml")
        );
    }

    #[test]
    fn config_path_ignores_relative_xdg_config_home() {
        let xdg = Path::new("relative/config");
        let home = test_abs("/home/user");
        let path = resolve_config_path(Some(xdg), Some(&home));
        assert_eq!(
            path,
            test_abs("/home/user")
                .join(".config")
                .join("symbrain")
                .join("config.toml")
        );
    }

    #[test]
    fn config_path_falls_back_to_default_when_both_missing() {
        assert_eq!(
            resolve_config_path(None, None),
            PathBuf::from(".config")
                .join("symbrain")
                .join("config.toml")
        );
        assert_eq!(
            resolve_config_path(Some(Path::new("relative")), None),
            PathBuf::from(".config")
                .join("symbrain")
                .join("config.toml")
        );
        assert_eq!(
            resolve_config_path(Some(Path::new("")), Some(Path::new(""))),
            PathBuf::from(".config")
                .join("symbrain")
                .join("config.toml")
        );
    }

    #[test]
    fn config_dir_is_parent_of_config_path() {
        let home = test_abs("/home/user");
        let dir = resolve_config_dir(None, Some(&home));
        let path = resolve_config_path(None, Some(&home));
        assert_eq!(dir, test_abs("/home/user").join(".config").join("symbrain"));
        assert_eq!(path.parent().unwrap(), dir);

        let fallback_dir = resolve_config_dir(None, None);
        assert_eq!(fallback_dir, PathBuf::from(".config").join("symbrain"));
    }

    #[test]
    fn profiles_dir_sits_under_config_dir() {
        let home = test_abs("/home/user");
        let profiles = resolve_profiles_dir(None, Some(&home));
        assert_eq!(
            profiles,
            test_abs("/home/user")
                .join(".config")
                .join("symbrain")
                .join("profiles")
        );

        let fallback_profiles = resolve_profiles_dir(None, None);
        assert_eq!(
            fallback_profiles,
            PathBuf::from(".config").join("symbrain").join("profiles")
        );
    }

    #[test]
    fn data_dir_accepts_relative_xdg_data_home() {
        let relative = Path::new("custom/data");
        assert_eq!(
            resolve_data_dir(Some(relative), None).unwrap(),
            PathBuf::from("custom/data").join("symbrain")
        );
        let home = test_abs("/home/user");
        assert_eq!(
            resolve_data_dir(Some(Path::new("rel")), Some(&home)).unwrap(),
            PathBuf::from("rel").join("symbrain")
        );
    }

    #[test]
    fn data_dir_honors_env_and_falls_back_to_home() {
        let xdg = test_abs("/custom/data");
        let home = test_abs("/home/user");
        assert_eq!(
            resolve_data_dir(Some(&xdg), Some(&home)).unwrap(),
            test_abs("/custom/data").join("symbrain")
        );
        assert_eq!(
            resolve_data_dir(None, Some(&home)).unwrap(),
            test_abs("/home/user")
                .join(".local")
                .join("share")
                .join("symbrain")
        );
        assert_eq!(
            resolve_data_dir(Some(Path::new("")), Some(&home)).unwrap(),
            test_abs("/home/user")
                .join(".local")
                .join("share")
                .join("symbrain")
        );
        assert_eq!(resolve_data_dir(None, None), None);
    }

    #[test]
    fn audit_dir_sits_under_data_dir() {
        let xdg = test_abs("/custom/data");
        assert_eq!(
            resolve_audit_dir(Some(&xdg), None).unwrap(),
            test_abs("/custom/data").join("symbrain").join("audit")
        );
    }

    #[test]
    fn patterns_dir_sits_under_data_dir_as_recipes() {
        let xdg = test_abs("/custom/data");
        assert_eq!(
            resolve_patterns_dir(Some(&xdg), None).unwrap(),
            test_abs("/custom/data").join("symbrain").join("recipes")
        );
    }

    #[test]
    fn cache_dir_accepts_relative_xdg_cache_home() {
        let relative = Path::new("custom/cache");
        assert_eq!(
            resolve_cache_dir(Some(relative), None).unwrap(),
            PathBuf::from("custom/cache").join("symbrain")
        );
        let home = test_abs("/home/user");
        assert_eq!(
            resolve_cache_dir(Some(Path::new("rel")), Some(&home)).unwrap(),
            PathBuf::from("rel").join("symbrain")
        );
    }

    #[test]
    fn cache_dir_honors_env_and_falls_back_to_home() {
        let xdg = test_abs("/custom/cache");
        let home = test_abs("/home/user");
        assert_eq!(
            resolve_cache_dir(Some(&xdg), Some(&home)).unwrap(),
            test_abs("/custom/cache").join("symbrain")
        );
        assert_eq!(
            resolve_cache_dir(None, Some(&home)).unwrap(),
            test_abs("/home/user").join(".cache").join("symbrain")
        );
        assert_eq!(
            resolve_cache_dir(Some(Path::new("")), Some(&home)).unwrap(),
            test_abs("/home/user").join(".cache").join("symbrain")
        );
        assert_eq!(resolve_cache_dir(None, None), None);
    }

    #[test]
    fn managed_bin_dir_sits_under_home() {
        let home = test_abs("/home/user");
        assert_eq!(
            resolve_managed_bin_dir(Some(&home)).unwrap(),
            test_abs("/home/user").join(".symaira").join("bin")
        );
        assert_eq!(resolve_managed_bin_dir(None), None);
        assert_eq!(resolve_managed_bin_dir(Some(Path::new(""))), None);
    }

    #[test]
    fn home_dir_unix_uses_home_only() {
        let mock_env = |var: &str| match var {
            "HOME" => Some(PathBuf::from("/home/unixuser")),
            "USERPROFILE" => Some(PathBuf::from("C:\\Users\\winuser")),
            "HOMEDRIVE" => Some(PathBuf::from("C:")),
            "HOMEPATH" => Some(PathBuf::from("\\Users\\winuser")),
            _ => None,
        };
        assert_eq!(
            resolve_home_dir_with(Platform::Unix, mock_env),
            Some(PathBuf::from("/home/unixuser"))
        );
    }

    #[test]
    fn home_dir_unix_ignores_userprofile_and_homedrive() {
        let mock_env = |var: &str| match var {
            "USERPROFILE" => Some(PathBuf::from("C:\\Users\\winuser")),
            "HOMEDRIVE" => Some(PathBuf::from("C:")),
            "HOMEPATH" => Some(PathBuf::from("\\Users\\winuser")),
            _ => None,
        };
        assert_eq!(resolve_home_dir_with(Platform::Unix, mock_env), None);
    }

    #[test]
    fn home_dir_windows_uses_userprofile_first() {
        let mock_env = |var: &str| match var {
            "HOME" => Some(PathBuf::from("/home/unixuser")),
            "USERPROFILE" => Some(PathBuf::from("C:\\Users\\winuser")),
            "HOMEDRIVE" => Some(PathBuf::from("D:")),
            "HOMEPATH" => Some(PathBuf::from("\\Users\\other")),
            _ => None,
        };
        assert_eq!(
            resolve_home_dir_with(Platform::Windows, mock_env),
            Some(PathBuf::from("C:\\Users\\winuser"))
        );
    }

    #[test]
    fn home_dir_windows_falls_back_to_homedrive_and_homepath() {
        let mock_env = |var: &str| match var {
            "HOME" => Some(PathBuf::from("/home/unixuser")),
            "HOMEDRIVE" => Some(PathBuf::from("C:")),
            "HOMEPATH" => Some(PathBuf::from("\\Users\\driveuser")),
            _ => None,
        };
        assert_eq!(
            resolve_home_dir_with(Platform::Windows, mock_env),
            Some(PathBuf::from("C:\\Users\\driveuser"))
        );
    }

    #[test]
    fn home_dir_windows_partial_homedrive_or_homepath_returns_none() {
        let mock_only_drive = |var: &str| match var {
            "HOMEDRIVE" => Some(PathBuf::from("C:")),
            _ => None,
        };
        assert_eq!(
            resolve_home_dir_with(Platform::Windows, mock_only_drive),
            None
        );

        let mock_only_path = |var: &str| match var {
            "HOMEPATH" => Some(PathBuf::from("\\Users\\winuser")),
            _ => None,
        };
        assert_eq!(
            resolve_home_dir_with(Platform::Windows, mock_only_path),
            None
        );
    }

    #[test]
    fn home_dir_windows_ignores_home() {
        let mock_env = |var: &str| match var {
            "HOME" => Some(PathBuf::from("/home/unixuser")),
            _ => None,
        };
        assert_eq!(resolve_home_dir_with(Platform::Windows, mock_env), None);
    }

    #[test]
    fn home_dir_empty_returns_none() {
        let mock_env_empty = |var: &str| match var {
            "HOME" | "USERPROFILE" | "HOMEDRIVE" | "HOMEPATH" => Some(PathBuf::from("")),
            _ => None,
        };
        assert_eq!(resolve_home_dir_with(Platform::Unix, mock_env_empty), None);
        assert_eq!(
            resolve_home_dir_with(Platform::Windows, mock_env_empty),
            None
        );
    }

    #[test]
    fn live_config_path_always_resolves() {
        let path = config_path();
        assert!(!path.as_os_str().is_empty());
        assert!(path.ends_with("config.toml"));
    }

    #[test]
    fn live_env_resolves_when_home_is_present() {
        if home_dir().is_some() {
            assert!(data_dir().is_some());
            assert!(audit_dir().is_some());
            assert!(patterns_dir().is_some());
            assert!(cache_dir().is_some());
            assert!(managed_bin_dir().is_some());
        }
    }
}
