//! Explicit memory aging (#491, ported for #760 from `internal/memory/aging`
//! at v0.12.0).
//!
//! Every non-retired memory gets a decay factor derived from its last access,
//! access count and creation time; the factor is stored and multiplied into
//! the retrieval score. Memories whose factor drops below a floor are
//! *flagged* retired (`retired_at`), never deleted. The pass takes an injected
//! clock so results are deterministic.

use crate::gotime;
use crate::model::{Store, StoreError};
use chrono::{DateTime, Datelike, Utc};
use rusqlite::params;
use serde::Serialize;

/// The aging policy; mirrors the persisted `aging.*` configuration section.
#[derive(Debug, Clone, PartialEq)]
pub struct AgingConfig {
    /// Master switch; when false the pass is a no-op.
    pub enabled: bool,
    /// Days after which an unaccessed memory halves its decay.
    pub access_half_life_days: f64,
    /// A decay below this floor retires the memory (flagged, not deleted).
    pub retire_below: f64,
    /// Access count at which the access boost saturates.
    pub access_boost_cap: i64,
}

impl Default for AgingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            access_half_life_days: 120.0,
            retire_below: 0.1,
            access_boost_cap: 20,
        }
    }
}

/// `aging.DecayFactor`: `clamp(recency * (1 - 0.5*boost) + 0.5*boost, 0.02, 1)`.
///
/// `recency` is exponential decay over the days since the effective last
/// access (the real `last_access`, else `created_at`); `boost` is the
/// log-scaled access count and requires a recorded `last_access`.
#[must_use]
pub fn decay_factor(
    config: &AgingConfig,
    created_at: DateTime<Utc>,
    last_access: Option<DateTime<Utc>>,
    access_count: i64,
    now: DateTime<Utc>,
) -> f64 {
    if !config.enabled {
        return 1.0;
    }
    let half_life = if config.access_half_life_days > 0.0 {
        config.access_half_life_days
    } else {
        120.0
    };
    let cap = if config.access_boost_cap > 0 {
        config.access_boost_cap
    } else {
        20
    };
    let effective = last_access.unwrap_or(created_at);
    #[allow(clippy::cast_precision_loss)] // Millisecond counts stay far below 2^52.
    let days = ((now - effective).num_milliseconds() as f64 / 86_400_000.0).max(0.0);
    let recency = (-0.693 * days / half_life).exp();
    #[allow(clippy::cast_precision_loss)] // Access counts are small.
    let boost = if last_access.is_some() {
        ((access_count as f64).ln_1p() / (cap as f64).ln_1p()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (recency * (1.0 - 0.5 * boost) + 0.5 * boost).clamp(0.02, 1.0)
}

/// Summary of one aging pass (`aging.Report`).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AgingReport {
    /// Non-retired memories considered.
    pub scanned: usize,
    /// Memories whose factor changed.
    pub decayed: usize,
    /// Memories flagged retired this pass.
    pub retired: usize,
    /// True when nothing was written.
    pub dry_run: bool,
    /// Wall time of the pass in milliseconds.
    #[serde(rename = "elapsed_ms")]
    pub elapsed: f64,
    /// Ids flagged this pass.
    pub retired_ids: Vec<String>,
}

impl std::fmt::Display for AgingReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let verb = if self.dry_run {
            "would retire"
        } else {
            "retired"
        };
        write!(
            formatter,
            "aging pass: scanned {}, decayed {}, {verb} {} ({:.0} ms)",
            self.scanned, self.decayed, self.retired, self.elapsed
        )?;
        if self.dry_run {
            write!(formatter, " [dry-run, no writes]")?;
        }
        if !self.retired_ids.is_empty() {
            write!(formatter, "; retired: [{}]", self.retired_ids.join(" "))?;
        }
        Ok(())
    }
}

struct Candidate {
    id: String,
    created_at: DateTime<Utc>,
    last_access: Option<DateTime<Utc>>,
    access_count: i64,
    decay_factor: f64,
}

fn time_of(raw: &str, column: &str, id: &str) -> Result<DateTime<Utc>, StoreError> {
    gotime::parse(raw)
        .ok_or_else(|| StoreError::Invalid(format!("memory {id}: unreadable {column} {raw:?}")))
}

impl Store {
    /// `aging.Run`: scores every non-retired memory at `now`, stores changed
    /// factors and flags memories below the floor as retired. With `dry_run`
    /// nothing is written but the report counts what would change.
    ///
    /// # Errors
    /// A SQLite error or an unreadable stored timestamp.
    pub fn run_aging(
        &self,
        config: &AgingConfig,
        now: DateTime<Utc>,
        dry_run: bool,
    ) -> Result<AgingReport, StoreError> {
        let started = std::time::Instant::now();
        let mut report = AgingReport {
            dry_run,
            ..AgingReport::default()
        };
        if !config.enabled {
            return Ok(report);
        }
        let conn = self.lock()?;
        let candidates = {
            let mut statement = conn.prepare(
                "SELECT id, created_at, last_access, access_count, decay_factor FROM memories WHERE retired_at IS NULL ORDER BY id",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, f64>(4)?,
                ))
            })?;
            let mut candidates = Vec::new();
            for row in rows {
                let (id, created, accessed, access_count, decay_factor) = row?;
                candidates.push(Candidate {
                    created_at: time_of(&created, "created_at", &id)?,
                    last_access: accessed
                        .filter(|text| !text.is_empty())
                        .map(|text| time_of(&text, "last_access", &id))
                        .transpose()?
                        // Go's zero time means "never accessed".
                        .filter(|time| time.year() > 1),
                    id,
                    access_count,
                    decay_factor,
                });
            }
            candidates
        };
        report.scanned = candidates.len();
        let stamp = gotime::format(Utc::now());
        for candidate in candidates {
            let factor = decay_factor(
                config,
                candidate.created_at,
                candidate.last_access,
                candidate.access_count,
                now,
            );
            if factor < config.retire_below {
                report.retired += 1;
                report.retired_ids.push(candidate.id.clone());
                if !dry_run {
                    conn.execute(
                        "UPDATE memories SET retired_at = ?, updated_at = ? WHERE id = ?",
                        params![gotime::format(now), stamp, candidate.id],
                    )?;
                }
            } else if (factor - candidate.decay_factor).abs() > 1e-9 {
                report.decayed += 1;
                if !dry_run {
                    conn.execute(
                        "UPDATE memories SET decay_factor = ? WHERE id = ?",
                        params![factor, candidate.id],
                    )?;
                }
            }
        }
        report.elapsed = started.elapsed().as_secs_f64() * 1000.0;
        Ok(report)
    }

    /// `UnretireMemory`: clears the retirement flag and resets the decay
    /// factor to 1.0 so the memory is retrievable again.
    ///
    /// # Errors
    /// A SQLite error, or `memory not found: <id>`.
    pub fn unretire_memory(&self, id: &str) -> Result<(), StoreError> {
        let conn = self.lock()?;
        let changed = conn.execute(
            "UPDATE memories SET retired_at = NULL, decay_factor = 1.0, updated_at = ? WHERE id = ?",
            params![gotime::format(Utc::now()), id],
        )?;
        if changed == 0 {
            return Err(StoreError::Invalid(format!("memory not found: {id}")));
        }
        Ok(())
    }
}
