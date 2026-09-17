//! Native embedded skills MCP tools implementation.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value, json};
use symbrain_skills::install::{self, InstallOptions};
use symbrain_skills::{RenderMetadata, load_bundle, render_target, validate};

use crate::GatewayError;
use crate::embedded::common::pretty;

pub(crate) fn dispatch(name: &str, value: &Value) -> Result<String, GatewayError> {
    match name {
        "skills_list" => list(value),
        "skills_inspect" => inspect(value),
        "skills_validate" => validate_tool(value),
        "skills_render_plan" => render_plan(value),
        "skills_install" => install_tool(value),
        "skills_targets_status" => targets_status(value),
        _ => Err(GatewayError::UnknownTool(name.to_string())),
    }
}

fn resolve_skills_dirs() -> (PathBuf, PathBuf, PathBuf) {
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let library_dir = if let Some(path) = std::env::var_os("SYMBRAIN_SKILLS_LIBRARY_DIR") {
        PathBuf::from(path)
    } else if let Some(data) = symbrain_core::xdg::data_dir() {
        data.join("skills").join("library")
    } else {
        home.join(".local")
            .join("share")
            .join("symskills")
            .join("library")
    };

    let base_dir = if let Some(path) = std::env::var_os("SYMBRAIN_SKILLS_BASE_DIR") {
        PathBuf::from(path)
    } else if let Some(data) = symbrain_core::xdg::data_dir() {
        data.join("skills").join("base")
    } else {
        home.join(".local")
            .join("share")
            .join("symskills")
            .join("base")
    };

    (library_dir, base_dir, home)
}

#[derive(Debug, Serialize)]
struct SkillItem {
    name: String,
    description: String,
    category: String,
    root: String,
}

#[derive(Debug, Serialize)]
struct ListResult {
    skills: Vec<SkillItem>,
    category_counts: BTreeMap<String, usize>,
    issues: Vec<String>,
}

fn list(_value: &Value) -> Result<String, GatewayError> {
    let (library_dir, _, _) = resolve_skills_dirs();
    let mut skills = Vec::new();
    let mut category_counts = BTreeMap::new();
    let mut issues = Vec::new();

    if let Ok(entries) = fs::read_dir(&library_dir) {
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();

        for path in paths {
            if !path.is_dir() {
                continue;
            }
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|s| s.starts_with('.'))
            {
                continue;
            }
            if !path.join("SKILL.md").exists() {
                continue;
            }
            match load_bundle(&path) {
                Ok(bundle) => {
                    let cat = bundle.frontmatter.category.clone();
                    if !cat.is_empty() {
                        *category_counts.entry(cat.clone()).or_insert(0) += 1;
                    }
                    skills.push(SkillItem {
                        name: bundle.frontmatter.name.clone(),
                        description: bundle.frontmatter.description.clone(),
                        category: cat,
                        root: path.to_string_lossy().into_owned(),
                    });
                }
                Err(err) => {
                    issues.push(format!("{}: {err}", path.display()));
                }
            }
        }
    }

    pretty(&ListResult {
        skills,
        category_counts,
        issues,
    })
}

fn resolve_bundle_path(value: &Value) -> Result<PathBuf, GatewayError> {
    if let Some(path_str) = value.get("path").and_then(Value::as_str) {
        let path = PathBuf::from(path_str);
        if !path.exists() {
            return Err(GatewayError::InvalidArguments(format!(
                "skill path does not exist: {path_str}"
            )));
        }
        return Ok(path);
    }
    if let Some(name_str) = value.get("name").and_then(Value::as_str) {
        let (library_dir, _, _) = resolve_skills_dirs();
        let path = library_dir.join(name_str);
        if !path.exists() {
            return Err(GatewayError::InvalidArguments(format!(
                "skill '{name_str}' not found in library"
            )));
        }
        return Ok(path);
    }
    Err(GatewayError::InvalidArguments(
        "either 'path' or 'name' is required".into(),
    ))
}

fn inspect(value: &Value) -> Result<String, GatewayError> {
    let path = resolve_bundle_path(value)?;
    let bundle = load_bundle(&path)
        .map_err(|err| GatewayError::InvalidArguments(format!("load skill bundle: {err}")))?;

    let res = json!({
        "name": bundle.frontmatter.name,
        "description": bundle.frontmatter.description,
        "category": bundle.frontmatter.category,
        "version": bundle.frontmatter.version,
        "author": bundle.frontmatter.author,
        "license": bundle.frontmatter.license,
        "root": bundle.root.to_string_lossy(),
        "resources_count": bundle.resources.len(),
    });
    pretty(&res)
}

fn validate_tool(value: &Value) -> Result<String, GatewayError> {
    let path = resolve_bundle_path(value)?;
    let bundle = load_bundle(&path)
        .map_err(|err| GatewayError::InvalidArguments(format!("load skill bundle: {err}")))?;

    let issues = validate(&bundle);
    let issue_messages: Vec<String> = issues.into_iter().map(|i| i.message).collect();
    let res = json!({
        "valid": issue_messages.is_empty(),
        "issues": issue_messages,
    });
    pretty(&res)
}

fn targets_status(value: &Value) -> Result<String, GatewayError> {
    let _scope = value.get("scope").and_then(Value::as_str).unwrap_or("user");
    let mut targets = Vec::new();

    for h in symbrain_harness::all() {
        let Some(skill_target) = h.skill_target.as_str() else {
            continue;
        };
        targets.push(json!({
            "target": h.name.as_str(),
            "installed": true,
            "managed_count": 0,
            "unmanaged_count": 0,
            "skill_root": skill_target,
        }));
    }

    let res = json!({ "targets": targets });
    pretty(&res)
}

fn render_plan(value: &Value) -> Result<String, GatewayError> {
    let path = resolve_bundle_path(value)?;
    let target_name = value
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or("opencode");

    let bundle = load_bundle(&path)
        .map_err(|err| GatewayError::InvalidArguments(format!("load skill bundle: {err}")))?;

    let rendered = render_target(&bundle, target_name, &RenderMetadata::default())
        .map_err(|err| GatewayError::InvalidArguments(format!("render skill: {err}")))?;

    let res = json!({
        "action": "planned",
        "target": target_name,
        "name": bundle.frontmatter.name,
        "files_count": rendered.files.len(),
    });
    pretty(&res)
}

fn install_tool(value: &Value) -> Result<String, GatewayError> {
    let path = resolve_bundle_path(value)?;
    let target_name = value
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or("opencode");
    let dry_run = value
        .get("dry_run")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mode = value
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("copy")
        .to_string();

    let bundle = load_bundle(&path)
        .map_err(|err| GatewayError::InvalidArguments(format!("load skill bundle: {err}")))?;

    let rendered = render_target(&bundle, target_name, &RenderMetadata::default())
        .map_err(|err| GatewayError::InvalidArguments(format!("render skill: {err}")))?;

    let (_, base_dir, home_dir) = resolve_skills_dirs();
    let install_opts = InstallOptions {
        home_dir,
        project_dir: None,
        base_dir: Some(base_dir),
        mode,
        allow_executable: false,
        force: false,
        dry_run,
        fault: None,
        events_path: None,
    };

    let result = install::install_rendered(&bundle, &rendered, &install_opts)
        .map_err(|err| GatewayError::InvalidArguments(format!("install skill: {err}")))?;

    let res = json!({
        "action": result.action,
        "target": result.target,
        "name": result.name,
        "path": result.path.to_string_lossy(),
    });
    pretty(&res)
}
