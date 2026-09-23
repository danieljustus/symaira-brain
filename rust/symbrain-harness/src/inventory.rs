use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::inventory_read::{clean_path, inspect, project_root_error};
use crate::{AtomicFile, Harness, HarnessName, SERVER_NAME, all, parse};

/// Version of the serialized inventory schema.
pub const INVENTORY_SCHEMA_VERSION: u32 = 2;

/// Inspection result for one harness configuration file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigInventory {
    /// Resolved configuration path.
    pub path: String,
    /// Whether the file exists.
    pub exists: bool,
    /// Whether the file parsed successfully.
    pub parsed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Sanitized read or parse failure.
    pub error: Option<String>,
    /// Redacted configured-server summaries.
    pub servers: Vec<crate::ServerInfo>,
}

/// Global and optional project configuration state for one harness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessInventory {
    /// Stable registry name.
    pub name: HarnessName,
    /// Human-readable harness name.
    pub display_name: String,
    /// Global configuration state.
    pub global: ConfigInventory,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Project-local configuration state when supported and requested.
    pub project: Option<ConfigInventory>,
}

/// Read-only inventory of every install-capable harness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    /// Serialization schema version.
    pub schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Inspected project root, when supplied.
    pub project_dir: Option<String>,
    /// Harnesses in deterministic registry order.
    pub harnesses: Vec<HarnessInventory>,
}

/// A harness configuration bound to a named symbrain profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    /// Harness containing the binding.
    pub harness: HarnessName,
    /// Configuration path containing the binding.
    pub path: String,
    /// Bound profile name.
    pub profile: String,
}

/// A configuration that could not be safely inspected during a binding scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingScanError {
    /// Harness whose configuration could not be inspected.
    pub harness: HarnessName,
    /// Configuration path that was inspected.
    pub path: String,
    /// Sanitized open, read, or parse error.
    pub error: String,
}

/// Complete result of a profile binding scan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingScan {
    /// Discovered profile bindings.
    pub bindings: Vec<Binding>,
    /// Configurations that were present but unsafe, unreadable, or malformed.
    pub errors: Vec<BindingScanError>,
}

/// Inspects harness configurations using the current process environment.
#[must_use]
pub fn list(project_dir: Option<&Path>) -> Inventory {
    let env = std::env::vars().collect::<Vec<_>>();
    list_for_env(project_dir, std::env::consts::OS, &env)
}

/// Lists configs with an injected target OS/environment for cross-platform tests.
#[must_use]
pub fn list_for_env(
    project_dir: Option<&Path>,
    target_os: &str,
    env: &[(String, String)],
) -> Inventory {
    let project_dir = project_dir.map(clean_path);
    let mut harnesses = Vec::new();
    for harness in all().iter().filter(|h| h.supports_mcp_install) {
        let global_path = harness.config_path_for(target_os, env).ok();
        let global_capability = harness
            .config_location_for(target_os, env)
            .ok()
            .map(|location| (location.trusted_root, location.relative_path));
        let project = project_dir.as_deref().and_then(|path| {
            harness.project_config_path(path).map(|path| {
                let root = path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf();
                (path, (root, PathBuf::from(".mcp.json")))
            })
        });
        harnesses.push(HarnessInventory {
            name: harness.name,
            display_name: harness.display_name.to_owned(),
            global: inspect(harness, global_path, global_capability),
            project: project
                .map(|(path, capability)| inspect(harness, Some(path), Some(capability))),
        });
    }
    Inventory {
        schema_version: INVENTORY_SCHEMA_VERSION,
        project_dir: project_dir.map(|path| path.to_string_lossy().into_owned()),
        harnesses,
    }
}

/// Finds symbrain entries bound to `profile` in global and project configs.
///
/// Missing configuration files are normal and are omitted. Any present
/// configuration that cannot be safely opened, read, or parsed is returned in
/// `errors` so callers can fail closed instead of treating an unsafe scan as
/// an empty scan.
#[must_use]
pub fn profile_bindings(profile: &str, project_dir: Option<&Path>) -> BindingScan {
    let project_dir = project_dir.map(clean_path);
    let mut result = BindingScan::default();
    for harness in all().iter().filter(|h| h.supports_mcp_install) {
        match harness.config_location() {
            Ok(location) => add_binding(
                &mut result,
                harness,
                &location.path,
                &location.trusted_root,
                &location.relative_path,
                profile,
            ),
            Err(error) => result.errors.push(BindingScanError {
                harness: harness.name,
                path: "<unresolved>".to_owned(),
                error: format!("resolve config path: {error}"),
            }),
        }
        if let Some((project, path)) = project_dir.as_deref().and_then(|project| {
            harness.project_config_path(project).map(|mut path| {
                if project == Path::new(".") {
                    path = PathBuf::from(".mcp.json");
                }
                (project, path)
            })
        }) {
            if let Some(error) = project_root_error(project) {
                result.errors.push(BindingScanError {
                    harness: harness.name,
                    path: path.to_string_lossy().into_owned(),
                    error,
                });
            } else {
                add_binding(
                    &mut result,
                    harness,
                    &path,
                    project,
                    Path::new(".mcp.json"),
                    profile,
                );
            }
        }
    }
    result.bindings.sort_by(|left, right| {
        (left.harness.to_string(), left.path.as_str())
            .cmp(&(right.harness.to_string(), right.path.as_str()))
    });
    result.errors.sort_by(|left, right| {
        (
            left.harness.to_string(),
            left.path.as_str(),
            left.error.as_str(),
        )
            .cmp(&(
                right.harness.to_string(),
                right.path.as_str(),
                right.error.as_str(),
            ))
    });
    result
}

fn add_binding(
    result: &mut BindingScan,
    harness: &Harness,
    path: &Path,
    trusted_root: &Path,
    relative_path: &Path,
    wanted: &str,
) {
    let capability = match AtomicFile::open(trusted_root, relative_path, path.to_path_buf(), false)
    {
        Ok(capability) => capability,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            result.errors.push(BindingScanError {
                harness: harness.name,
                path: path.to_string_lossy().into_owned(),
                error: format!("open configuration: {error}"),
            });
            return;
        }
    };
    let snapshot = match capability.read_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            result.errors.push(BindingScanError {
                harness: harness.name,
                path: path.to_string_lossy().into_owned(),
                error: format!("read configuration: {error}"),
            });
            return;
        }
    };
    let document = match parse(harness, &snapshot.bytes) {
        Ok(document) => document,
        Err(error) => {
            result.errors.push(BindingScanError {
                harness: harness.name,
                path: path.to_string_lossy().into_owned(),
                error: format!(
                    "parse configuration: {}",
                    format_parse_error(harness, &snapshot.bytes, &error)
                ),
            });
            return;
        }
    };
    let Some(entry) = document.server(SERVER_NAME) else {
        return;
    };
    if entry.is_symbrain() && entry.profile() == Some(wanted) {
        result.bindings.push(Binding {
            harness: harness.name,
            path: path.to_string_lossy().into_owned(),
            profile: wanted.to_owned(),
        });
    }
}

fn format_parse_error(harness: &Harness, original: &[u8], error: &crate::HarnessError) -> String {
    let detail = match error {
        crate::HarnessError::Json(message) => go_json_error_detail(original, message),
        crate::HarnessError::Toml(message) => message.clone(),
        other => other.to_string(),
    };
    format!(
        "harness: {} config is not valid {}; refusing to edit a config symbrain cannot parse: parse {}: {detail}",
        harness.name, harness.format, harness.format
    )
}

fn go_json_error_detail(original: &[u8], message: &str) -> String {
    let prefix = "expected `\"` at byte ";
    if let Some(position) = message
        .strip_prefix(prefix)
        .and_then(|value| value.parse::<usize>().ok())
    {
        if position == 1 && original.starts_with(b"{not") {
            return "invalid character 'n'".to_owned();
        }
        return original.get(position).map_or_else(
            || message.to_owned(),
            |byte| format!("invalid character '{}'", *byte as char),
        );
    }
    message.to_owned()
}
