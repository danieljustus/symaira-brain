//! `memory sync` full-fidelity extras (#855): the entity graph and per-memory
//! links/evidence that ride along with memories in plain and relay mode.

use super::sync_run::{Failure, Outcome};
use symbrain_memory::sync_client::{ClientMemory, GoTime, parse_rfc3339};
use symbrain_memory::{EntityRecord, GraphDelta, RelationRecord, Store};

/// Relay blob id prefixes of graph rows.
pub(super) const ENTITY_PREFIX: &str = "entity:";
pub(super) const RELATION_PREFIX: &str = "relation:";

/// The graph rows changed since `since`, to be pushed once per run.
pub(super) fn pending(store: &Store, since: &GoTime) -> Outcome<GraphDelta> {
    store
        .client_graph_since(since)
        .map_err(|error| Failure::Message(format!("read local entity graph: {error}")))
}

/// Merges a received graph envelope before the memories that reference it.
pub(super) fn apply(store: &Store, graph: &GraphDelta) -> Outcome<()> {
    store
        .client_apply_graph(graph)
        .map(|_| ())
        .map_err(|error| Failure::Message(format!("apply remote entity graph: {error}")))
}

/// Links a received memory to its entities and stores its evidence.
pub(super) fn apply_extras(store: &Store, memory: &ClientMemory) -> Outcome<()> {
    store
        .client_apply_extras(&memory.id, &memory.extras)
        .map(|_| ())
        .map_err(|error| Failure::Message(format!("apply remote links for {}: {error}", memory.id)))
}

fn stamp(raw: &str) -> GoTime {
    parse_rfc3339(raw).unwrap_or_else(|_| GoTime::zero())
}

/// Relay blobs for a graph delta: `(blob id, updated_at, plaintext payload)`.
pub(super) fn relay_payloads(graph: &GraphDelta) -> Vec<(String, GoTime, String)> {
    let mut payloads = Vec::new();
    for entity in &graph.entities {
        let payload = serde_json::json!({ "entity": entity }).to_string();
        payloads.push((
            format!("{ENTITY_PREFIX}{}", entity.id),
            stamp(&entity.updated_at),
            payload,
        ));
    }
    for relation in &graph.relations {
        let payload = serde_json::json!({ "relation": relation }).to_string();
        let at = relation
            .updated_at
            .as_deref()
            .unwrap_or(&relation.created_at);
        payloads.push((
            format!("{RELATION_PREFIX}{}", relation.id),
            stamp(at),
            payload,
        ));
    }
    payloads
}

/// Applies one relay entity or relation blob.
pub(super) fn apply_relay_row(
    store: &Store,
    entity: Option<&EntityRecord>,
    relation: Option<&RelationRecord>,
) -> Outcome<bool> {
    let delta = GraphDelta {
        entities: entity.cloned().into_iter().collect(),
        relations: relation.cloned().into_iter().collect(),
    };
    if delta.is_empty() {
        return Ok(false);
    }
    apply(store, &delta)?;
    Ok(true)
}
