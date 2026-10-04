//! Embedded skills tools use the library core, never an external skills binary.
use crate::{GatewayError, GatewayResponse};
use serde_json::{Value, json};
use symbrain_skills::{Bundle, SkillError, config, load_bundle};

#[path = "skills_args.rs"]
mod arguments;
#[path = "skills_library.rs"]
mod library;
#[path = "skills_render.rs"]
mod render;
#[path = "skills_versioning.rs"]
mod versioning;

pub(super) struct Error {
    message: String,
    code: Option<&'static str>,
}
impl From<SkillError> for Error {
    fn from(error: SkillError) -> Self {
        Self {
            message: error.to_string(),
            code: None,
        }
    }
}
impl Error {
    fn validation(context: &str, message: impl std::fmt::Display) -> Self {
        Self {
            message: format!("{context}: {message}"),
            code: Some("validation"),
        }
    }
    fn internal(context: &str, message: impl std::fmt::Display) -> Self {
        Self {
            message: format!("{context}: {message}"),
            code: Some("internal"),
        }
    }
}

pub(crate) fn response(
    id: Value,
    name: &str,
    raw: Option<&serde_json::value::RawValue>,
) -> Result<GatewayResponse, GatewayError> {
    let result = arguments::decode(name, raw).and_then(|value| dispatch(name, &value));
    let body = match result {
        Ok(text) => json!({"content":[{"type":"text","text":text}],"isError":false}),
        Err(error) => {
            let mut result =
                json!({"content":[{"type":"text","text":error.message}],"isError":true});
            if let Some(code) = error.code {
                result["_meta"] =
                    json!({"symaira.dev/tool_error":{"code":code,"message":error.message}});
            }
            result
        }
    };
    GatewayResponse::success(id, body)
}

fn dispatch(name: &str, value: &Value) -> Result<String, Error> {
    match name {
        "skills_list" => library::list(),
        "skills_inspect" => library::inspect(value),
        "skills_validate" => library::validate(value),
        "skills_profile_list" => library::profiles(),
        "skills_profile_resolve" => library::resolve_profile(value),
        "skills_targets_status" => library::targets(value),
        "skills_render_plan" => render::plan(value),
        "skills_install" => render::install(value),
        "skills_discover_sources" => versioning::discover(value),
        "skills_history" => versioning::history(value),
        "skills_restore" => versioning::restore(value),
        _ => Err(Error {
            message: format!("Unknown tool: {name}"),
            code: None,
        }),
    }
}

fn text(value: &Value, key: &str) -> Result<String, Error> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.clone()),
        _ => Err(Error::validation(
            "parse arguments",
            format!("field {key} must be a string"),
        )),
    }
}
fn boolean(value: &Value, key: &str, default: bool) -> Result<bool, Error> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err(Error::validation(
            "parse arguments",
            format!("field {key} must be a boolean"),
        )),
    }
}
fn bundle(value: &Value) -> Result<Bundle, Error> {
    let requested = text(value, "path")?;
    let name = text(value, "name")?;
    let path = if requested.is_empty() && !name.is_empty() {
        config::defaults().library_dir.join(name)
    } else {
        std::path::PathBuf::from(requested)
    };
    if path.as_os_str().is_empty() {
        return Err(Error::validation(
            "inspect skill",
            "path or name is required",
        ));
    }
    load_bundle(&path).map_err(Into::into)
}
fn compact<T: serde::Serialize>(value: &T) -> Result<String, Error> {
    serde_json::to_string(value)
        .map(|json| {
            json.replace('&', "\\u0026")
                .replace('<', "\\u003c")
                .replace('>', "\\u003e")
                .replace('\u{2028}', "\\u2028")
                .replace('\u{2029}', "\\u2029")
        })
        .map_err(|error| Error::internal("serialize result", error))
}

#[cfg(test)]
#[path = "skills_target_tests.rs"]
mod historical_target_contract_tests;
