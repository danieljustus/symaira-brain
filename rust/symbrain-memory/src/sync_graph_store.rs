//! Store entry points for the entity graph and per-memory extras of a
//! full-fidelity sync replica (#855), for the client and the serving peer.

use crate::model::{Store, StoreError};
use crate::sync_client_time::GoTime;
use crate::sync_graph::{GraphDelta, MemoryExtras, extras_for, graph_since};
use crate::sync_graph_apply::{GraphApplied, apply_extras, apply_graph};
use crate::sync_state::render_cursor;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

fn text(error: &impl std::fmt::Display) -> String {
    error.to_string()
}

impl Store {
    /// Entities and relations changed after the client's cursor.
    ///
    /// # Errors
    /// The SQLite error text.
    pub fn client_graph_since(&self, since: &GoTime) -> Result<GraphDelta, String> {
        let conn = self.lock().map_err(|error| text(&error))?;
        graph_since(&conn, &since.to_go_string()).map_err(|error| text(&error))
    }

    /// Merges a received graph envelope.
    ///
    /// # Errors
    /// The SQLite error text; nothing is kept on failure.
    pub fn client_apply_graph(&self, delta: &GraphDelta) -> Result<GraphApplied, String> {
        let conn = self.lock().map_err(|error| text(&error))?;
        apply_graph(&conn, delta).map_err(|error| text(&error))
    }

    /// Links a received memory to its entities and adds its evidence.
    ///
    /// # Errors
    /// The SQLite error text; nothing is kept on failure.
    pub fn client_apply_extras(
        &self,
        memory_id: &str,
        extras: &MemoryExtras,
    ) -> Result<GraphApplied, String> {
        let conn = self.lock().map_err(|error| text(&error))?;
        apply_extras(&conn, memory_id, extras).map_err(|error| text(&error))
    }

    pub(crate) fn serve_graph_since(&self, since: DateTime<Utc>) -> Result<GraphDelta, StoreError> {
        let conn = self.lock()?;
        Ok(graph_since(&conn, &render_cursor(since))?)
    }

    pub(crate) fn serve_extras(
        &self,
        ids: &[String],
    ) -> Result<BTreeMap<String, MemoryExtras>, StoreError> {
        let conn = self.lock()?;
        Ok(extras_for(&conn, ids)?)
    }

    pub(crate) fn serve_apply_graph(&self, delta: &GraphDelta) -> Result<GraphApplied, StoreError> {
        let conn = self.lock()?;
        Ok(apply_graph(&conn, delta)?)
    }

    pub(crate) fn serve_apply_extras(
        &self,
        memory_id: &str,
        extras: &MemoryExtras,
    ) -> Result<GraphApplied, StoreError> {
        let conn = self.lock()?;
        Ok(apply_extras(&conn, memory_id, extras)?)
    }
}
