//! Discovery and history/restore tools retain forward-only per-skill git state.
use super::{Error, boolean, compact, text};
use serde_json::{Value, json};
use symbrain_skills::{config, vcs};

fn integer(value: &Value, key: &str) -> Result<i64, Error> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(0),
        Some(value) => value.as_i64().ok_or_else(|| {
            Error::validation("parse arguments", format!("field {key} must be an integer"))
        }),
    }
}
pub(super) fn discover(value: &Value) -> Result<String, Error> {
    let _scope = text(value, "scope")?;
    let paths = match value.get("paths") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value.as_str().map(str::to_owned).ok_or_else(|| {
                    Error::validation("parse arguments", "paths must contain strings")
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(Error::validation(
                "parse arguments",
                "paths must be an array",
            ));
        }
    };
    let rows = symbrain_skills::discover::scanned(&config::home_dir(), None, "user", &paths)
        .map_err(|error| Error::validation("discover sources", error))?;
    compact(&json!({"candidates":rows}))
}
pub(super) fn history(value: &Value) -> Result<String, Error> {
    let name = text(value, "name")?;
    let limit = integer(value, "limit")?;
    if name.is_empty() {
        return Err(Error::validation("history", "name is required"));
    }
    let dir = config::defaults().library_dir.join(&name);
    if !vcs::is_repo(&dir) {
        return Err(Error::validation(
            "history",
            format!(
                "skill {name:?} is not versioned: no git repository at {}",
                dir.display()
            ),
        ));
    }
    let history =
        vcs::history(&dir, limit).map_err(|error| Error::internal("read history", error))?;
    compact(&json!({"name":name,"history":history}))
}
pub(super) fn restore(value: &Value) -> Result<String, Error> {
    let name = text(value, "name")?;
    let rev = text(value, "rev")?;
    let dry = boolean(value, "dry_run", true)?;
    let allow_dirty = boolean(value, "allow_dirty", false)?;
    let sync = boolean(value, "sync", false)?;
    if name.is_empty() || rev.is_empty() {
        return Err(Error::validation(
            "restore skill",
            "name and rev are required",
        ));
    }
    symbrain_skills::validate_skill_name(&name)
        .map_err(|error| Error::validation("restore skill", error))?;
    let cfg = config::defaults();
    let dir = cfg.library_dir.join(&name);
    let _owner = if dry {
        None
    } else {
        Some(
            vcs::lock_repository(&dir)
                .map_err(|error| Error::internal("lock restored skill", error))?,
        )
    };
    if !vcs::is_repo(&dir) {
        return Err(Error::validation(
            "restore skill",
            format!(
                "skill {name:?} is not versioned: no git repository at {}",
                dir.display()
            ),
        ));
    }
    let resolved = vcs::resolve(&dir, &rev).map_err(|error| {
        Error::validation(
            "restore skill",
            format!("revision {rev:?} not found: {error}"),
        )
    })?;
    let dirty = vcs::dirty(&dir).map_err(|error| Error::internal("check working tree", error))?;
    if dirty && !allow_dirty {
        return Err(Error {
            message: format!(
                "restore skill: skill {name:?} has uncommitted changes; they are never discarded — pass allow_dirty=true to snapshot them into a pre-restore commit first"
            ),
            code: Some("conflict"),
        });
    }
    let tmp = temp_directory()?;
    let restored = tmp.path().join(&name);
    vcs::extract(&dir, &resolved, &restored)
        .map_err(|error| Error::internal("extract revision", error))?;
    let bundle = symbrain_skills::load_bundle(&restored).map_err(|error| {
        Error::validation(
            "restore skill",
            format!("refusing restore to {resolved}: restored state is not a valid skill: {error}"),
        )
    })?;
    let errors: Vec<_> = symbrain_skills::validate(&bundle)
        .into_iter()
        .filter(|issue| issue.severity == "error")
        .map(|issue| issue.message)
        .collect();
    if !errors.is_empty() {
        return Err(Error::validation(
            "restore skill",
            format!(
                "refusing restore to {resolved}: restored state fails validation: {}",
                errors.join("; ")
            ),
        ));
    }
    let changed = vcs::changed(&dir, &resolved)
        .map_err(|error| Error::internal("compare revision", error))?;
    let mut notes = vec![
        "Restore creates a new forward commit; it never resets or rewrites history.",
        "Run symskills sync to reinstall targets that become stale after the restore.",
    ];
    if dirty {
        notes.push("Uncommitted changes would be committed as a pre-restore snapshot.");
    }
    let mut result = json!({"name":name,"rev":resolved,"dry_run":dry,"action":"planned","changed_files":changed,"notes":notes});
    if dry {
        return compact(&result);
    }
    if dirty {
        vcs::commit(
            &dir,
            &format!("restore: snapshot uncommitted changes before restore to {resolved}"),
        )
        .map_err(|error| Error::internal("snapshot uncommitted changes", error))?;
    }
    let head = vcs::restore(
        &dir,
        &restored,
        &format!("restore: skill {name} to {resolved}"),
    )
    .map_err(|error| Error::internal("restore skill", error))?;
    result["action"] = json!("restored");
    result["head"] = json!(head);
    if sync {
        let statuses = symbrain_skills::install::status(&symbrain_skills::install::StatusOptions {
            home_dir: config::home_dir(),
            library_dir: cfg.library_dir.clone(),
            base_dir: Some(cfg.base_dir.clone()),
            ..Default::default()
        })
        .map_err(|error| Error::internal("post-restore status", error))?;
        let stale = statuses
            .iter()
            .filter(|row| {
                row.name == name
                    && matches!(
                        row.status,
                        symbrain_skills::install::StatusKind::Stale
                            | symbrain_skills::install::StatusKind::HarnessChanged
                    )
            })
            .cloned()
            .collect::<Vec<_>>();
        let rows = if stale.is_empty() {
            Value::Null
        } else {
            json!(
                symbrain_skills::install::sync_selected(
                    &symbrain_skills::install::SyncOptions {
                        home_dir: config::home_dir(),
                        library_dir: cfg.library_dir,
                        base_dir: Some(cfg.base_dir),
                        render_dir: Some(cfg.render_dir),
                        skills: vec![name],
                        ..Default::default()
                    },
                    &stale
                )
                .map_err(|error| Error::internal("sync restored skill", error))?
                .into_iter()
                .map(|row| {
                    let mut out = json!({"target":row.target,"name":row.name,"action":row.action});
                    if !row.error.is_empty() {
                        out["error"] = json!(row.error);
                    }
                    if !row.path.as_os_str().is_empty() {
                        out["path"] = json!(row.path);
                    }
                    out
                })
                .collect::<Vec<_>>()
            )
        };
        result["synced"] = rows;
    }
    compact(&result)
}
fn temp_directory() -> Result<tempfile::TempDir, Error> {
    tempfile::Builder::new()
        .prefix("symskills-restore-")
        .tempdir()
        .map_err(|error| Error::internal("restore skill", error))
}
