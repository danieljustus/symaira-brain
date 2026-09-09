use crate::GatewayError;
use crate::embedded::common::{limit, pretty, string_arg, usize_value};
use serde_json::{Value, json};
use symbrain_memory::Store;

pub(crate) fn dispatch(store: &Store, name: &str, value: &Value) -> Result<String, GatewayError> {
    match name {
        "memory_get" => memory_get(store, value),
        "memory_set" => memory_set(store, value),
        "memory_search" => memory_search(store, value),
        "memory_list" => memory_list(store, value),
        "memory_candidates" => memory_candidates(store, value),
        "memory_promote" => memory_promote(store, value),
        "memory_reject" => memory_reject(store, value),
        "entity_list" => entity_list(store),
        "entity_resolve" => entity_resolve(store, value),
        "query_log" => pretty(&store.query_log(limit(value, 20, 100))?),
        "entity_relate" => entity_relate(store, value),
        "graph_neighbors" => graph_neighbors(store, value),
        _ => Err(GatewayError::UnknownTool(name.to_string())),
    }
}

fn memory_get(store: &Store, value: &Value) -> Result<String, GatewayError> {
    let id = string_arg(value, "id")?;
    match store.get(&id)? {
        Some(memory) => pretty(&memory),
        None => Ok(format!("memory not found: {id}")),
    }
}

fn memory_set(store: &Store, value: &Value) -> Result<String, GatewayError> {
    let memory_text = string_arg(value, "content")?;
    if memory_text.chars().count() > 65_536 {
        return Err(GatewayError::InvalidArguments(
            "memory content exceeds 65536 characters".into(),
        ));
    }
    let kind = canonical_kind(&string_arg(value, "kind")?)?;
    let scope = value
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("global");
    let metadata = match value.get("metadata") {
        None => serde_json::Map::new(),
        Some(Value::Object(metadata)) => metadata.clone(),
        Some(Value::String(text)) => serde_json::from_str(text).map_err(|error| {
            GatewayError::InvalidArguments(format!(
                "'metadata' must be a valid JSON object: {error}"
            ))
        })?,
        Some(_) => {
            return Err(GatewayError::InvalidArguments(
                "'metadata' must be a valid JSON object".into(),
            ));
        }
    };
    let staged = value
        .get("staged")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let session_id = value
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let entities = value
        .get("entities")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    let working = value
        .get("working")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let memory = store.set_with_options(
        &memory_text,
        scope,
        &kind,
        metadata,
        &symbrain_memory::SetOptions {
            session_id,
            entities,
            working,
            staged,
        },
    )?;
    Ok(if staged {
        format!(
            "Memory staged as candidate (not yet retrievable) with ID: {}",
            memory.id
        )
    } else {
        format!("Memory saved successfully with ID: {}", memory.id)
    })
}

fn canonical_kind(value: &str) -> Result<String, GatewayError> {
    let normalized = value.trim().to_ascii_lowercase();
    let kind = match normalized.as_str() {
        "user" | "preference" | "preferences" | "personal" | "identity" | "user-pref"
        | "user-prefs" | "about-user" => "user",
        "feedback" | "correction" | "corrections" | "critique" | "evaluation" | "review"
        | "praise" | "complaint" => "feedback",
        "project"
        | "project-rule"
        | "project-rules"
        | "rule"
        | "rules"
        | "guideline"
        | "guidelines"
        | "constraint"
        | "constraints"
        | "architectural-decision"
        | "adr"
        | "decision"
        | "decisions"
        | "architecture" => "project",
        "reference" | "fact" | "facts" | "documentation" | "doc" | "docs" | "howto" | "how-to"
        | "api" | "external" => "reference",
        _ => {
            return Err(GatewayError::InvalidArguments(format!(
                "'kind' is required and must be one of: user, feedback, project, reference (got {value:?})"
            )));
        }
    };
    Ok(kind.to_string())
}
fn memory_search(store: &Store, value: &Value) -> Result<String, GatewayError> {
    let query = string_arg(value, "query")?;
    let rows = store.search(
        &query,
        value.get("scope").and_then(Value::as_str).unwrap_or(""),
        limit(value, 5, 100),
    )?;
    if rows.is_empty() {
        return Ok("No relevant memories found.".into());
    }
    pretty(
        &rows
            .into_iter()
            .map(|(memory, score)| json!({"memory": memory, "score": score}))
            .collect::<Vec<_>>(),
    )
}

fn memory_list(store: &Store, value: &Value) -> Result<String, GatewayError> {
    let rows = store.list(
        value.get("scope").and_then(Value::as_str).unwrap_or(""),
        limit(value, 100, 1_000),
    )?;
    if rows.is_empty() {
        Ok("Memory store is empty.".into())
    } else {
        pretty(&json!({"memories": rows}))
    }
}

fn memory_candidates(store: &Store, value: &Value) -> Result<String, GatewayError> {
    let rows = store.candidates(limit(value, 100, 1_000))?;
    if rows.is_empty() {
        Ok("No staged candidates awaiting review.".into())
    } else {
        pretty(&json!({"count": rows.len(), "results": rows}))
    }
}

fn memory_promote(store: &Store, value: &Value) -> Result<String, GatewayError> {
    let id = string_arg(value, "id")?;
    Ok(if store.promote(&id)? {
        format!("Memory {id} promoted: it is now retrievable.")
    } else {
        format!("memory not found: {id}")
    })
}

fn memory_reject(store: &Store, value: &Value) -> Result<String, GatewayError> {
    let id = string_arg(value, "id")?;
    Ok(if store.reject(&id)? {
        format!("Memory {id} rejected and removed.")
    } else {
        format!("memory not found: {id}")
    })
}

fn entity_list(store: &Store) -> Result<String, GatewayError> {
    let rows = store.entities()?;
    if rows.is_empty() {
        Ok("No entities found.".into())
    } else {
        pretty(&rows)
    }
}

fn entity_resolve(store: &Store, value: &Value) -> Result<String, GatewayError> {
    let query = string_arg(value, "query")?.to_lowercase();
    let rows = store
        .entities()?
        .into_iter()
        .filter(|entity| {
            entity
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.to_lowercase().contains(&query))
        })
        .take(limit(value, 10, 100))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        Ok("No matching entities found.".into())
    } else {
        pretty(&rows)
    }
}

fn entity_relate(store: &Store, value: &Value) -> Result<String, GatewayError> {
    let from = string_arg(value, "from")?;
    let to = string_arg(value, "to")?;
    let relation = string_arg(value, "relation")?;
    pretty(
        &store.relate(
            &from,
            &to,
            &relation,
            value
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("create"),
        )?,
    )
}

fn graph_neighbors(store: &Store, value: &Value) -> Result<String, GatewayError> {
    let entity = string_arg(value, "entity")?;
    pretty(&store.graph_neighbors(&entity, usize_value(value, "depth", 1))?)
}
