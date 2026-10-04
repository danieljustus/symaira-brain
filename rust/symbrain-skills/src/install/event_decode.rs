//! Operation logs retain Go null, duplicate-field and byte replacement rules.
use super::event::OperationEvent;
use super::marker_string::repair_json_strings;

pub(super) fn decode(raw: &[u8]) -> Option<OperationEvent> {
    let fields = super::marker_json::fields(raw).ok()?;
    let raw = raw.trim_ascii();
    let mut event = OperationEvent::default();
    if raw == b"null" {
        return Some(event);
    }
    if raw.first() != Some(&b'{') {
        return None;
    }
    for (key, value) in fields {
        let key: String = serde_json::from_str(&repair_json_strings(key)).ok()?;
        let key: String = key
            .chars()
            .map(|ch| match ch {
                '\u{212a}' => 'k',
                '\u{017f}' => 's',
                _ => ch.to_ascii_lowercase(),
            })
            .collect();
        let field = match key.as_str() {
            "ts" => &mut event.ts,
            "event" => &mut event.event,
            "skill" => &mut event.skill,
            "skill_version" => &mut event.skill_version,
            "target" => &mut event.target,
            "scope" => &mut event.scope,
            "mode" => &mut event.mode,
            "path" => &mut event.path,
            "source_hash" => &mut event.source_hash,
            "outcome" => &mut event.outcome,
            "error" => &mut event.error,
            "tool_version" => &mut event.tool_version,
            "actor" => &mut event.actor,
            _ => continue,
        };
        if value.trim_ascii() != b"null" {
            *field = serde_json::from_str(&repair_json_strings(value)).ok()?;
        }
    }
    Some(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn null_retains_previous_string_and_invalid_bytes_are_repaired_per_byte() {
        let event = decode(b"{\"skill\":\"demo\",\"skill\":null,\"target\":\"\xe2\x82\"}").unwrap();
        assert_eq!(event.skill, "demo");
        assert_eq!(event.target, "\u{fffd}\u{fffd}");
        assert!(decode(br#"{"skill":4,"skill":"demo"}"#).is_none());
    }
}
