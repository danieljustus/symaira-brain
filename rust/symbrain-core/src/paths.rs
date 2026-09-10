//! Component path resolution for Symaira Memory and Symaira Skills
//! under the shared symbrain XDG namespace with legacy fallback support.

use std::env;
use std::path::{Path, PathBuf};

use crate::xdg;

/// Application name namespace for Symaira Brain files.
pub const APP_NAME: &str = "symbrain";

/// Component name for Symaira Memory.
pub const COMPONENT_MEMORY: &str = "memory";

/// Component name for Symaira Skills.
pub const COMPONENT_SKILLS: &str = "skills";

/// Legacy standalone app name for Symaira Memory before being absorbed.
pub const LEGACY_MEMORY_APP: &str = "symmemory";

/// Legacy standalone app name for Symaira Skills before being absorbed.
pub const LEGACY_SKILLS_APP: &str = "symskills";

/// Resolved on-disk directory and whether it was found under a legacy standalone location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Location {
    /// Resolved directory path.
    pub dir: PathBuf,
    /// True if resolved to a legacy standalone directory instead of the current namespace.
    pub legacy: bool,
}

/// Resolves a component location with fallback to a legacy directory if present.
///
/// `base` is `env_val` if absolute, otherwise `<home>/<fallback_rel>`.
/// Relative values for `env_val` are ignored per the XDG Base Directory Specification.
/// If `<base>/symbrain/<component>` exists as a directory, it is returned with `legacy = false`.
/// Otherwise, if `<base>/<legacy_app>` exists as a directory, it is returned with `legacy = true`.
/// If neither exists, returns `<base>/symbrain/<component>` with `legacy = false`.
/// Returns `None` if `env_val` is not absolute and `home` is missing or empty.
#[must_use]
pub fn resolve_location<F>(
    env_val: Option<&Path>,
    home: Option<&Path>,
    fallback_rel: &Path,
    component: &str,
    legacy_app: &str,
    mut dir_exists: F,
) -> Option<Location>
where
    F: FnMut(&Path) -> bool,
{
    let base: PathBuf = if let Some(env) = env_val.filter(|p| p.is_absolute()) {
        env.to_path_buf()
    } else {
        home.filter(|h| !h.as_os_str().is_empty())?
            .join(fallback_rel)
    };

    let current = base.join(APP_NAME).join(component);
    if dir_exists(&current) {
        return Some(Location {
            dir: current,
            legacy: false,
        });
    }

    let legacy = base.join(legacy_app);
    if dir_exists(&legacy) {
        return Some(Location {
            dir: legacy,
            legacy: true,
        });
    }

    Some(Location {
        dir: current,
        legacy: false,
    })
}

fn live_dir_exists(path: &Path) -> bool {
    path.is_dir()
}

fn env_path(var_name: &str) -> Option<PathBuf> {
    env::var_os(var_name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Resolves Symaira Memory configuration directory (`$XDG_CONFIG_HOME/symbrain/memory`).
///
/// Falls back to legacy `$XDG_CONFIG_HOME/symmemory` (or `~/.config/symmemory`)
/// when only that exists on disk.
#[must_use]
pub fn memory_config_dir() -> Option<Location> {
    let env = env_path("XDG_CONFIG_HOME");
    let home = xdg::home_dir();
    resolve_location(
        env.as_deref(),
        home.as_deref(),
        Path::new(".config"),
        COMPONENT_MEMORY,
        LEGACY_MEMORY_APP,
        live_dir_exists,
    )
}

/// Resolves Symaira Memory data directory (`$XDG_DATA_HOME/symbrain/memory`).
///
/// Falls back to legacy `$XDG_DATA_HOME/symmemory` (or `~/.local/share/symmemory`)
/// when only that exists on disk.
#[must_use]
pub fn memory_data_dir() -> Option<Location> {
    let env = env_path("XDG_DATA_HOME");
    let home = xdg::home_dir();
    resolve_location(
        env.as_deref(),
        home.as_deref(),
        &Path::new(".local").join("share"),
        COMPONENT_MEMORY,
        LEGACY_MEMORY_APP,
        live_dir_exists,
    )
}

/// Resolves Symaira Skills configuration directory (`$XDG_CONFIG_HOME/symbrain/skills`).
///
/// Falls back to legacy `$XDG_CONFIG_HOME/symskills` (or `~/.config/symskills`)
/// when only that exists on disk.
#[must_use]
pub fn skills_config_dir() -> Option<Location> {
    let env = env_path("XDG_CONFIG_HOME");
    let home = xdg::home_dir();
    resolve_location(
        env.as_deref(),
        home.as_deref(),
        Path::new(".config"),
        COMPONENT_SKILLS,
        LEGACY_SKILLS_APP,
        live_dir_exists,
    )
}

/// Resolves Symaira Skills data directory (`$XDG_DATA_HOME/symbrain/skills`).
///
/// Falls back to legacy `$XDG_DATA_HOME/symskills` (or `~/.local/share/symskills`)
/// when only that exists on disk.
#[must_use]
pub fn skills_data_dir() -> Option<Location> {
    let env = env_path("XDG_DATA_HOME");
    let home = xdg::home_dir();
    resolve_location(
        env.as_deref(),
        home.as_deref(),
        &Path::new(".local").join("share"),
        COMPONENT_SKILLS,
        LEGACY_SKILLS_APP,
        live_dir_exists,
    )
}

/// Resolves Symaira Skills cache directory (`$XDG_CACHE_HOME/symbrain/skills`).
///
/// Falls back to legacy `$XDG_CACHE_HOME/symskills` (or `~/.cache/symskills`)
/// when only that exists on disk.
#[must_use]
pub fn skills_cache_dir() -> Option<Location> {
    let env = env_path("XDG_CACHE_HOME");
    let home = xdg::home_dir();
    resolve_location(
        env.as_deref(),
        home.as_deref(),
        Path::new(".cache"),
        COMPONENT_SKILLS,
        LEGACY_SKILLS_APP,
        live_dir_exists,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

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
    fn resolve_location_defaults_to_current_namespace() {
        let home = test_abs("/home/user");
        let loc = resolve_location(
            None,
            Some(&home),
            Path::new(".config"),
            COMPONENT_MEMORY,
            LEGACY_MEMORY_APP,
            |_| false,
        )
        .unwrap();

        assert_eq!(
            loc.dir,
            test_abs("/home/user")
                .join(".config")
                .join("symbrain")
                .join("memory")
        );
        assert!(!loc.legacy);
    }

    #[test]
    fn resolve_location_honors_absolute_xdg_env() {
        let base = test_abs("/custom/config");
        let loc = resolve_location(
            Some(&base),
            None,
            Path::new(".config"),
            COMPONENT_MEMORY,
            LEGACY_MEMORY_APP,
            |_| false,
        )
        .unwrap();

        assert_eq!(
            loc.dir,
            test_abs("/custom/config").join("symbrain").join("memory")
        );
        assert!(!loc.legacy);
    }

    #[test]
    fn resolve_location_falls_back_to_legacy_when_only_legacy_exists() {
        let base = test_abs("/custom/config");
        let legacy_path = base.join(LEGACY_MEMORY_APP);

        let loc = resolve_location(
            Some(&base),
            None,
            Path::new(".config"),
            COMPONENT_MEMORY,
            LEGACY_MEMORY_APP,
            |p| p == legacy_path,
        )
        .unwrap();

        assert_eq!(loc.dir, legacy_path);
        assert!(loc.legacy);
    }

    #[test]
    fn resolve_location_prefers_current_when_both_exist() {
        let base = test_abs("/custom/config");
        let current_path = base.join(APP_NAME).join(COMPONENT_MEMORY);
        let legacy_path = base.join(LEGACY_MEMORY_APP);

        let mut existing = HashSet::new();
        existing.insert(current_path.clone());
        existing.insert(legacy_path);

        let loc = resolve_location(
            Some(&base),
            None,
            Path::new(".config"),
            COMPONENT_MEMORY,
            LEGACY_MEMORY_APP,
            |p| existing.contains(p),
        )
        .unwrap();

        assert_eq!(loc.dir, current_path);
        assert!(!loc.legacy);
    }

    #[test]
    fn resolve_location_ignores_relative_xdg_env() {
        let home = test_abs("/home/user");
        let rel_env = Path::new("relative/config");

        let loc = resolve_location(
            Some(rel_env),
            Some(&home),
            Path::new(".config"),
            COMPONENT_SKILLS,
            LEGACY_SKILLS_APP,
            |_| false,
        )
        .unwrap();

        assert_eq!(
            loc.dir,
            test_abs("/home/user")
                .join(".config")
                .join("symbrain")
                .join("skills")
        );
        assert!(!loc.legacy);
    }

    #[test]
    fn resolve_location_memory_data_dir() {
        let base = test_abs("/custom/data");
        let loc = resolve_location(
            Some(&base),
            None,
            &Path::new(".local").join("share"),
            COMPONENT_MEMORY,
            LEGACY_MEMORY_APP,
            |_| false,
        )
        .unwrap();

        assert_eq!(
            loc.dir,
            test_abs("/custom/data").join("symbrain").join("memory")
        );
        assert!(!loc.legacy);
    }

    #[test]
    fn resolve_location_skills_config_dir() {
        let base = test_abs("/custom/config");
        let loc = resolve_location(
            Some(&base),
            None,
            Path::new(".config"),
            COMPONENT_SKILLS,
            LEGACY_SKILLS_APP,
            |_| false,
        )
        .unwrap();

        assert_eq!(
            loc.dir,
            test_abs("/custom/config").join("symbrain").join("skills")
        );
        assert!(!loc.legacy);
    }

    #[test]
    fn resolve_location_skills_config_legacy_fallback() {
        let base = test_abs("/custom/config");
        let legacy_path = base.join(LEGACY_SKILLS_APP);

        let loc = resolve_location(
            Some(&base),
            None,
            Path::new(".config"),
            COMPONENT_SKILLS,
            LEGACY_SKILLS_APP,
            |p| p == legacy_path,
        )
        .unwrap();

        assert_eq!(loc.dir, legacy_path);
        assert!(loc.legacy);
    }

    #[test]
    fn resolve_location_skills_data_dir() {
        let base = test_abs("/custom/data");
        let loc = resolve_location(
            Some(&base),
            None,
            &Path::new(".local").join("share"),
            COMPONENT_SKILLS,
            LEGACY_SKILLS_APP,
            |_| false,
        )
        .unwrap();

        assert_eq!(
            loc.dir,
            test_abs("/custom/data").join("symbrain").join("skills")
        );
        assert!(!loc.legacy);
    }

    #[test]
    fn resolve_location_skills_cache_dir() {
        let base = test_abs("/custom/cache");
        let loc = resolve_location(
            Some(&base),
            None,
            Path::new(".cache"),
            COMPONENT_SKILLS,
            LEGACY_SKILLS_APP,
            |_| false,
        )
        .unwrap();

        assert_eq!(
            loc.dir,
            test_abs("/custom/cache").join("symbrain").join("skills")
        );
        assert!(!loc.legacy);
    }

    #[test]
    fn resolve_location_skills_cache_legacy_fallback() {
        let base = test_abs("/custom/cache");
        let legacy_path = base.join(LEGACY_SKILLS_APP);

        let loc = resolve_location(
            Some(&base),
            None,
            Path::new(".cache"),
            COMPONENT_SKILLS,
            LEGACY_SKILLS_APP,
            |p| p == legacy_path,
        )
        .unwrap();

        assert_eq!(loc.dir, legacy_path);
        assert!(loc.legacy);
    }

    #[test]
    fn resolve_location_returns_none_when_no_base_or_home() {
        assert_eq!(
            resolve_location(
                None,
                None,
                Path::new(".config"),
                COMPONENT_MEMORY,
                LEGACY_MEMORY_APP,
                |_| false,
            ),
            None
        );
        assert_eq!(
            resolve_location(
                Some(Path::new("relative")),
                None,
                Path::new(".config"),
                COMPONENT_MEMORY,
                LEGACY_MEMORY_APP,
                |_| false,
            ),
            None
        );
    }

    #[test]
    fn live_component_paths_resolve_when_home_is_present() {
        if xdg::home_dir().is_some() {
            let mem_cfg = memory_config_dir().expect("memory config dir resolves");
            assert!(!mem_cfg.dir.as_os_str().is_empty());
            assert!(mem_cfg.dir.ends_with(Path::new("symbrain").join("memory")) || mem_cfg.legacy);

            let mem_data = memory_data_dir().expect("memory data dir resolves");
            assert!(!mem_data.dir.as_os_str().is_empty());
            assert!(
                mem_data.dir.ends_with(Path::new("symbrain").join("memory")) || mem_data.legacy
            );

            let skills_cfg = skills_config_dir().expect("skills config dir resolves");
            assert!(!skills_cfg.dir.as_os_str().is_empty());
            assert!(
                skills_cfg
                    .dir
                    .ends_with(Path::new("symbrain").join("skills"))
                    || skills_cfg.legacy
            );

            let skills_data = skills_data_dir().expect("skills data dir resolves");
            assert!(!skills_data.dir.as_os_str().is_empty());
            assert!(
                skills_data
                    .dir
                    .ends_with(Path::new("symbrain").join("skills"))
                    || skills_data.legacy
            );

            let skills_cache = skills_cache_dir().expect("skills cache dir resolves");
            assert!(!skills_cache.dir.as_os_str().is_empty());
            assert!(
                skills_cache
                    .dir
                    .ends_with(Path::new("symbrain").join("skills"))
                    || skills_cache.legacy
            );
        }
    }
}
