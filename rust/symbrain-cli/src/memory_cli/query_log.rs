//! Native memory query log contracts.

use super::{
    BTreeMap, DateTime, FixedOffset, NaiveDateTime, OsString, OutputFormat, QUERY_LOG_USAGE,
    SecondsFormat, Serialize, TimeZone, Utc, Write, exit, flags, go_json, open_store,
};

pub(super) fn run_query_log(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    if flags::has_help(args) {
        let _ = write!(stdout, "{QUERY_LOG_USAGE}");
        return exit::OK;
    }
    let Ok(parsed) = flags::parse(args, "query-log", stderr) else {
        return exit::USAGE;
    };
    if let Some(argument) = parsed.positional.first() {
        let _ = writeln!(
            stderr,
            "symbrain memory query-log: unexpected argument {}",
            symbrain_core::config::format_go_quoted(argument)
        );
        return exit::USAGE;
    }
    let limit = usize::try_from(if parsed.limit <= 0 {
        50
    } else {
        parsed.limit.min(1000)
    })
    .unwrap_or(1000);
    let actor = parsed.text("actor");
    let store = match open_store(stderr, Some(&parsed.raw("db"))) {
        Ok(store) => store,
        Err(code) => return code,
    };
    let mut summary = match store.query_log_summary(limit, &actor) {
        Ok(summary) => summary,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory query-log: read query log: {err}");
            return exit::GENERIC;
        }
    };
    normalize_query_log_timestamps(&mut summary);
    render_query_log(&summary, stdout, format);
    exit::OK
}

/// Go-shaped query-log row. Field order and `omitempty` follow
/// `internal/memory/db.QueryLogEntry` so the JSON bytes match.
#[derive(Debug, Serialize)]
pub(super) struct QueryLogRow {
    id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    actor: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    scope: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    session: String,
    tool: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    query_text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    params: String,
    duration_ms: i64,
    created_at: String,
}

/// Go-shaped summary. `tool_breakdown` and `actor_breakdown` are Go maps, so
/// their keys are sorted; `pruned_count` is omitted when zero.
#[derive(Debug, Serialize)]
pub(super) struct QueryLogReport {
    total_queries: i64,
    tool_breakdown: BTreeMap<String, i64>,
    actor_breakdown: BTreeMap<String, i64>,
    recent_entries: Vec<QueryLogRow>,
    /// Go omits this field when it is zero (`omitempty`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pruned_count: Option<i64>,
}

/// Rebuilds the store's summary as the Go struct, in Go field order.
pub(super) fn go_query_log_report(summary: &serde_json::Value) -> QueryLogReport {
    let counts = |key: &str| {
        summary[key]
            .as_object()
            .map(|values| {
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.as_i64().unwrap_or_default()))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default()
    };
    let entries = summary["recent_entries"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| QueryLogRow {
                    id: row["id"].as_str().unwrap_or_default().to_owned(),
                    actor: row["actor"].as_str().unwrap_or_default().to_owned(),
                    scope: row["scope"].as_str().unwrap_or_default().to_owned(),
                    session: row["session"].as_str().unwrap_or_default().to_owned(),
                    tool: row["tool"].as_str().unwrap_or_default().to_owned(),
                    query_text: row["query_text"].as_str().unwrap_or_default().to_owned(),
                    params: row["params"].as_str().unwrap_or_default().to_owned(),
                    duration_ms: row["duration_ms"].as_i64().unwrap_or_default(),
                    created_at: row["created_at"].as_str().unwrap_or_default().to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    QueryLogReport {
        total_queries: summary["total_queries"].as_i64().unwrap_or_default(),
        tool_breakdown: counts("tool_breakdown"),
        actor_breakdown: counts("actor_breakdown"),
        recent_entries: entries,
        pruned_count: summary["pruned_count"].as_i64().filter(|count| *count != 0),
    }
}

pub(super) fn render_query_log(
    summary: &serde_json::Value,
    stdout: &mut dyn Write,
    format: OutputFormat,
) {
    match format {
        OutputFormat::Json => {
            let _ = writeln!(stdout, "{}", go_json(&go_query_log_report(summary)));
        }
        OutputFormat::Table => {
            let total = summary["total_queries"].as_i64().unwrap_or_default();
            let _ = writeln!(stdout, "Total queries: {total}\n");
            let entries = summary["recent_entries"].as_array();
            if entries.is_none_or(Vec::is_empty) {
                let _ = writeln!(stdout, "No recorded queries.");
            } else {
                let _ = writeln!(stdout, "WHEN\tTOOL\tACTOR\tMS\tQUERY");
                for entry in entries.into_iter().flatten() {
                    let _ = writeln!(
                        stdout,
                        "{}\t{}\t{}\t{}\t{}",
                        table_query_log_timestamp(entry["created_at"].as_str().unwrap_or_default()),
                        entry["tool"].as_str().unwrap_or_default(),
                        entry["actor"].as_str().unwrap_or_default(),
                        entry["duration_ms"].as_i64().unwrap_or_default(),
                        table_query_content(entry["query_text"].as_str().unwrap_or_default()),
                    );
                }
            }
        }
    }
}

pub(super) fn normalize_query_log_timestamps(summary: &mut serde_json::Value) {
    if let Some(entries) = summary["recent_entries"].as_array_mut() {
        for entry in entries {
            if let Some(raw) = entry["created_at"].as_str() {
                entry["created_at"] = normalize_query_log_timestamp(raw).into();
            }
        }
    }
}

pub(super) fn parse_query_log_timestamp(raw: &str) -> Option<DateTime<FixedOffset>> {
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(raw) {
        return Some(timestamp);
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f") {
        return FixedOffset::east_opt(0)?
            .from_local_datetime(&naive)
            .single();
    }
    let mut parts = raw.split_whitespace();
    let date = parts.next()?;
    let time = parts.next()?;
    let offset = parts.next()?;
    let datetime = format!("{date} {time}");
    let naive = NaiveDateTime::parse_from_str(&datetime, "%Y-%m-%d %H:%M:%S%.f").ok()?;
    let sign = match offset.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hours = offset.get(1..3)?.parse::<i32>().ok()?;
    let minutes = offset.get(3..5)?.parse::<i32>().ok()?;
    let offset = FixedOffset::east_opt(sign * (hours * 3_600 + minutes * 60))?;
    offset.from_local_datetime(&naive).single()
}

pub(super) fn normalize_query_log_timestamp(raw: &str) -> String {
    parse_query_log_timestamp(raw).map_or_else(
        || raw.to_string(),
        |timestamp| {
            timestamp
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::AutoSi, true)
        },
    )
}

pub(super) fn table_query_log_timestamp(raw: &str) -> String {
    parse_query_log_timestamp(raw).map_or_else(
        || raw.to_string(),
        |timestamp| {
            timestamp
                .with_timezone(&Utc)
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
        },
    )
}

pub(super) fn table_query_content(content: &str) -> String {
    content.replace(['\t', '\r', '\n'], " ")
}
