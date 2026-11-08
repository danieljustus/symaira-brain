//! `syncclient.Run`: pull, push and the encrypted relay against one remote,
//! with the shipped counters, cursor rules and error wording.

use super::sync_graph as graph;
use super::sync_net::{Client, Exchange};
use serde::Serialize;
use std::time::Instant;
use symbrain_memory::Store;
use symbrain_memory::sync_client::{
    ClientApplyMode, ClientMemory, GoTime, LocalMemory, RelayCipher, apply_body_with_graph,
    decode_api_error, decode_apply_counts, decode_changes, decode_relay_page, decode_relay_payload,
    decode_relay_stored, go_quote, relay_body, relay_payload_json,
};

const PAGE_LIMIT: usize = 500;
const PUSH_BATCH: i64 = 200;
const TOMBSTONE_PREFIX: &str = "tombstone:";

pub(super) struct Options<'a> {
    pub remote: &'a str,
    pub token: &'a str,
    pub pull: bool,
    pub push: bool,
    pub relay: bool,
    pub passphrase: &'a [u8],
    pub deadline: Instant,
    pub mode: ClientApplyMode,
}

#[derive(Serialize)]
pub(super) struct Report {
    pub remote: String,
    pub mode: &'static str,
    pub encrypted_relay: bool,
    pub cursor: String,
    pub server_time: String,
    pub pulled_memories_applied: i64,
    pub pulled_deletes_applied: i64,
    pub pushed_memories: i64,
    pub pushed_deletes: i64,
    pub relay_blobs_fetched: i64,
    pub relay_blobs_stored: i64,
    #[serde(skip)]
    pub cursor_time: GoTime,
    #[serde(skip)]
    pub server_clock: GoTime,
}

/// How a run ended without a report.
pub(super) enum Failure {
    /// The text after `symbrain memory sync: `.
    Message(String),
    /// A Go runtime panic (a `null` memory in a reply).
    NilPanic,
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

pub(super) type Outcome<T> = Result<T, Failure>;

/// `url.Values.Encode` of one value.
fn query_escape(value: &str) -> String {
    use std::fmt::Write as _;
    let mut escaped = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                escaped.push(char::from(byte));
            }
            b' ' => escaped.push('+'),
            other => {
                let _ = write!(escaped, "%{other:02X}");
            }
        }
    }
    escaped
}

/// `HTTPError.Error` and the JSON `{error, code}` body behind it.
fn http_error(exchange: &Exchange) -> String {
    let limit = exchange.body.len().min(1 << 16);
    let (message, code) = decode_api_error(&exchange.body[..limit]);
    if code.is_empty() {
        format!("remote returned status {}: {message}", exchange.status)
    } else {
        format!("remote returned {code} ({}): {message}", exchange.status)
    }
}

fn checked(exchange: Exchange) -> Result<Exchange, String> {
    if exchange.status < 200 || exchange.status > 300 {
        return Err(http_error(&exchange));
    }
    Ok(exchange)
}

fn decode_text(error: &str) -> String {
    format!("decode remote response: {error}")
}

fn max_time(first: GoTime, second: GoTime) -> GoTime {
    GoTime::max_of(first, second)
}

fn memory_max(first: &GoTime, second: &GoTime) -> GoTime {
    max_time(first.clone(), second.clone())
}

pub(super) fn run(store: &Store, options: &Options<'_>) -> Outcome<Report> {
    let cursor = store
        .client_sync_cursor(options.remote)
        .map_err(|error| format!("read sync cursor for {}: {error}", options.remote))?;
    let mode = match (options.pull, options.push) {
        (true, true) => "both",
        (true, false) => "pull",
        _ => "push",
    };
    let client = Client::new(options.remote, options.token, options.deadline);
    let mut report = Report {
        remote: options.remote.to_owned(),
        mode,
        encrypted_relay: options.relay,
        cursor: String::new(),
        server_time: String::new(),
        pulled_memories_applied: 0,
        pulled_deletes_applied: 0,
        pushed_memories: 0,
        pushed_deletes: 0,
        relay_blobs_fetched: 0,
        relay_blobs_stored: 0,
        cursor_time: GoTime::zero(),
        server_clock: GoTime::zero(),
    };
    let mut server_time = GoTime::zero();
    let mut local_max = GoTime::zero();
    let wrap = |prefix: String| {
        move |error: Failure| match error {
            Failure::Message(message) => Failure::Message(format!("{prefix}: {message}")),
            other @ Failure::NilPanic => other,
        }
    };
    if options.relay {
        server_time = relay_pull(store, &client, options, &cursor, &mut report)
            .map_err(wrap(format!("relay pull from {}", options.remote)))?;
        if options.push {
            local_max = relay_push(store, &client, options, &cursor, &mut report)
                .map_err(wrap(format!("relay push to {}", options.remote)))?;
        }
    } else {
        if options.pull {
            server_time = plain_pull(store, &client, options, &cursor, &mut report)
                .map_err(wrap(format!("pull from {}", options.remote)))?;
        }
        if options.push {
            local_max = plain_push(store, &client, options, &cursor, &mut report)
                .map_err(wrap(format!("push to {}", options.remote)))?;
        }
    }
    let new_cursor = max_time(server_time.clone(), local_max);
    store
        .client_set_sync_cursor(options.remote, &new_cursor)
        .map_err(|error| format!("persist sync cursor for {}: {error}", options.remote))?;
    report.server_clock = server_time;
    report.cursor_time = new_cursor;
    report.cursor = report.cursor_time.to_rfc3339_nano();
    report.server_time = report.server_clock.to_rfc3339_nano();
    Ok(report)
}

fn changes_target(base: &str, since: &GoTime, cursor: &str) -> String {
    let mut query = Vec::new();
    if !cursor.is_empty() {
        query.push(format!("cursor={}", query_escape(cursor)));
    }
    // Full-fidelity replica (#855): ask for stored vectors and the entity
    // graph; a Go-era server ignores both and answers with the lite form.
    query.push("include_embeddings=true".to_owned());
    query.push("include_graph=true".to_owned());
    query.push(format!("limit={PAGE_LIMIT}"));
    if cursor.is_empty() && !since.is_zero() {
        query.push(format!("since={}", query_escape(&since.to_rfc3339_utc())));
    }
    format!("{base}/api/sync/changes?{}", query.join("&"))
}

fn plain_pull(
    store: &Store,
    client: &Client,
    options: &Options<'_>,
    since: &GoTime,
    report: &mut Report,
) -> Outcome<GoTime> {
    let mut max_seen = GoTime::zero();
    let mut since = since.clone();
    let mut cursor = String::new();
    loop {
        let target = changes_target(&client.base, &since, &cursor);
        let exchange = checked(client.get(&target)?)?;
        let changes = decode_changes(&exchange.body, exchange.stop.as_deref())
            .map_err(|error| decode_text(&error))?;
        graph::apply(store, &changes.graph)?;
        for memory in &changes.memories {
            let Some(memory) = memory else {
                return Err(Failure::NilPanic);
            };
            let applied = store
                .client_upsert_if_newer(memory, options.mode)
                .map_err(|error| format!("apply remote memory {}: {error}", memory.id))?;
            if applied {
                report.pulled_memories_applied += 1;
            }
            graph::apply_extras(store, memory)?;
            max_seen = memory_max(&max_seen, &memory.updated_at);
        }
        for deleted in &changes.deleted {
            let removed = store
                .client_apply_remote_delete(
                    &deleted.id,
                    &deleted.deleted_at,
                    options.mode.audit_enabled,
                )
                .map_err(|error| format!("apply remote delete {}: {error}", deleted.id))?;
            if removed {
                report.pulled_deletes_applied += 1;
            }
            max_seen = memory_max(&max_seen, &deleted.deleted_at);
        }
        max_seen = max_time(max_seen, changes.server_time.clone());
        if changes.next_cursor.is_empty() {
            return Ok(max_seen);
        }
        cursor = changes.next_cursor;
        since = GoTime::zero();
    }
}

/// One push batch cursor, as the shipped loops keep it.
struct Position {
    memories_since: GoTime,
    deletes_since: GoTime,
    memory_id: String,
    delete_id: String,
}

struct Batch {
    memories: Vec<LocalMemory>,
    deleted: Vec<(String, GoTime)>,
}

fn next_batch(store: &Store, position: &Position) -> Outcome<Option<Batch>> {
    let memories = store
        .client_local_memories(&position.memories_since, &position.memory_id, PUSH_BATCH)
        .map_err(|error| format!("read local changes: {error}"))?;
    let deleted = store
        .client_local_deletes(&position.deletes_since, &position.delete_id, PUSH_BATCH)
        .map_err(|error| format!("read local tombstones: {error}"))?;
    if memories.is_empty() && deleted.is_empty() {
        return Ok(None);
    }
    Ok(Some(Batch { memories, deleted }))
}

fn advance(position: &mut Position, batch: &Batch, local_max: &mut GoTime) -> bool {
    let mut batch_max = GoTime::zero();
    for memory in &batch.memories {
        batch_max = memory_max(&batch_max, &memory.updated_at);
    }
    for (_, at) in &batch.deleted {
        batch_max = memory_max(&batch_max, at);
    }
    if batch_max.after(local_max) {
        *local_max = batch_max;
    }
    if let Some(last) = batch.memories.last() {
        position.memories_since = last.updated_at.clone();
        position.memory_id.clone_from(&last.id);
    }
    if let Some((id, at)) = batch.deleted.last() {
        position.deletes_since = at.clone();
        position.delete_id.clone_from(id);
    }
    let full = PUSH_BATCH;
    let (memories, deleted) = (
        i64::try_from(batch.memories.len()).unwrap_or(0),
        i64::try_from(batch.deleted.len()).unwrap_or(0),
    );
    !(memories < full && deleted < full)
}

fn start(since: &GoTime) -> Position {
    Position {
        memories_since: since.clone(),
        deletes_since: since.clone(),
        memory_id: String::new(),
        delete_id: String::new(),
    }
}

fn plain_push(
    store: &Store,
    client: &Client,
    _options: &Options<'_>,
    since: &GoTime,
    report: &mut Report,
) -> Outcome<GoTime> {
    let mut local_max = GoTime::zero();
    let mut position = start(since);
    let mut pending = graph::pending(store, since)?;
    loop {
        let batch = match next_batch(store, &position)? {
            Some(batch) => batch,
            None if !pending.is_empty() => Batch {
                memories: Vec::new(),
                deleted: Vec::new(),
            },
            None => return Ok(local_max),
        };
        let envelope = std::mem::take(&mut pending);
        let body = apply_body_with_graph(&batch.memories, &batch.deleted, &envelope);
        let exchange = checked(client.post(
            &format!("{}/api/sync/apply", client.base),
            body.into_bytes(),
        )?)?;
        let counts = decode_apply_counts(&exchange.body, exchange.stop.as_deref())
            .map_err(|error| decode_text(&error))?;
        report.pushed_memories += counts.applied;
        report.pushed_deletes += counts.deleted;
        if !advance(&mut position, &batch, &mut local_max) {
            return Ok(local_max);
        }
    }
}

fn relay_target(base: &str, since: &GoTime) -> String {
    let mut query = vec![format!("limit={PAGE_LIMIT}")];
    if !since.is_zero() {
        query.push(format!(
            "since={}",
            query_escape(&since.to_rfc3339_nano_utc())
        ));
    }
    format!("{base}/api/sync/relay?{}", query.join("&"))
}

fn relay_pull(
    store: &Store,
    client: &Client,
    options: &Options<'_>,
    since: &GoTime,
    report: &mut Report,
) -> Outcome<GoTime> {
    let mut cipher = RelayCipher::new();
    let mut max_seen = GoTime::zero();
    let mut page_since = since.clone();
    loop {
        let exchange = checked(client.get(&relay_target(&client.base, &page_since))?)?;
        let page = decode_relay_page(&exchange.body, exchange.stop.as_deref())
            .map_err(|error| decode_text(&error))?;
        max_seen = max_time(max_seen, page.server_time.clone());
        for blob in &page.blobs {
            let payload = cipher
                .decrypt(&blob.blob, options.passphrase)
                .map_err(|error| format!("decrypt relay blob {}: {error}", blob.id))?;
            let decoded = decode_relay_payload(&payload)
                .map_err(|error| format!("decode relay blob {}: {error}", blob.id))?;
            let recognised = if decoded.entity.is_some() || decoded.relation.is_some() {
                graph::apply_relay_row(store, decoded.entity.as_ref(), decoded.relation.as_ref())?
            } else if let Some(deleted) = &decoded.deleted {
                let removed = store
                    .client_apply_remote_delete(
                        &deleted.id,
                        &deleted.deleted_at,
                        options.mode.audit_enabled,
                    )
                    .map_err(|error| format!("apply relay delete {}: {error}", deleted.id))?;
                if removed {
                    report.pulled_deletes_applied += 1;
                }
                true
            } else if let Some(memory) = &decoded.memory {
                let applied = apply_relay_memory(store, memory, options)?;
                if applied {
                    report.pulled_memories_applied += 1;
                }
                true
            } else {
                false
            };
            if recognised {
                report.relay_blobs_fetched += 1;
            }
            max_seen = max_time(max_seen, blob.updated_at.clone());
        }
        if page.blobs.len() < PAGE_LIMIT {
            return Ok(max_seen);
        }
        if let Some(last) = page.blobs.last() {
            page_since = last.updated_at.clone();
        }
    }
}

fn apply_relay_memory(
    store: &Store,
    memory: &ClientMemory,
    options: &Options<'_>,
) -> Outcome<bool> {
    let applied = store
        .client_upsert_if_newer(memory, options.mode)
        .map_err(|error| Failure::Message(format!("apply relay memory {}: {error}", memory.id)))?;
    graph::apply_extras(store, memory)?;
    Ok(applied)
}

fn relay_push(
    store: &Store,
    client: &Client,
    options: &Options<'_>,
    since: &GoTime,
    report: &mut Report,
) -> Outcome<GoTime> {
    let mut local_max = GoTime::zero();
    let mut position = start(since);
    let mut cipher = RelayCipher::new();
    let mut pending = graph::pending(store, since)?;
    loop {
        let batch = match next_batch(store, &position)? {
            Some(batch) => batch,
            None if !pending.is_empty() => Batch {
                memories: Vec::new(),
                deleted: Vec::new(),
            },
            None => return Ok(local_max),
        };
        let mut blobs: Vec<(String, GoTime, Vec<u8>)> = Vec::new();
        for (id, at, payload) in graph::relay_payloads(&std::mem::take(&mut pending)) {
            blobs.push((id, at, encrypt(&mut cipher, &payload, options)?));
        }
        for memory in &batch.memories {
            let payload = relay_payload_json(Some(memory), None);
            let sealed = encrypt(&mut cipher, &payload, options)?;
            blobs.push((memory.id.clone(), memory.updated_at.clone(), sealed));
        }
        for (id, at) in &batch.deleted {
            let payload = relay_payload_json(None, Some((id, at)));
            let sealed = encrypt(&mut cipher, &payload, options)?;
            blobs.push((format!("{TOMBSTONE_PREFIX}{id}"), at.clone(), sealed));
        }
        let body = relay_body(&blobs);
        let exchange = checked(client.post(
            &format!("{}/api/sync/relay", client.base),
            body.into_bytes(),
        )?)?;
        let stored = decode_relay_stored(&exchange.body, exchange.stop.as_deref())
            .map_err(|error| decode_text(&error))?;
        report.relay_blobs_stored += stored;
        if !advance(&mut position, &batch, &mut local_max) {
            return Ok(local_max);
        }
    }
}

fn encrypt(cipher: &mut RelayCipher, payload: &str, options: &Options<'_>) -> Outcome<Vec<u8>> {
    cipher
        .encrypt(payload.as_bytes(), options.passphrase)
        .map_err(|error| Failure::Message(format!("encrypt relay payload: {error}")))
}

/// The `panic:` lines Go prints for a nil dereference (the addresses and
/// goroutine trace that follow are runtime specific).
pub(super) fn nil_panic_text() -> String {
    format!(
        "panic: runtime error: invalid memory address or nil pointer dereference\n[signal SIGSEGV: segmentation violation code=0x2 addr=0x0 pc=0x0]\n\ngoroutine 1 [running]:\n{}\n",
        go_quote("sync run aborted")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_targets_sort_their_keys() {
        let since = symbrain_memory::sync_client::parse_rfc3339("2026-10-26T01:02:03.5Z").unwrap();
        assert_eq!(
            changes_target("http://h", &since, ""),
            "http://h/api/sync/changes?include_embeddings=true&include_graph=true&limit=500&since=2026-10-26T01%3A02%3A03Z"
        );
        assert_eq!(
            changes_target("http://h", &since, "c 1"),
            "http://h/api/sync/changes?cursor=c+1&include_embeddings=true&include_graph=true&limit=500"
        );
        assert_eq!(
            relay_target("http://h", &since),
            "http://h/api/sync/relay?limit=500&since=2026-10-26T01%3A02%3A03.5Z"
        );
    }
}
