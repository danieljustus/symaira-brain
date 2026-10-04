//! Sync timestamps retain the wire offset rather than erasing it at decode.

use chrono::{DateTime, FixedOffset, SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A Go `time.Time` wire instant, including its fixed offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SyncTime(pub DateTime<FixedOffset>);

impl Default for SyncTime {
    fn default() -> Self {
        Self(
            Utc.with_ymd_and_hms(1, 1, 1, 0, 0, 0)
                .single()
                .expect("Go zero time exists")
                .fixed_offset(),
        )
    }
}

impl SyncTime {
    /// Renders the RFC3339Nano form used on the sync wire.
    #[must_use]
    pub fn wire(self) -> String {
        let mut wire = self.0.to_rfc3339_opts(SecondsFormat::Secs, true);
        let nanos = self.0.timestamp_subsec_nanos();
        if nanos != 0 {
            let fraction = format!("{nanos:09}");
            wire.insert_str(19, &format!(".{}", fraction.trim_end_matches('0')));
        }
        wire
    }

    /// Renders the SQLite driver's timestamp parameter, retaining nanoseconds.
    #[must_use]
    pub fn sqlite(self) -> String {
        let base = self.0.format("%Y-%m-%d %H:%M:%S").to_string();
        let fraction = format!("{:09}", self.0.timestamp_subsec_nanos());
        let fraction = fraction.trim_end_matches('0');
        let clock = if fraction.is_empty() {
            base
        } else {
            format!("{base}.{fraction}")
        };
        let offset = self.0.format("%z").to_string();
        let name = if self.0.offset().local_minus_utc() == 0 {
            "UTC"
        } else {
            &offset
        };
        format!("{clock} {offset} {name}")
    }

    pub(crate) fn parse_sqlite(raw: &str) -> Option<Self> {
        if let Ok(time) = DateTime::parse_from_rfc3339(raw) {
            return Some(Self(time));
        }
        if let Some((body, _zone)) = raw.rsplit_once(' ') {
            if let Ok(time) = DateTime::parse_from_str(body, "%Y-%m-%d %H:%M:%S%.f %z") {
                return Some(Self(time));
            }
        }
        crate::gotime::parse(raw).map(|time| Self(time.fixed_offset()))
    }

    /// Uses whole UTC seconds for the plain changes request, as Go does.
    #[must_use]
    pub fn plain_since(self) -> String {
        self.0
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    /// Uses UTC nanoseconds for the relay changes request.
    #[must_use]
    pub fn relay_since(self) -> String {
        Self(self.0.with_timezone(&Utc).fixed_offset()).wire()
    }
}

impl Serialize for SyncTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.wire())
    }
}

impl<'de> Deserialize<'de> for SyncTime {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Option::<String>::deserialize(deserializer)?;
        // Go time.Time ignores a JSON null and keeps the previous value. This
        // canonical codec only represents a new value; duplicate-field merge
        // must be implemented in the bounded transport before CLI admission.
        raw.map_or_else(
            || Ok(Self::default()),
            |raw| {
                DateTime::parse_from_rfc3339(&raw)
                    .map(Self)
                    .map_err(serde::de::Error::custom)
            },
        )
    }
}
