//! Native skill-target inventory status.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use symbrain_skills::install;

use crate::GatewayError;
use crate::embedded::common::pretty;

pub(super) fn targets_status(value: &Value) -> Result<String, GatewayError> {
    let scope = if value.get("scope").and_then(Value::as_str) == Some("project") {
        "project"
    } else {
        "user"
    };
    let project_dir = (scope == "project")
        .then(|| {
            std::env::current_dir().map_err(|error| {
                GatewayError::InvalidArguments(format!("resolve project directory: {error}"))
            })
        })
        .transpose()?;
    let (_, _, home_dir) = super::resolve_skills_dirs();
    let mut targets = Vec::new();

    for harness in symbrain_harness::all() {
        let Some(target) = harness.skill_target.as_str() else {
            continue;
        };
        targets.push(target_status(
            target,
            &home_dir,
            project_dir.as_deref(),
            scope,
        )?);
    }

    pretty(&json!({ "targets": targets }))
}

fn target_status(
    target: &str,
    home_dir: &Path,
    project_dir: Option<&Path>,
    scope: &str,
) -> Result<Value, GatewayError> {
    let skill_root =
        install::install_path_for(target, home_dir, project_dir, scope, "status-placeholder")
            .map_err(|error| {
                GatewayError::InvalidArguments(format!("resolve skill root: {error}"))
            })?
            .parent()
            .ok_or_else(|| GatewayError::InvalidArguments("skill root has no parent".into()))?
            .to_path_buf();

    let root_metadata = fs::symlink_metadata(&skill_root).ok();
    let root_exists = root_metadata.is_some();
    let (skill_root_readable, managed, unmanaged) = if root_exists {
        match count_entries(&skill_root) {
            Ok((managed, unmanaged)) => (true, managed, unmanaged),
            Err(_) => (false, 0, 0),
        }
    } else {
        (false, 0, 0)
    };
    let install_state = if !root_exists {
        "missing"
    } else if !skill_root_readable {
        "unreadable"
    } else if managed > 0 && unmanaged == 0 {
        "managed"
    } else if managed > 0 && unmanaged > 0 {
        "mixed"
    } else if unmanaged > 0 {
        "unmanaged"
    } else {
        "empty"
    };
    let setup_hint = match install_state {
        "missing" => format!(
            "Create skill directory {} or run 'symskills install --target {target} <skill>'",
            skill_root.display()
        ),
        "unreadable" => format!("Check permissions for skill root {}", skill_root.display()),
        "managed" => format!("Harness is active with {managed} managed skill(s)"),
        "mixed" => format!("Harness contains {managed} managed and {unmanaged} unmanaged skill(s)"),
        "unmanaged" => format!(
            "Harness contains {unmanaged} unmanaged skill(s); consider importing into library"
        ),
        _ => format!(
            "Harness skill directory is ready; install skills with 'symskills install --target {target} <skill>'"
        ),
    };

    let config_dir = config_dir_for(target, &skill_root);
    let config_exists = fs::metadata(&config_dir).is_ok_and(|metadata| metadata.is_dir());
    let mut evidence = Vec::new();
    if let Some(path) = binary_for(target).as_deref() {
        evidence.push(format!("binary:{}", path.display()));
    }
    if config_exists {
        evidence.push(format!("config_dir:{}", config_dir.display()));
    }
    let installed = !evidence.is_empty();

    Ok(json!({
        "target": target,
        "display_name": display_name(target),
        "installed": installed,
        "evidence": if evidence.is_empty() { "none".to_owned() } else { evidence.join(",") },
        "effective_skill_root": skill_root,
        "skill_root_exists": root_exists,
        "skill_root_readable": skill_root_readable,
        "managed_skills_count": managed,
        "unmanaged_skills_count": unmanaged,
        "install_state": install_state,
        "capabilities": ["render", "install", "symlink", "copy"],
        "runtime_capabilities": runtime_capabilities(target),
        "setup_hint": setup_hint,
        "verification_status": if installed { "verified" } else { "not_verified" }
    }))
}

fn count_entries(root: &Path) -> std::io::Result<(usize, usize)> {
    let mut managed = 0;
    let mut unmanaged = 0;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if marker_exists(&path) {
            managed += 1;
        } else {
            let file_type = entry.file_type()?;
            if file_type.is_dir() || file_type.is_symlink() {
                unmanaged += 1;
            }
        }
    }
    Ok((managed, unmanaged))
}

fn marker_exists(path: &Path) -> bool {
    if fs::metadata(path.join(install::MARKER_FILE)).is_ok() {
        return true;
    }
    if !path.is_symlink() {
        return false;
    }
    let Ok(mut target) = fs::read_link(path) else {
        return false;
    };
    if target.is_relative() {
        target = path.parent().unwrap_or_else(|| Path::new(".")).join(target);
    }
    fs::metadata(target.join(install::MARKER_FILE)).is_ok()
}

fn display_name(target: &str) -> String {
    match target {
        "opencode" => "OpenCode".to_owned(),
        "claude" => "Claude Code".to_owned(),
        "codex" => "Codex".to_owned(),
        "hermes" => "Hermes".to_owned(),
        "antigravity" => "Antigravity".to_owned(),
        "openclaw" => "OpenClaw".to_owned(),
        _ => target.to_owned(),
    }
}

fn config_dir_for(target: &str, skill_root: &Path) -> PathBuf {
    if target == "hermes"
        && skill_root
            .file_name()
            .is_some_and(|name| name == std::ffi::OsStr::new("symaira"))
    {
        skill_root
            .parent()
            .and_then(Path::parent)
            .map_or_else(|| skill_root.to_path_buf(), Path::to_path_buf)
    } else {
        skill_root
            .parent()
            .map_or_else(|| skill_root.to_path_buf(), Path::to_path_buf)
    }
}

fn binary_for(target: &str) -> Option<PathBuf> {
    let name = match target {
        "opencode" => "opencode",
        "claude" => "claude",
        "codex" => "codex",
        "hermes" => "hermes",
        "antigravity" => "agy",
        "openclaw" => "openclaw",
        _ => return None,
    };
    let names = binary_names(name);
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|path| path.is_file() && executable(path))
}

#[cfg(windows)]
fn binary_names(name: &str) -> Vec<OsString> {
    let mut names = vec![OsString::from(name)];
    if Path::new(name).extension().is_none()
        && let Some(extensions) = std::env::var_os("PATHEXT")
    {
        names.extend(
            extensions
                .to_string_lossy()
                .split(';')
                .filter(|extension| !extension.is_empty())
                .map(|extension| OsString::from(format!("{name}{extension}"))),
        );
    }
    names
}

#[cfg(not(windows))]
fn binary_names(name: &str) -> Vec<OsString> {
    vec![OsString::from(name)]
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(path: &Path) -> bool {
    path.is_file()
}

fn runtime_capabilities(target: &str) -> BTreeMap<&'static str, &'static str> {
    let mut states = BTreeMap::from([
        ("subagents", "unknown"),
        ("background_tasks", "unknown"),
        ("mcp", "unknown"),
        ("slash_commands", "unknown"),
        ("hooks", "unknown"),
        ("scheduled_tasks", "unknown"),
    ]);
    if matches!(target, "claude" | "hermes") {
        states.insert("subagents", "supported");
    }
    states
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{display_name, target_status};

    #[test]
    fn target_status_matches_harness_inventory_semantics() {
        assert_eq!(display_name("codex"), "Codex");
        let root = std::env::temp_dir().join(format!(
            "symbrain-gateway-status-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("test root");
        let home = root.join("home");
        fs::create_dir_all(&home).expect("home");

        let missing = target_status("opencode", &home, None, "user").expect("missing");
        assert_eq!(missing["skill_root_exists"], false);
        assert_eq!(missing["managed_skills_count"], 0);
        assert_eq!(missing["install_state"], "missing");

        let skill_root = home.join(".config/opencode/skills");
        fs::create_dir_all(skill_root.join("manual")).expect("manual skill root");
        fs::write(skill_root.join("manual/.symskills.json"), b"not-json").expect("marker");
        #[cfg(unix)]
        std::os::unix::fs::symlink("manual", skill_root.join("linked")).expect("linked skill");
        fs::create_dir_all(skill_root.join("unmanaged")).expect("unmanaged skill");
        let present = target_status("opencode", &home, None, "user").expect("present");
        assert_eq!(present["display_name"], "OpenCode");
        #[cfg(unix)]
        assert_eq!(present["managed_skills_count"], 2);
        #[cfg(not(unix))]
        assert_eq!(present["managed_skills_count"], 1);
        assert_eq!(present["unmanaged_skills_count"], 1);
        assert_eq!(present["install_state"], "mixed");

        #[cfg(unix)]
        {
            let real_root = root.join("real-opencode-skills");
            fs::rename(&skill_root, &real_root).expect("move real skill root");
            std::os::unix::fs::symlink(&real_root, &skill_root).expect("link skill root");
            let linked =
                target_status("opencode", &home, None, "user").expect("linked root status");
            assert_eq!(linked["skill_root_exists"], true);
            assert_eq!(linked["skill_root_readable"], true);
            assert_eq!(linked["managed_skills_count"], 2);
            assert_eq!(linked["unmanaged_skills_count"], 1);
            assert_eq!(linked["install_state"], "mixed");
            fs::remove_file(&skill_root).expect("remove skill root link");
            fs::rename(real_root, skill_root).expect("restore real skill root");
        }

        let project = root.join("project");
        fs::create_dir_all(project.join(".hermes/skills/manual")).expect("project skill root");
        let project_status =
            target_status("hermes", &home, Some(&project), "project").expect("project");
        assert_eq!(project_status["display_name"], "Hermes");
        let expected = project
            .join(".hermes")
            .join("skills")
            .to_string_lossy()
            .into_owned();
        assert_eq!(project_status["effective_skill_root"], expected);
        fs::remove_dir_all(root).expect("test root cleanup");
    }
}
