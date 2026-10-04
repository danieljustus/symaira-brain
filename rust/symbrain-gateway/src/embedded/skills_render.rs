//! Render/install MCP defaults and profile paths reuse the confined skills core.
use super::{Error, boolean, bundle, compact, text};
use serde_json::{Value, json};
use symbrain_skills::{
    Bundle, RenderMetadata, config, context_profile, install, materialize, render_target, wire,
};

fn targets(value: &Value, default_all: bool) -> Result<Vec<String>, Error> {
    let target = text(value, "target")?;
    if target.is_empty() {
        return Ok(if default_all {
            symbrain_skills::default_targets()
        } else {
            vec!["opencode".into()]
        });
    }
    if symbrain_skills::lookup(&target).is_none() {
        return Err(Error {
            message: format!(
                "unknown target {target:?} (valid: all, {})",
                symbrain_skills::default_targets().join(", ")
            ),
            code: None,
        });
    }
    Ok(vec![target])
}
fn profile_rows(value: &Value) -> Result<Option<Vec<context_profile::Resolved>>, Error> {
    let name = text(value, "profile")?;
    if name.is_empty() {
        return Ok(None);
    }
    let cfg = config::defaults();
    let (rows, issues) = context_profile::resolve(&cfg.library_dir, &cfg.profiles_dir, None, &name)
        .map_err(|error| Error::validation("resolve profile", error))?;
    if !issues.is_empty() {
        return Err(Error {
            message: compact(&json!({"issues":issues}))?,
            code: Some("profile_issues"),
        });
    }
    Ok(Some(rows))
}
fn render(bundle: &Bundle, target: &str, meta: &RenderMetadata, dry: bool) -> Result<Value, Error> {
    let item = render_target(bundle, target, meta)?;
    let cfg = config::defaults();
    let path = cfg.render_dir.join(target).join(&item.name);
    if !dry {
        materialize(bundle, &item, &cfg.render_dir)?;
    }
    Ok(wire::rendered(&item, &path))
}
pub(super) fn plan(value: &Value) -> Result<String, Error> {
    let targets = targets(value, true)?;
    let dry = boolean(value, "dry_run", false)?;
    let profile = match profile_rows(value) {
        Ok(rows) => rows,
        Err(error) if error.code == Some("profile_issues") => {
            let issues: Value = serde_json::from_str(&error.message)
                .map_err(|error| Error::internal("serialize issues", error))?;
            return compact(&json!({"skills":[],"issues":issues["issues"]}));
        }
        Err(error) => return Err(error),
    };
    let mut rows = Vec::new();
    let mut first_error = None;
    if let Some(profile) = profile {
        for row in profile {
            let cfg = config::defaults();
            let bundle = match symbrain_skills::load_bundle(&cfg.library_dir.join(&row.skill)) {
                Ok(bundle) => bundle,
                Err(error) if !dry => {
                    if first_error.is_none() {
                        first_error = Some(Error {
                            message: format!("profile link {:?}: {error}", row.name),
                            code: None,
                        });
                    }
                    continue;
                }
                Err(error) => {
                    return Err(Error {
                        message: format!("profile link {:?}: {error}", row.name),
                        code: None,
                    });
                }
            };
            let meta = RenderMetadata {
                source: row.source,
                profile: row.profile,
                alias: if dry { String::new() } else { row.alias },
                ..RenderMetadata::default()
            };
            for target in &targets {
                match render(&bundle, target, &meta, dry) {
                    Ok(row) => rows.push(row),
                    Err(error) if !dry => {
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                    Err(_) => {}
                }
            }
        }
        if let Some(error) = first_error {
            return Err(Error::validation("resolve profile", error.message));
        }
        return compact(&if rows.is_empty() {
            Value::Null
        } else {
            json!(rows)
        });
    }
    let bundle = bundle(value)?;
    for target in &targets {
        match render(&bundle, target, &RenderMetadata::default(), dry) {
            Ok(row) => rows.push(row),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(Error {
                        message: format!("target {target}: {}", error.message),
                        code: None,
                    });
                }
            }
        }
    }
    if rows.is_empty()
        && let Some(error) = first_error
    {
        return Err(error);
    }
    compact(&rows)
}
fn install_one(
    bundle: &Bundle,
    target: &str,
    meta: &RenderMetadata,
    scope: &str,
    dry: bool,
) -> Result<Value, Error> {
    let cfg = config::defaults();
    let item = render_target(bundle, target, meta)?;
    let home = config::home_dir();
    let project = if scope == "project" {
        std::env::current_dir().ok()
    } else {
        None
    };
    let path = install::install_path_for(target, &home, project.as_deref(), scope, &item.name)?;
    if dry {
        return Ok(
            json!({"action":"planned","target":target,"name":item.name,"path":symbrain_skills::GoText::from_path(&path),"mode":""}),
        );
    }
    // Go renders before installing even for project scope without a project.
    materialize(bundle, &item, &cfg.render_dir)?;
    let result = install::install_rendered(
        bundle,
        &item,
        &install::InstallOptions {
            legacy_project_base: scope == "project",
            home_dir: home,
            project_dir: project,
            base_dir: None,
            render_dir: Some(cfg.render_dir),
            mode: String::new(),
            force: false,
            dry_run: false,
            allow_executable: false,
            fault: None,
            events_path: None,
        },
    )?;
    let mut wire = json!({"action":result.action,"target":result.target,"name":result.name,"path":symbrain_skills::GoText::from_path(&result.path),"mode":result.mode});
    if let Some(path) = result.backup_path {
        wire["backup_path"] = json!(symbrain_skills::GoText::from_path(&path));
    }
    if !result.mode_changes.is_empty() {
        wire["mode_changes"] = json!(result.mode_changes);
    }
    Ok(wire)
}
pub(super) fn install(value: &Value) -> Result<String, Error> {
    let targets = targets(value, false)?;
    let target = &targets[0];
    let dry = boolean(value, "dry_run", true)?;
    let scope = text(value, "scope")?;
    let scope = if scope == "project" {
        "project"
    } else {
        "user"
    };
    match profile_rows(value) {
        Ok(Some(profile)) => {
            let mut results = Vec::new();
            let mut first_error = None;
            for row in profile {
                let cfg = config::defaults();
                let bundle = match symbrain_skills::load_bundle(&cfg.library_dir.join(&row.skill)) {
                    Ok(bundle) => bundle,
                    Err(error) if !dry => {
                        if first_error.is_none() {
                            first_error = Some(Error {
                                message: format!("profile link {:?}: {error}", row.name),
                                code: None,
                            });
                        }
                        continue;
                    }
                    Err(error) => {
                        return Err(Error {
                            message: format!("profile link {:?}: {error}", row.name),
                            code: None,
                        });
                    }
                };
                let meta = RenderMetadata {
                    source: row.source,
                    profile: row.profile,
                    alias: if dry { String::new() } else { row.alias },
                    ..RenderMetadata::default()
                };
                if dry && render_target(&bundle, target, &meta).is_err() {
                    continue;
                }
                match install_one(&bundle, target, &meta, scope, dry) {
                    Ok(result) => results.push(result),
                    Err(error) if !dry => {
                        if first_error.is_none() {
                            first_error = Some(Error {
                                message: format!("profile link {:?}: {}", row.name, error.message),
                                code: None,
                            });
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
            if let Some(error) = first_error {
                return Err(Error::validation("resolve profile", error.message));
            }
            compact(&if results.is_empty() {
                Value::Null
            } else {
                json!(results)
            })
        }
        Ok(None) => compact(&install_one(
            &bundle(value)?,
            target,
            &RenderMetadata::default(),
            scope,
            dry,
        )?),
        Err(error) if error.code == Some("profile_issues") => {
            let issues: Value = serde_json::from_str(&error.message)
                .map_err(|error| Error::internal("serialize issues", error))?;
            compact(&json!({"results":[],"issues":issues["issues"]}))
        }
        Err(error) => Err(error),
    }
}
