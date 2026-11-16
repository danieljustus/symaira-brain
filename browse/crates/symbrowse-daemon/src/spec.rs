#[path = "spec_paths.rs"]
mod paths;
pub(crate) use paths::json_path;
pub(crate) use paths::{default_session_cache_root, worktree_origin};

use std::{path::PathBuf, time::Duration};

use symbrowse_core::config::{Config, resolve_selection};

/// Effective configuration owned by one daemon session.
///
/// The daemon, its clients, MCP autostart, state store, and status handshake
/// all use this value. Keeping it typed prevents a caller from silently
/// falling back to process-global defaults after startup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionSpec {
    pub session: String,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub engine: String,
    pub mode: String,
    pub executable_path: PathBuf,
    pub cdp_endpoint: String,
    pub allowed_domains: Vec<String>,
    pub ssrf_enabled: bool,
    pub allow_private: bool,
    pub fetch_robots: bool,
    pub fetch_user_agent: String,
    pub operation_timeout: Duration,
    pub read_timeout: Duration,
    pub idle_timeout: Option<Duration>,
    pub state_expire_days: i64,
    pub daemon_log: PathBuf,
    pub socket_path: PathBuf,
}

impl SessionSpec {
    #[must_use]
    pub fn for_session(session: impl Into<String>) -> Self {
        let session = session.into();
        let state_dir = default_state_dir();
        Self {
            socket_path: default_socket_path(&session),
            daemon_log: default_log_path(),
            session,
            cache_dir: default_cache_dir(),
            state_dir,
            engine: "chrome".into(),
            mode: "browser".into(),
            executable_path: PathBuf::new(),
            cdp_endpoint: String::new(),
            allowed_domains: Vec::new(),
            ssrf_enabled: false,
            allow_private: false,
            fetch_robots: true,
            fetch_user_agent: "symbrowse/1.0".into(),
            operation_timeout: Duration::from_secs(25),
            read_timeout: Duration::from_secs(30),
            idle_timeout: Some(Duration::from_secs(1800)),
            state_expire_days: 30,
        }
    }

    pub fn from_config(config: &Config, session: impl Into<String>) -> Self {
        let mut spec = Self::for_session(session);
        spec.state_dir = PathBuf::from(&config.state_dir);
        spec.cache_dir = PathBuf::from(&config.cache_dir);
        spec.engine = if config.engine.is_empty() {
            "chrome"
        } else {
            &config.engine
        }
        .into();
        spec.mode = if config.engine == "static" {
            "static".into()
        } else {
            config.mode.clone()
        };
        spec.executable_path = PathBuf::from(&config.executable_path);
        spec.cdp_endpoint = config.cdp_endpoint.clone();
        spec.allowed_domains = config.allowed_domains.clone();
        spec.ssrf_enabled = config.ssrf_enabled;
        spec.allow_private = config.allow_private;
        spec.fetch_robots = config.fetch_robots;
        spec.fetch_user_agent = config.fetch_user_agent.clone();
        spec.operation_timeout = Duration::from_secs(config.operation_timeout.max(1) as u64);
        spec.read_timeout = Duration::from_secs(config.read_timeout.max(1) as u64);
        spec.idle_timeout =
            (config.idle_timeout > 0).then_some(Duration::from_secs(config.idle_timeout as u64));
        spec.state_expire_days = config.state_expire_days.max(1);
        spec.daemon_log = if config.daemon_log.is_empty() {
            default_log_path()
        } else {
            PathBuf::from(&config.daemon_log)
        };
        spec.socket_path = default_socket_path(&spec.session);
        spec
    }

    pub fn validate_selection(&self) -> Result<(), String> {
        // `compat` is retired; reject it before the static-engine shortcut so
        // it is never silently served as static.
        if self.mode == "compat" {
            return resolve_selection(Some(&self.mode), None)
                .map(|_| ())
                .map_err(|error| format!("{}: {}", error.code, error.message));
        }
        if self.engine == "static" {
            return Ok(());
        }
        resolve_selection(Some(&self.mode), Some(&self.engine))
            .map(|_| ())
            .map_err(|error| format!("{}: {}", error.code, error.message))
    }

    #[must_use]
    pub fn state_store_dir(&self) -> PathBuf {
        self.state_dir.join("states")
    }

    #[must_use]
    pub fn user_data_dir(&self) -> PathBuf {
        default_session_cache_root().join(&self.session)
    }

    #[must_use]
    pub fn output_cache_dir(&self) -> PathBuf {
        self.cache_dir.join("out")
    }
}

#[must_use]
pub fn default_state_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("SYMBROWSE_STATE_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("symbrowse");
    }
    std::env::var_os("HOME").map_or_else(
        || std::env::temp_dir().join("symbrowse/state"),
        |home| PathBuf::from(home).join(".local/state/symbrowse"),
    )
}

#[must_use]
pub fn default_cache_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("SYMBROWSE_CACHE_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(path).join("symbrowse");
    }
    std::env::var_os("HOME").map_or_else(
        || std::env::temp_dir().join("symbrowse/cache"),
        |home| PathBuf::from(home).join(".cache/symbrowse"),
    )
}

#[must_use]
pub fn default_log_path() -> PathBuf {
    use symbrowse_core::config::{FlagOverrides, LoadContext, load};
    if let Some(config) = LoadContext::from_process(FlagOverrides::default())
        .ok()
        .and_then(|context| load(&context).ok())
    {
        return PathBuf::from(config.config.daemon_log);
    }
    std::env::var_os("SYMBROWSE_DAEMON_LOG")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_state_dir().join("daemon.log"))
}

/// Go `socketBaseDir` (`internal/daemon/path.go`) with the process facts passed
/// in, so both platform branches are checkable on any host. macOS keeps the
/// socket under `~/Library/Caches`; elsewhere `$XDG_RUNTIME_DIR/symbrowse` wins
/// over `<user cache dir>/symbrowse/run`. Every result is `filepath.Join`-clean.
pub fn socket_base_dir(
    macos: bool,
    home: Option<&str>,
    xdg_runtime_dir: Option<&str>,
    xdg_cache_home: Option<&str>,
) -> Result<PathBuf, String> {
    use symbrowse_core::config::go_join;
    let home = home.filter(|value| !value.is_empty());
    if macos {
        let home = home.ok_or("determine home directory: $HOME is not defined")?;
        return Ok(PathBuf::from(go_join(&[
            home,
            "Library",
            "Caches",
            "symbrowse",
            "run",
        ])));
    }
    if let Some(runtime) = xdg_runtime_dir.filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(go_join(&[runtime, "symbrowse"])));
    }
    // Go `os.UserCacheDir` on Unix.
    let cache = match xdg_cache_home.filter(|value| !value.is_empty()) {
        Some(cache) if cache.starts_with('/') => cache.to_owned(),
        Some(_) => {
            return Err("determine cache directory: path in $XDG_CACHE_HOME is relative".into());
        }
        None => {
            let home = home.ok_or(
                "determine cache directory: neither $XDG_CACHE_HOME nor $HOME are defined",
            )?;
            format!("{home}/.cache")
        }
    };
    Ok(PathBuf::from(go_join(&[&cache, "symbrowse", "run"])))
}

/// The default socket of `session` as Go's `daemon.SocketPath` resolves it, or
/// the Go error text when the home or cache directory is undefined.
pub fn try_default_socket_path(session: &str) -> Result<PathBuf, String> {
    let var = |name: &str| std::env::var(name).ok();
    socket_base_dir(
        cfg!(target_os = "macos"),
        var("HOME").as_deref(),
        var("XDG_RUNTIME_DIR").as_deref(),
        var("XDG_CACHE_HOME").as_deref(),
    )
    .map(|base| base.join(format!("{session}.sock")))
}

/// Infallible variant for daemon-internal callers: without a usable home the
/// socket falls back to the temporary directory instead of failing startup.
#[must_use]
pub fn default_socket_path(session: &str) -> PathBuf {
    try_default_socket_path(session)
        .unwrap_or_else(|_| std::env::temp_dir().join(format!("symbrowse-{session}.sock")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_base_dir_follows_go_path_go_on_every_platform() {
        let dir = |macos, home, runtime, cache| socket_base_dir(macos, home, runtime, cache);
        // darwin: HOME based, XDG ignored, path cleaned.
        assert_eq!(
            dir(true, Some("/Users/u"), Some("/run/u"), Some("/c")).unwrap(),
            PathBuf::from("/Users/u/Library/Caches/symbrowse/run")
        );
        assert_eq!(
            dir(true, Some("/Users//u/./x/.."), None, None).unwrap(),
            PathBuf::from("/Users/u/Library/Caches/symbrowse/run")
        );
        assert_eq!(
            dir(true, Some(""), None, None).unwrap_err(),
            "determine home directory: $HOME is not defined"
        );
        assert_eq!(
            dir(true, None, None, None).unwrap_err(),
            "determine home directory: $HOME is not defined"
        );
        // elsewhere: runtime dir first, then the user cache dir.
        assert_eq!(
            dir(false, Some("/h"), Some("/run/user/1000"), Some("/c")).unwrap(),
            PathBuf::from("/run/user/1000/symbrowse")
        );
        assert_eq!(
            dir(false, Some("/h"), Some(""), Some("/c/")).unwrap(),
            PathBuf::from("/c/symbrowse/run")
        );
        assert_eq!(
            dir(false, Some("/h"), None, None).unwrap(),
            PathBuf::from("/h/.cache/symbrowse/run")
        );
        assert_eq!(
            dir(false, Some("/h"), None, Some("")).unwrap(),
            PathBuf::from("/h/.cache/symbrowse/run")
        );
        assert_eq!(
            dir(false, Some("/h"), None, Some("rel")).unwrap_err(),
            "determine cache directory: path in $XDG_CACHE_HOME is relative"
        );
        assert_eq!(
            dir(false, None, None, None).unwrap_err(),
            "determine cache directory: neither $XDG_CACHE_HOME nor $HOME are defined"
        );
        assert_eq!(
            dir(false, Some(""), None, Some("/c")).unwrap(),
            PathBuf::from("/c/symbrowse/run")
        );
    }

    #[test]
    fn spec_paths_are_one_configured_tree() {
        let spec = SessionSpec::for_session("alpha");
        assert!(spec.state_store_dir().ends_with("states"));
        assert!(spec.user_data_dir().ends_with("sessions/alpha"));
        assert!(spec.output_cache_dir().ends_with("out"));
    }

    #[test]
    fn retired_compat_mode_is_rejected_even_with_the_static_engine() {
        for engine in ["static", "chrome", ""] {
            let mut spec = SessionSpec::for_session("alpha");
            spec.mode = "compat".into();
            spec.engine = engine.into();
            let error = spec.validate_selection().unwrap_err();
            assert!(
                error.starts_with("transport_retired: "),
                "{engine}: {error}"
            );
        }
        for (mode, engine) in [("static", "static"), ("browser", "chrome")] {
            let mut spec = SessionSpec::for_session("alpha");
            spec.mode = mode.into();
            spec.engine = engine.into();
            spec.validate_selection().unwrap();
        }
    }
}
