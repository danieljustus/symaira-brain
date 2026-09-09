use crate::GatewayError;
use crate::embedded::common::{pretty, string_arg, usize_value};
use chrono::{DateTime, Utc};
use serde_json::Value;
use symbrain_activity::{fence_summary, token_count, validate_search_options};
use symbrain_memory::{ActivityItem, ActivitySearch, Store};

pub(crate) fn dispatch(store: &Store, name: &str, value: &Value) -> Result<String, GatewayError> {
    match name {
        "activity_search" => search(store, value),
        "activity_get" => get(store, value),
        "activity_status" => status(store, value),
        _ => Err(GatewayError::UnknownTool(name.to_string())),
    }
}

fn search(store: &Store, value: &Value) -> Result<String, GatewayError> {
    let query = string_arg(value, "query")?;
    let from = parse_date(value, "from")?;
    let to = parse_date(value, "to")?;
    if value.get("limit").and_then(Value::as_u64).is_none()
        || value.get("max_tokens").and_then(Value::as_u64).is_none()
    {
        return Err(GatewayError::InvalidArguments(
            "limit and max_tokens are required; unbounded queries are refused".into(),
        ));
    }
    if value
        .get("source")
        .and_then(Value::as_str)
        .is_some_and(|source| source.chars().count() > symbrain_activity::MAX_QUERY_LENGTH)
    {
        return Err(GatewayError::InvalidArguments(format!(
            "source exceeds {} characters",
            symbrain_activity::MAX_QUERY_LENGTH
        )));
    }
    let options = ActivitySearch {
        query,
        source: value
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        from,
        to,
        limit: usize_value(value, "limit", 0),
        max_tokens: usize_value(value, "max_tokens", 0),
        include_episodes: value
            .get("include_episodes")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };
    validate_search_options(&symbrain_activity::SearchOptions {
        query: options.query.clone(),
        source: options.source.clone(),
        from: options.from,
        to: options.to,
        limit: options.limit,
        max_tokens: options.max_tokens,
        include_episodes: options.include_episodes,
    })
    .map_err(|error| GatewayError::InvalidArguments(error.to_string()))?;
    let mut page = store.activity_search(&options)?;
    for item in &mut page.results {
        fence(item, options.max_tokens);
    }
    page.used_tokens = page.results.iter().map(|item| item.tokens).sum();
    pretty(&page)
}

fn status(store: &Store, value: &Value) -> Result<String, GatewayError> {
    let max_tokens = required_usize(value, "max_tokens")?;
    if !(1..=symbrain_activity::MAX_TOKENS).contains(&max_tokens) {
        return Err(GatewayError::InvalidArguments(
            "max_tokens must be between 1 and 4000".into(),
        ));
    }
    pretty(&store.activity_status()?)
}

fn required_usize(value: &Value, key: &str) -> Result<usize, GatewayError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| {
            GatewayError::InvalidArguments(format!(
                "{key} is required; unbounded queries are refused"
            ))
        })
}

fn parse_date(value: &Value, key: &str) -> Result<DateTime<Utc>, GatewayError> {
    string_arg(value, key)?
        .parse::<DateTime<Utc>>()
        .map(|date| date.with_timezone(&Utc))
        .map_err(|error| GatewayError::InvalidArguments(format!("{key} must be RFC3339: {error}")))
}

fn get(store: &Store, value: &Value) -> Result<String, GatewayError> {
    let id = string_arg(value, "id")?;
    if id.chars().count() > symbrain_activity::MAX_QUERY_LENGTH {
        return Err(GatewayError::InvalidArguments(format!(
            "id exceeds {} characters",
            symbrain_activity::MAX_QUERY_LENGTH
        )));
    }
    let max_tokens = usize_value(value, "max_tokens", 0);
    if !(1..=symbrain_activity::MAX_TOKENS).contains(&max_tokens) {
        return Err(GatewayError::InvalidArguments(
            "max_tokens must be between 1 and 4000".into(),
        ));
    }
    match store.activity_get(&id)? {
        Some(mut item) => {
            fence(&mut item, max_tokens);
            pretty(&item)
        }
        None => Ok(format!("activity not found: {id}")),
    }
}

fn fence(item: &mut ActivityItem, max_tokens: usize) {
    item.summary = fence_summary(&item.summary, max_tokens);
    item.tokens = token_count(&item.summary);
}
