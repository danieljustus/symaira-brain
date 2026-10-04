//! Typed persisted rotation records: retain declaration/duplicate field order.
use std::fmt;

use chrono::DateTime;
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor},
};
use serde_json::value::RawValue;
use symbrain_core::GoText;

use crate::startup_fallback::Entry;

pub(super) struct Records {
    pub entries: Vec<Entry>,
    pub nil: bool,
}

fn field(actual: &str, expected: &str) -> bool {
    actual
        .chars()
        .map(|c| match c {
            '\u{17f}' => 's',
            '\u{212a}' => 'k',
            _ => c.to_ascii_lowercase(),
        })
        .eq(expected.chars())
}

impl<'de> Deserialize<'de> for Entry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Entry;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("fallback entry")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Entry, E> {
                Ok(Entry {
                    secret: String::new(),
                    expires: DateTime::parse_from_rfc3339("0001-01-01T00:00:00Z")
                        .expect("Go zero time"),
                })
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Entry, M::Error> {
                let mut entry = self.visit_unit()?;
                while let Some(name) = map.next_key::<String>()? {
                    let raw = map.next_value::<Box<RawValue>>()?;
                    if raw.get() == "null" {
                        continue;
                    }
                    if field(&name, "secret") {
                        entry.secret =
                            serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
                    } else if field(&name, "expires_at") {
                        let text: String =
                            serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
                        entry.expires = DateTime::parse_from_rfc3339(&text)
                            .map_err(serde::de::Error::custom)?;
                    }
                }
                Ok(entry)
            }
        }
        deserializer.deserialize_any(Fields)
    }
}

pub(super) fn parse(bytes: &[u8]) -> Result<Records, GoText> {
    // Like Go encoding/json: malformed string bytes become one replacement per
    // byte; unknown fields are raw/skipped rather than float64-converted.
    let mut quoted = false;
    let mut escaped = false;
    let mut depth = 0usize;
    for &byte in bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else if byte == b'"' {
            quoted = true;
        } else if matches!(byte, b'[' | b'{') {
            depth += 1;
            if depth > 10_000 {
                return Err(format!(
                    "invalid character '{}' exceeded max depth",
                    char::from(byte)
                )
                .into());
            }
        } else if matches!(byte, b']' | b'}') {
            depth = depth.saturating_sub(1);
        }
    }
    let repaired = symbrain_core::go_json_compatible_text(bytes);
    let entries: Option<Vec<Entry>> =
        serde_json::from_str(&repaired).map_err(|error| GoText::from(error.to_string()))?;
    Ok(Records {
        nil: entries.is_none(),
        entries: entries.unwrap_or_default(),
    })
}

pub(super) fn render(entries: &Records) -> String {
    if entries.nil {
        return "null".to_owned();
    }
    let records: Vec<_> = entries
        .entries
        .iter()
        .map(|entry| {
            let base = entry.expires.format("%Y-%m-%dT%H:%M:%S").to_string();
            let nanos = entry.expires.timestamp_subsec_nanos();
            let fraction = if nanos == 0 {
                String::new()
            } else {
                format!(".{}", format!("{nanos:09}").trim_end_matches('0'))
            };
            let offset = if entry.expires.offset().local_minus_utc() == 0 {
                "Z".to_owned()
            } else {
                entry.expires.format("%:z").to_string()
            };
            let expiry = format!("{base}{fraction}{offset}");
            let secret = symbrain_core::go_json_string_bytes(entry.secret.as_bytes())
                .expect("valid JSON string");
            let expiry = symbrain_core::go_json_string_bytes(expiry.as_bytes())
                .expect("valid JSON timestamp");
            format!(
                "{{\"secret\":{},\"expires_at\":{}}}",
                secret.get(),
                expiry.get()
            )
        })
        .collect();
    format!("[{}]", records.join(","))
}

#[cfg(test)]
mod tests {
    #[test]
    fn preserves_duplicate_and_folded_field_order_and_nanosecond_time() {
        let rows = super::parse(br#"[{"secret":"old","Secret":"new","expires_at":"2099-01-01T00:00:00.1234Z","unknown":1e400}]"#).unwrap();
        assert_eq!(rows.entries[0].secret, "new");
        assert_eq!(
            super::render(&rows),
            "[{\"secret\":\"new\",\"expires_at\":\"2099-01-01T00:00:00.1234Z\"}]"
        );
    }

    #[test]
    fn repairs_surrogates_and_obeys_go_unknown_value_depth() {
        let rows = super::parse(
            br#"[{"secret":"\ud800\udc00\ud800","expires_at":"2099-01-01T00:00:00Z"}]"#,
        )
        .unwrap();
        assert_eq!(rows.entries[0].secret, "\u{10000}\u{fffd}");
        for (depth, accepted) in [(9_998, true), (9_999, false)] {
            let bytes = format!(
                "[{{\"unknown\":{}{}}}]",
                "[".repeat(depth),
                "]".repeat(depth)
            );
            assert_eq!(super::parse(bytes.as_bytes()).is_ok(), accepted);
        }
    }

    #[test]
    fn retains_nil_slice_distinct_from_empty_rotation_records() {
        assert_eq!(super::render(&super::parse(b"null").unwrap()), "null");
        assert_eq!(super::render(&super::parse(b"[]").unwrap()), "[]");
    }
}
