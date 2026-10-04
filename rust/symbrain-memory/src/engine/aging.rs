//! Explicit injected-time aging curve; retirement/store passes are separate.

use chrono::{DateTime, Datelike, Timelike, Utc};

/// Frozen Go aging inputs, including its fallback rules for nonpositive values.
#[derive(Debug, Clone, Copy)]
pub struct AgingConfig {
    pub enabled: bool,
    pub access_half_life_days: f64,
    pub retire_below: f64,
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

/// Computes a decay multiplier without opening a database or reading a clock.
///
/// Invalid floating-point values retain Go's IEEE behavior; callers must not
/// substitute a finite factor or silently retire a row for a NaN result.
#[must_use]
pub fn decay_factor(
    config: AgingConfig,
    created_at: DateTime<Utc>,
    last_access: Option<DateTime<Utc>>,
    access_count: i64,
    now: DateTime<Utc>,
) -> f64 {
    if !config.enabled {
        return 1.0;
    }
    let half_life = if config.access_half_life_days <= 0.0 {
        120.0
    } else {
        config.access_half_life_days
    };
    let cap = if config.access_boost_cap <= 0 {
        20
    } else {
        config.access_boost_cap
    };
    let last_access = last_access.filter(|time| !is_go_zero(*time));
    let effective = last_access.unwrap_or(created_at);
    let elapsed = now.signed_duration_since(effective);
    // time.Time.Sub saturates to the Go int64 duration bounds.
    let nanos =
        elapsed
            .num_nanoseconds()
            .unwrap_or(if now >= effective { i64::MAX } else { i64::MIN });
    #[allow(clippy::cast_precision_loss)]
    let mut days = (nanos / 3_600_000_000_000) as f64
        + (nanos % 3_600_000_000_000) as f64 / 3_600_000_000_000.0;
    days /= 24.0;
    if days < 0.0 {
        days = 0.0;
    }
    let recency = (-0.693 * days / half_life).exp();
    let mut boost = 0.0;
    if last_access.is_some() {
        #[allow(clippy::cast_precision_loss)]
        {
            boost = (access_count as f64).ln_1p() / (cap as f64).ln_1p();
        }
        boost = boost.clamp(0.0, 1.0);
    }
    let decay = recency * (1.0 - 0.5 * boost) + 0.5 * boost;
    // Fixed finite bounds preserve a NaN input, just like Go's comparisons.
    decay.clamp(0.02, 1.0)
}

fn is_go_zero(time: DateTime<Utc>) -> bool {
    time.year() == 1
        && time.ordinal() == 1
        && time.num_seconds_from_midnight() == 0
        && time.nanosecond() == 0
}
