//! Ordered rotation fields over original JSON byte spans and local Go time.
use crate::{
    startup_fallback::Entry,
    startup_json_scan::{self, Kind},
    startup_time::Time,
};
use symbrain_core::GoText;

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
fn string(raw: &[u8]) -> String {
    serde_json::from_str(&symbrain_core::go_json_compatible_text(raw))
        .expect("scanner admitted JSON string")
}
fn kind(value: Kind) -> &'static str {
    match value {
        Kind::Array => "array",
        Kind::Object => "object",
        Kind::String => "string",
        Kind::Number => "number",
        Kind::Bool => "bool",
        Kind::Null => "null",
    }
}
fn type_error(value: Kind, target: &str) -> GoText {
    format!(
        "json: cannot unmarshal {} into Go value of type {target}",
        kind(value)
    )
    .into()
}

pub(super) fn parse(bytes: &[u8]) -> Result<Records, GoText> {
    // Syntax is checked completely before typed decoding, exactly as Go
    // Unmarshal. Unknown fields retain grammar/depth without float conversion.
    let nodes = startup_json_scan::scan(bytes)?;
    let root = &nodes[0];
    if root.kind == Kind::Null {
        return Ok(Records {
            entries: Vec::new(),
            nil: true,
        });
    }
    if root.kind != Kind::Array {
        return Err(type_error(root.kind, "[]security.fallbackEntry"));
    }
    let mut entries = Vec::new();
    let mut first_type_error = None;
    let children = Children::new(&nodes);
    for index in children.of(0) {
        let node = &nodes[index];
        let mut entry = Entry {
            secret: String::new(),
            expires: Time::zero(),
        };
        if node.kind == Kind::Object {
            for child_index in children.of(index) {
                let child = &nodes[child_index];
                let name = string(&bytes[child.key.clone().expect("object key")]);
                if child.kind == Kind::Null {
                    continue;
                }
                let raw = &bytes[child.span.clone()];
                if field(&name, "secret") {
                    if child.kind == Kind::String {
                        entry.secret = string(raw);
                    } else if first_type_error.is_none() {
                        first_type_error = Some(format!("json: cannot unmarshal {} into Go struct field fallbackEntry.secret of type string",kind(child.kind)).into());
                    }
                } else if field(&name, "expires_at") {
                    // An UnmarshalJSON error is immediately fatal in Go,
                    // including when an earlier ordinary type error was saved.
                    entry.expires = Time::json(raw)?;
                }
            }
        } else if node.kind != Kind::Null && first_type_error.is_none() {
            first_type_error = Some(type_error(node.kind, "security.fallbackEntry"));
        }
        entries.push(entry);
    }
    if let Some(error) = first_type_error {
        return Err(error);
    }
    Ok(Records {
        entries,
        nil: false,
    })
}

pub(super) fn render(entries: &Records) -> Result<String, GoText> {
    if entries.nil {
        return Ok("null".to_owned());
    }
    let mut records = Vec::new();
    for entry in &entries.entries {
        let expiry = entry.expires.render().map_err(|error| {
            error.with_prefix("json: error calling MarshalJSON for type time.Time: ")
        })?;
        let secret = symbrain_core::go_json_string_bytes(entry.secret.as_bytes())
            .expect("valid JSON string");
        let expiry =
            symbrain_core::go_json_string_bytes(expiry.as_bytes()).expect("valid JSON timestamp");
        records.push(format!(
            "{{\"secret\":{},\"expires_at\":{}}}",
            secret.get(),
            expiry.get()
        ));
    }
    Ok(format!("[{}]", records.join(",")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn preserves_duplicate_and_folded_field_order_and_nanosecond_time() {
        let rows = super::parse(br#"[{"secret":"old","Secret":"new","expires_at":"2099-01-01T00:00:00.1234Z","unknown":1e400}]"#).unwrap();
        assert_eq!(rows.entries[0].secret, "new");
        assert_eq!(
            super::render(&rows).unwrap(),
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
        assert_eq!(
            super::render(&super::parse(b"null").unwrap()).unwrap(),
            "null"
        );
        assert_eq!(super::render(&super::parse(b"[]").unwrap()).unwrap(), "[]");
    }
}

// Build direct-child links in one reverse pass. Each link is visited only by
// its owning root/object decoder; wide input never rescans unrelated nodes.
struct Children {
    first: Vec<usize>,
    next: Vec<usize>,
}
impl Children {
    fn new(nodes: &[crate::startup_json_scan::Node]) -> Self {
        let mut links = Self {
            first: vec![usize::MAX; nodes.len()],
            next: vec![usize::MAX; nodes.len()],
        };
        for (index, node) in nodes.iter().enumerate().rev() {
            if let Some(parent) = node.parent {
                links.next[index] = links.first[parent];
                links.first[parent] = index;
            }
        }
        links
    }
    fn of(&self, parent: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(
            (self.first[parent] != usize::MAX).then_some(self.first[parent]),
            |&index| (self.next[index] != usize::MAX).then_some(self.next[index]),
        )
    }
}
