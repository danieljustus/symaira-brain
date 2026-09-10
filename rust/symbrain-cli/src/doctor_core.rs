use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use symbrain_core::xdg;
use toml_edit::DocumentMut;

use super::doctor_process::run_process;
use super::doctor_types::{
    HARNESSES, HarnessCheck, HarnessSpec, MemoryDbCheck, SkillsLibraryCheck,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Deserialize)]
struct VersionPayload {
    version: String,
}

pub(super) fn probe_version(path: &Path) -> Result<String, String> {
    probe_version_with_args(path, &["version", "--json"])
}

pub(super) fn probe_version_with_args(path: &Path, args: &[&str]) -> Result<String, String> {
    let (status, stdout, _stderr) = run_process(path, args, PROBE_TIMEOUT)?;
    if !status.success() {
        let detail = status
            .code()
            .map_or_else(|| status.to_string(), |code| format!("exit status {code}"));
        return Err(format!("run {} version --json: {detail}", path.display()));
    }
    serde_json::from_slice::<VersionPayload>(&stdout)
        .map(|payload| payload.version)
        .map_err(|error| format!("parse version --json output: {error}"))
}

pub(super) fn check_memory_db() -> MemoryDbCheck {
    let Some(data) = component_location("memory", "symmemory", ".local/share") else {
        return MemoryDbCheck {
            path: String::new(),
            exists: false,
            mode: String::new(),
            mode_ok: false,
            quick_check: String::new(),
            legacy: false,
            error: "resolve memory data directory".to_string(),
        };
    };
    let path = data.path.join("default.db");
    let mut check = MemoryDbCheck {
        path: path.display().to_string(),
        exists: false,
        mode: String::new(),
        mode_ok: false,
        quick_check: String::new(),
        legacy: data.legacy,
        error: String::new(),
    };
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return check,
        Err(error) => {
            check.error = format!("stat database: {error}");
            return check;
        }
    };
    check.exists = true;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode() & 0o777;
        check.mode = format!("{mode:04o}");
        check.mode_ok = mode == 0o600;
    }
    #[cfg(not(unix))]
    {
        check.mode = "0600".to_string();
        check.mode_ok = true;
    }
    match rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(connection) => {
            let result =
                connection.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0));
            match result {
                Ok(value) => {
                    check.quick_check = value;
                    if check.quick_check != "ok" {
                        check.error = format!("quick_check: {}", check.quick_check);
                    }
                }
                Err(error) => check.error = format!("quick_check: {error}"),
            }
        }
        Err(error) => check.error = format!("open database: {error}"),
    }
    check
}

pub(super) fn check_skills_library() -> SkillsLibraryCheck {
    let Some(data) = component_location("skills", "symskills", ".local/share") else {
        return SkillsLibraryCheck {
            path: String::new(),
            exists: false,
            count: 0,
            legacy: false,
            error: "resolve skills data directory".to_string(),
        };
    };
    let path = data.path.join("library");
    let mut check = SkillsLibraryCheck {
        path: path.display().to_string(),
        exists: false,
        count: 0,
        legacy: data.legacy,
        error: String::new(),
    };
    match fs::metadata(&path) {
        Ok(metadata) => check.exists = metadata.is_dir(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return check,
        Err(error) => {
            check.error = format!("stat skills library: {error}");
            return check;
        }
    }
    let entries = match fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(error) => {
            check.error = format!("read skills library: {error}");
            return check;
        }
    };
    let mut issues = 0_usize;
    for entry in entries {
        match entry {
            Ok(entry) => match entry.file_type() {
                Ok(kind) if kind.is_dir() => match fs::read(entry.path().join("SKILL.md")) {
                    Ok(_) => check.count += 1,
                    Err(_) => issues += 1,
                },
                Ok(_) => {}
                Err(_) => issues += 1,
            },
            Err(_) => issues += 1,
        }
    }
    if issues > 0 {
        check.error = format!("{issues} skill(s) failed to load");
    }
    check
}

struct Location {
    path: PathBuf,
    legacy: bool,
}
fn component_location(component: &str, legacy_name: &str, _fallback: &str) -> Option<Location> {
    let base = xdg::data_dir()?;
    let current = base.join(component);
    if current.is_dir() {
        return Some(Location {
            path: current,
            legacy: false,
        });
    }
    let legacy = base.parent().unwrap_or(&base).join(legacy_name);
    if legacy.is_dir() {
        return Some(Location {
            path: legacy,
            legacy: true,
        });
    }
    Some(Location {
        path: current,
        legacy: false,
    })
}

pub(super) fn check_harnesses(profiles_dir: &Path) -> Vec<HarnessCheck> {
    HARNESSES
        .iter()
        .map(|spec| check_harness(*spec, profiles_dir))
        .collect()
}

pub(super) fn check_harness(spec: HarnessSpec, profiles_dir: &Path) -> HarnessCheck {
    let path = harness_path(spec).unwrap_or_default();
    let mut check = HarnessCheck {
        name: spec.name.to_string(),
        config_path: path.display().to_string(),
        config_found: false,
        config_parsed: false,
        config_error: String::new(),
        supports_mcp_install: spec.supports_mcp_install,
        installed: false,
        profile: String::new(),
        profile_exists: false,
        profile_missing: false,
        superseded: Vec::new(),
    };
    if !spec.supports_mcp_install {
        check.config_path.clear();
        check.config_error = "harness does not support MCP installation".to_string();
        return check;
    }
    if path.as_os_str().is_empty() {
        check.config_error = "resolve harness config path".to_string();
        return check;
    }
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return check,
        Err(error) => {
            check.config_error = error.to_string();
            return check;
        }
    };
    check.config_found = true;
    let servers = match parse_server_map(spec, &bytes) {
        Ok(servers) => {
            check.config_parsed = true;
            servers
        }
        Err(error) => {
            check.config_error = error;
            return check;
        }
    };
    for name in sorted_server_names(&servers) {
        let Some(entry) = servers.get(&name) else {
            continue;
        };
        let command = entry
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let base = Path::new(command)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or(command);
        if base == "symmemory" || base == "symskills" {
            check.superseded.push(format!("{name} (superseded {base})"));
        }
    }
    if let Some(entry) = servers.get("symbrain") {
        let command = entry
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let base = Path::new(command)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or(command);
        if base != "symbrain" {
            return check;
        }
        check.installed = true;
        let args = entry
            .get("args")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if let Some(profile) = profile_arg(&args) {
            check.profile.clone_from(&profile);
            check.profile_exists = profiles_dir.join(format!("{profile}.toml")).is_file();
            check.profile_missing = !check.profile_exists;
        }
    }
    check
}

pub(super) fn harness_path(spec: HarnessSpec) -> Option<PathBuf> {
    let home = xdg::home_dir()?;
    let config_home = user_config_dir(&home);
    match spec.name {
        "claude" => Some(home.join(".claude.json")),
        "claude-desktop" => {
            #[cfg(target_os = "macos")]
            {
                Some(home.join("Library/Application Support/Claude/claude_desktop_config.json"))
            }
            #[cfg(not(target_os = "macos"))]
            {
                Some(config_home?.join("Claude/claude_desktop_config.json"))
            }
        }
        "cursor" => Some(home.join(".cursor/mcp.json")),
        "opencode" => Some(config_home?.join("opencode/config.json")),
        "codex" => Some(home.join(".codex/config.toml")),
        "antigravity" => Some(home.join(".gemini/config/mcp_config.json")),
        _ => None,
    }
}

#[cfg(windows)]
fn user_config_dir(_home: &Path) -> Option<PathBuf> {
    env::var_os("APPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(not(windows))]
fn user_config_dir(home: &Path) -> Option<PathBuf> {
    match env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        Some(value) => {
            let path = PathBuf::from(value);
            path.is_absolute().then_some(path)
        }
        None => Some(home.join(".config")),
    }
}

pub(super) fn parse_server_map(
    spec: HarnessSpec,
    bytes: &[u8],
) -> Result<BTreeMap<String, serde_json::Value>, String> {
    if spec.format == "json" {
        let root: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        let map = root
            .get(spec.servers_key)
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        Ok(map.into_iter().collect())
    } else {
        let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
        let doc = text
            .parse::<DocumentMut>()
            .map_err(|error| error.to_string())?;
        let Some(table) = doc.get(spec.servers_key).and_then(|v| v.as_table()) else {
            return Ok(BTreeMap::new());
        };
        let mut result = BTreeMap::new();
        for (name, item) in table {
            let mut value = serde_json::Map::new();
            if let Some(table) = item.as_table() {
                if let Some(command) = table.get("command").and_then(|v| v.as_str()) {
                    value.insert("command".to_string(), command.into());
                }
                if let Some(args) = table.get("args").and_then(|v| v.as_array()) {
                    value.insert(
                        "args".to_string(),
                        serde_json::Value::Array(
                            args.iter()
                                .filter_map(|v| v.as_str().map(Into::into))
                                .collect(),
                        ),
                    );
                }
            }
            result.insert(name.to_string(), serde_json::Value::Object(value));
        }
        Ok(result)
    }
}

pub(super) fn sorted_server_names(map: &BTreeMap<String, serde_json::Value>) -> Vec<String> {
    map.keys().cloned().collect()
}
pub(super) fn profile_arg(args: &[serde_json::Value]) -> Option<String> {
    args.iter().enumerate().find_map(|(i, arg)| {
        let value = arg.as_str()?;
        if value == "--profile" {
            args.get(i + 1).and_then(|v| v.as_str()).map(str::to_owned)
        } else {
            value.strip_prefix("--profile=").map(str::to_owned)
        }
    })
}
