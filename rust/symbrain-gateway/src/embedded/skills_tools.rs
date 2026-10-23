//! Native profile and versioning Skills tools: `skills_profile_list`,
//! `skills_profile_resolve`, `skills_discover_sources`, `skills_history` and
//! `skills_restore`.
//!
//! Each tool reproduces the Go `internal/skills/mcptools` handler: typed
//! argument decoding (`json.Unmarshal` into an anonymous struct), the same
//! error texts and `exitcodes` kind in `_meta`, and compact Go-escaped JSON
//! with map keys in sorted order. Per-skill git state is read and written
//! through the `git` binary exactly as `internal/skills/vcs` does.

use super::skills_config::{self, Dirs};
use crate::{GatewayError, GatewayResponse};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use symbrain_skills::{context_profile, discover, gotext, install, vcs};

/// A handler failure: Go's `exitcodes.Wrap(err, ..., kind, context)`.
struct ToolError {
    message: String,
    code: &'static str,
}

impl ToolError {
    fn new(code: &'static str, context: &str, message: impl fmt::Display) -> Self {
        Self {
            message: format!("{context}: {message}"),
            code,
        }
    }
    fn validation(context: &str, message: impl fmt::Display) -> Self {
        Self::new("validation", context, message)
    }
    fn internal(context: &str, message: impl fmt::Display) -> Self {
        Self::new("internal", context, message)
    }
}

type Outcome = Result<String, ToolError>;

#[derive(Serialize)]
struct MetaError<'a> {
    code: &'static str,
    message: &'a str,
}
#[derive(Serialize)]
struct Meta<'a> {
    #[serde(rename = "symaira.dev/tool_error")]
    error: MetaError<'a>,
}
#[derive(Serialize)]
struct Content<'a> {
    text: &'a str,
    #[serde(rename = "type")]
    kind: &'static str,
}
#[derive(Serialize)]
struct Wire<'a> {
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    meta: Option<Meta<'a>>,
    content: [Content<'a>; 1],
    #[serde(rename = "isError")]
    is_error: bool,
}

/// Go's `json.Marshal` HTML escaping, applied to already-serialized JSON
/// (`&`, `<`, `>` and the JS line separators only occur inside strings).
fn go_escape(json: &str) -> String {
    json.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn compact<T: Serialize>(value: &T) -> Outcome {
    serde_json::to_string(value)
        .map(|json| go_escape(&json))
        .map_err(|error| ToolError::internal("serialize result", error))
}

/// Names served by this module.
pub(crate) fn handles(name: &str) -> bool {
    super::skills_catalog::TOOLS.contains(&name)
}

/// Runs one tool call against the raw `arguments` member (None = absent).
pub(crate) fn response(
    id: Value,
    name: &str,
    raw: Option<&RawValue>,
) -> Result<GatewayResponse, GatewayError> {
    let outcome = dispatch(name, raw);
    let (text, error) = match &outcome {
        Ok(text) => (text.as_str(), None),
        Err(error) => (error.message.as_str(), Some(error.code)),
    };
    let wire = Wire {
        meta: error.map(|code| Meta {
            error: MetaError {
                code,
                message: text,
            },
        }),
        content: [Content { text, kind: "text" }],
        is_error: error.is_some(),
    };
    let json = serde_json::to_string(&wire)
        .map_err(|error| GatewayError::Serialization(error.to_string()))?;
    let result = RawValue::from_string(go_escape(&json))
        .map_err(|error| GatewayError::Serialization(error.to_string()))?;
    GatewayResponse::success(id, result)
}

fn dispatch(name: &str, raw: Option<&RawValue>) -> Outcome {
    let dirs = skills_config::defaults();
    match name {
        "skills_profile_list" => profile_list(&dirs),
        "skills_profile_resolve" => profile_resolve(&dirs, raw),
        "skills_discover_sources" => discover_sources(raw),
        "skills_history" => history(&dirs, raw),
        "skills_restore" => restore(&dirs, raw),
        _ => Err(ToolError {
            message: format!("Unknown tool: {name}"),
            code: "internal",
        }),
    }
}

// ----------------------------------------------------------------------
// Typed argument decoding (Go anonymous structs).
// ----------------------------------------------------------------------

struct Fields(Vec<(String, Box<RawValue>)>);
impl<'de> Deserialize<'de> for Fields {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct Ordered;
        impl<'de> Visitor<'de> for Ordered {
            type Value = Fields;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Fields, E> {
                Ok(Fields(Vec::new()))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Fields, A::Error> {
                let mut fields = Vec::new();
                while let Some(pair) = map.next_entry()? {
                    fields.push(pair);
                }
                Ok(Fields(fields))
            }
        }
        decoder.deserialize_any(Ordered)
    }
}

fn kind(raw: &str) -> &'static str {
    match raw.trim_start().as_bytes().first() {
        Some(b'"') => "string",
        Some(b'{') => "object",
        Some(b'[') => "array",
        Some(b't' | b'f') => "bool",
        Some(b'n') => "null",
        _ => "number",
    }
}

/// `struct { Name string "json:\"name\""; ... }` as `reflect` prints it.
fn descriptor(specs: &[(&str, &str)]) -> String {
    let members: Vec<_> = specs
        .iter()
        .map(|(name, ty)| {
            let native: String = name
                .split('_')
                .map(|part| {
                    let mut chars = part.chars();
                    chars.next().map_or_else(String::new, |first| {
                        first.to_uppercase().collect::<String>() + chars.as_str()
                    })
                })
                .collect();
            format!("{native} {ty} \"json:\\\"{name}\\\"\"")
        })
        .collect();
    format!("struct {{ {} }}", members.join("; "))
}

/// Go's `encoding/json` field match: exact, then case-insensitive (with the
/// Kelvin sign and long s folding to ASCII).
fn fold(key: &str) -> String {
    key.chars()
        .map(|ch| match ch {
            '\u{212a}' => 'k',
            '\u{017f}' => 's',
            _ => ch.to_ascii_lowercase(),
        })
        .collect()
}

fn string_of(raw: &str) -> Option<String> {
    serde_json::from_str::<String>(raw).ok()
}

fn decode(specs: &[(&str, &str)], raw: Option<&RawValue>) -> Result<Map<String, Value>, ToolError> {
    let raw = raw
        .ok_or_else(|| ToolError::validation("parse arguments", "unexpected end of JSON input"))?;
    let text = symbrain_mcp::raw_skills_repaired(raw);
    let fields = serde_json::from_str::<Fields>(&text).map_err(|_| {
        ToolError::validation(
            "parse arguments",
            format!(
                "json: cannot unmarshal {} into Go value of type {}",
                kind(raw.get()),
                descriptor(specs)
            ),
        )
    })?;
    let mut output = Map::new();
    let mut first: Option<String> = None;
    let mut slots = Vec::<String>::new();
    for (key, member) in fields.0 {
        let key = fold(&key);
        let Some((key, ty)) = specs.iter().find(|(name, _)| *name == key) else {
            continue;
        };
        let member = member.get();
        if member == "null" {
            // null leaves strings/bools/ints alone, resets a slice to nil and
            // clears a *bool.
            if *ty == "*bool" {
                output.remove(*key);
            }
            if *ty == "[]string" {
                slots.clear();
                output.insert((*key).into(), Value::Null);
            }
            continue;
        }
        let value = match *ty {
            "string" => string_of(member).map(Value::String),
            "bool" | "*bool" => serde_json::from_str::<bool>(member).ok().map(Value::Bool),
            "int" => serde_json::from_str::<i64>(member)
                .ok()
                .map(|number| json!(number)),
            "[]string" => serde_json::from_str::<Vec<Box<RawValue>>>(member)
                .ok()
                .map(|values| {
                    if values.is_empty() {
                        slots.clear();
                    }
                    for (index, item) in values.iter().enumerate() {
                        if index == slots.len() {
                            slots.push(String::new());
                        }
                        if item.get() == "null" {
                            continue;
                        }
                        match string_of(item.get()) {
                            Some(value) => slots[index] = value,
                            None if first.is_none() => {
                                first = Some(format!(
                                    "json: cannot unmarshal {} into Go struct field .{key} of type string",
                                    kind(item.get())
                                ));
                            }
                            None => {}
                        }
                    }
                    json!(slots[..values.len()])
                }),
            _ => None,
        };
        if let Some(value) = value {
            output.insert((*key).into(), value);
        } else if first.is_none() {
            let from = if *ty == "int" && kind(member) == "number" {
                format!("number {member}")
            } else {
                kind(member).into()
            };
            first = Some(format!(
                "json: cannot unmarshal {from} into Go struct field .{key} of type {}",
                ty.trim_start_matches('*')
            ));
        }
    }
    match first {
        Some(error) => Err(ToolError::validation("parse arguments", error)),
        None => Ok(output),
    }
}

fn text(args: &Map<String, Value>, key: &str) -> String {
    args.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}
fn flag(args: &Map<String, Value>, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

// ----------------------------------------------------------------------
// Profiles.
// ----------------------------------------------------------------------

// Result bodies are structs, not maps, so field order is Go's sorted map-key
// order and nested records keep their declaration order.
#[derive(Serialize)]
struct ProfilesOut<'a> {
    profiles: Option<&'a [context_profile::ProfileRef]>,
}
#[derive(Serialize)]
struct ResolvedOut<'a> {
    issues: Option<&'a [symbrain_skills::Issue]>,
    skills: Option<&'a [context_profile::Resolved]>,
}
#[derive(Serialize)]
struct DiscoveredOut<'a> {
    candidates: &'a [discover::Candidate],
}
#[derive(Serialize)]
struct HistoryOut<'a> {
    history: &'a [vcs::Commit],
    name: &'a str,
}

/// Go marshals a nil slice as `null`; every list here is nil when empty.
fn or_null<T>(rows: &[T]) -> Option<&[T]> {
    (!rows.is_empty()).then_some(rows)
}

fn profile_list(dirs: &Dirs) -> Outcome {
    let refs = context_profile::list(&dirs.profiles, None)
        .map_err(|error| ToolError::validation("list profiles", error))?;
    compact(&ProfilesOut {
        profiles: or_null(&refs),
    })
}

fn profile_resolve(dirs: &Dirs, raw: Option<&RawValue>) -> Outcome {
    let args = decode(&[("name", "string")], raw)?;
    let (rows, issues) =
        context_profile::resolve(&dirs.library, &dirs.profiles, None, &text(&args, "name"))
            .map_err(|error| ToolError::validation("resolve profile", error))?;
    compact(&ResolvedOut {
        issues: or_null(&issues),
        skills: or_null(&rows),
    })
}

// ----------------------------------------------------------------------
// Discovery.
// ----------------------------------------------------------------------

fn discover_sources(raw: Option<&RawValue>) -> Outcome {
    let args = decode(&[("paths", "[]string"), ("scope", "string")], raw)?;
    let paths: Vec<String> = args
        .get("paths")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    // The gateway has no project directory: every scope scans the user roots.
    let rows = discover::scanned(&skills_config::home_dir(), &paths)
        .map_err(|error| ToolError::validation("discover sources", error))?;
    compact(&DiscoveredOut { candidates: &rows })
}

// ----------------------------------------------------------------------
// Versioning.
// ----------------------------------------------------------------------

fn skill_dir(dirs: &Dirs, name: &str) -> PathBuf {
    gotext::join(&dirs.library, name)
}

fn not_versioned(context: &str, name: &str, dir: &Path) -> ToolError {
    ToolError::validation(
        context,
        format!(
            "skill {} is not versioned: no git repository at {}",
            gotext::quote(name),
            dir.display()
        ),
    )
}

fn history(dirs: &Dirs, raw: Option<&RawValue>) -> Outcome {
    let args = decode(&[("name", "string"), ("limit", "int")], raw)?;
    let name = text(&args, "name");
    if name.is_empty() {
        return Err(ToolError::validation("history", "name is required"));
    }
    let limit = args.get("limit").and_then(Value::as_i64).unwrap_or(0);
    let dir = skill_dir(dirs, &name);
    if !vcs::is_repo(&dir) {
        return Err(not_versioned("history", &name, &dir));
    }
    let history =
        vcs::history(&dir, limit).map_err(|error| ToolError::internal("read history", error))?;
    compact(&HistoryOut {
        history: &history,
        name: &name,
    })
}

fn validate_restored(rev: &str, tree: &Path) -> Result<(), ToolError> {
    let bundle = symbrain_skills::load_bundle(tree).map_err(|error| {
        ToolError::validation(
            "restore skill",
            format!("refusing restore to {rev}: restored state is not a valid skill: {error}"),
        )
    })?;
    let errors: Vec<_> = symbrain_skills::validate(&bundle)
        .into_iter()
        .filter(|issue| issue.severity == "error")
        .map(|issue| issue.message.clone())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ToolError::validation(
            "restore skill",
            format!(
                "refusing restore to {rev}: restored state fails validation: {}",
                errors.join("; ")
            ),
        ))
    }
}

fn restore(dirs: &Dirs, raw: Option<&RawValue>) -> Outcome {
    let args = decode(
        &[
            ("name", "string"),
            ("rev", "string"),
            ("dry_run", "*bool"),
            ("allow_dirty", "bool"),
            ("sync", "bool"),
        ],
        raw,
    )?;
    let (name, rev) = (text(&args, "name"), text(&args, "rev"));
    if name.is_empty() || rev.is_empty() {
        return Err(ToolError::validation(
            "restore skill",
            "name and rev are required",
        ));
    }
    let dry_run = flag(&args, "dry_run", true);
    let dir = skill_dir(dirs, &name);
    if !vcs::is_repo(&dir) {
        return Err(not_versioned("restore skill", &name, &dir));
    }
    // `vcs.enabled` cannot be switched off here: the gateway registers the
    // tools with default options, so Go's disabled-versioning branch is
    // unreachable from this surface.
    let resolved = vcs::resolve(&dir, &rev).map_err(|error| {
        ToolError::validation(
            "restore skill",
            format!("revision {} not found: {error}", gotext::quote(&rev)),
        )
    })?;
    let dirty =
        vcs::dirty(&dir).map_err(|error| ToolError::internal("check working tree", error))?;
    if dirty && !flag(&args, "allow_dirty", false) {
        return Err(ToolError::new(
            "conflict",
            "restore skill",
            format!(
                "skill {} has uncommitted changes; they are never discarded — pass allow_dirty=true to snapshot them into a pre-restore commit first",
                gotext::quote(&name)
            ),
        ));
    }
    // Materialize and validate the target revision before any write. The
    // tree lands in <tmp>/<name> so the frontmatter name is checked against
    // the library directory name by the normal validation rules.
    let tmp = tempfile::Builder::new()
        .prefix("symskills-restore-")
        .tempdir()
        .map_err(|error| ToolError::internal("restore skill", error))?;
    let tree = gotext::join(tmp.path(), &name);
    vcs::extract(&dir, &resolved, &tree)
        .map_err(|error| ToolError::internal("extract revision", error))?;
    validate_restored(&resolved, &tree)?;
    let changed = vcs::changed(&dir, &resolved)
        .map_err(|error| ToolError::internal("compare revision", error))?;
    let mut notes = vec![
        "Restore creates a new forward commit; it never resets or rewrites history.",
        "Run symskills sync to reinstall targets that become stale after the restore.",
    ];
    if dirty {
        notes.push("Uncommitted changes would be committed as a pre-restore snapshot.");
    }
    let mut result = BTreeMap::from([
        ("name", json!(name)),
        ("rev", json!(resolved)),
        ("dry_run", json!(dry_run)),
        ("action", json!("planned")),
        ("changed_files", json!(changed)),
        ("notes", json!(notes)),
    ]);
    if dry_run {
        return compact(&result);
    }
    if dirty {
        vcs::commit(
            &dir,
            &format!("restore: snapshot uncommitted changes before restore to {resolved}"),
        )
        .map_err(|error| ToolError::internal("snapshot uncommitted changes", error))?;
    }
    let head = vcs::restore(&dir, &tree, &format!("restore: skill {name} to {resolved}"))
        .map_err(|error| ToolError::internal("restore skill", error))?;
    result.insert("action", json!("restored"));
    result.insert("head", json!(head));
    if flag(&args, "sync", false) {
        result.insert("synced", resync(dirs, &name)?);
    }
    compact(&result)
}

/// Re-installs this skill's stale targets, mirroring `symskills sync`.
fn resync(dirs: &Dirs, name: &str) -> Result<Value, ToolError> {
    let home = skills_config::home_dir();
    let statuses = install::status(&install::StatusOptions {
        home_dir: home.clone(),
        scope: "user".into(),
        library_dir: dirs.library.clone(),
        base_dir: Some(dirs.base.clone()),
        ..Default::default()
    })
    .map_err(|error| ToolError::internal("post-restore status", error))?;
    let stale: Vec<_> = statuses
        .into_iter()
        .filter(|row| {
            row.name == name
                && matches!(
                    row.status,
                    install::StatusKind::Stale | install::StatusKind::HarnessChanged
                )
        })
        .collect();
    let results = install::sync_selected(
        &install::SyncOptions {
            library_dir: dirs.library.clone(),
            render_dir: Some(dirs.render.clone()),
            base_dir: Some(dirs.base.clone()),
            scope: "user".into(),
            conflict_policy: install::ConflictPolicy::Abort,
            home_dir: home,
            ..Default::default()
        },
        &stale,
    )
    .map_err(|error| ToolError::internal("post-restore status", error))?;
    let rows: Vec<BTreeMap<&str, String>> = results
        .into_iter()
        .map(|row| {
            let mut out = BTreeMap::from([("target", row.target), ("name", row.name)]);
            out.insert("action", row.action);
            if !row.error.is_empty() {
                out.insert("error", row.error);
            }
            let path = row.path.to_string_lossy().into_owned();
            if !path.is_empty() {
                out.insert("path", path);
            }
            out
        })
        .collect();
    Ok(json!(rows))
}
