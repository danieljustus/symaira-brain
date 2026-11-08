//! The additive full-fidelity members of the sync wire (#855), read in a
//! second pass over bodies the Go-faithful decoder already accepted. Anything
//! malformed or absent yields empty extras, never an error: a Go-era peer
//! simply does not send them.

use crate::sync_graph::{EntityRecord, GraphDelta, MemoryExtras, RelationRecord};
use serde_json::Value;

/// Per-memory extras of a `/api/sync/changes` body, by position in
/// `memories`, and its `entity_graph` envelope.
pub(crate) fn changes(body: &[u8]) -> (Vec<MemoryExtras>, GraphDelta) {
    let Ok(Value::Object(root)) = serde_json::from_slice::<Value>(body) else {
        return (Vec::new(), GraphDelta::default());
    };
    let extras = root
        .get("memories")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_object()
                        .map(MemoryExtras::from_object)
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    (extras, GraphDelta::from_value(root.get("entity_graph")))
}

/// Extras of one decrypted relay payload.
#[derive(Debug, Default, Clone)]
pub(crate) struct PayloadExtras {
    pub memory: MemoryExtras,
    pub entity: Option<EntityRecord>,
    pub relation: Option<RelationRecord>,
}

/// Reads the `memory` extras and the `entity` / `relation` payload kinds.
pub(crate) fn payload(bytes: &[u8]) -> PayloadExtras {
    let Ok(Value::Object(root)) = serde_json::from_slice::<Value>(bytes) else {
        return PayloadExtras::default();
    };
    PayloadExtras {
        memory: root
            .get("memory")
            .and_then(Value::as_object)
            .map(MemoryExtras::from_object)
            .unwrap_or_default(),
        entity: root
            .get("entity")
            .and_then(|value| serde_json::from_value(value.clone()).ok()),
        relation: root
            .get("relation")
            .and_then(|value| serde_json::from_value(value.clone()).ok()),
    }
}
