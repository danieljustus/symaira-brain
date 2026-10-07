//! `BurntSushi` metadata order for successfully decoded Guard configurations.
//! Keep the parsed source spans: mutable TOML documents discard them, and a
//! tree traversal alone reorders later table declarations. Repeated array
//! element paths are separate metadata entries, not a set of warning names.
use std::{fmt::Write, path::Path};
use symbrain_core::{GoText, config::format_go_quoted};
use toml_edit::{Item, Table, TableLike, Value};

struct KeyEvent {
    position: usize,
    path: Vec<String>,
}

pub(super) fn collect(table: &Table, path: &Path) -> Option<Vec<GoText>> {
    let mut events = Vec::new();
    walk(table, &mut Vec::new(), &mut events)?;
    events.sort_by_key(|event| event.position);
    Some(
        events
            .into_iter()
            .filter(|event| !decoded(&event.path))
            .map(|event| {
                let key = toml_key(&event.path);
                let quoted = format_go_quoted(std::ffi::OsStr::new(&key));
                GoText::path(
                    &format!("config: warning: unknown key {quoted} in "),
                    path,
                    "\n",
                )
            })
            .collect(),
    )
}

fn event(position: usize, path: &[String], events: &mut Vec<KeyEvent>) {
    events.push(KeyEvent {
        position,
        path: path.to_vec(),
    });
}

fn walk(table: &dyn TableLike, path: &mut Vec<String>, events: &mut Vec<KeyEvent>) -> Option<()> {
    for (key, item) in table.iter() {
        path.push(key.to_owned());
        match item {
            Item::Value(value) => {
                if !value
                    .as_inline_table()
                    .is_some_and(toml_edit::InlineTable::is_dotted)
                {
                    event(table.key(key)?.span()?.start, path, events);
                }
                walk_value(value, path, events)?;
            }
            Item::Table(child) => {
                if !child.is_implicit() && !child.is_dotted() {
                    event(child.span()?.start, path, events);
                }
                walk(child, path, events)?;
            }
            Item::ArrayOfTables(children) => {
                for child in children {
                    event(child.span()?.start, path, events);
                    walk(child, path, events)?;
                }
            }
            Item::None => return None,
        }
        path.pop();
    }
    Some(())
}

fn walk_value(value: &Value, path: &mut Vec<String>, events: &mut Vec<KeyEvent>) -> Option<()> {
    match value {
        Value::InlineTable(table) => walk(table, path, events)?,
        Value::Array(array) => {
            for value in array {
                walk_value(value, path, events)?;
            }
        }
        _ => {}
    }
    Some(())
}

// All known fields have already passed exact typed decoding. Defaults is a
// dynamic map; the other sections are structs, slices of structs or strings.
fn decoded(path: &[String]) -> bool {
    let parts: Vec<_> = path.iter().map(String::as_str).collect();
    matches!(
        parts.as_slice(),
        ["defaults", ..]
            | ["rules" | "proxy" | "audit" | "remote" | "sequence" | "spawn"]
            | ["rules", "decision" | "match"]
            | [
                "rules",
                "match",
                "server" | "tool" | "capability" | "command_contains",
            ]
            | ["proxy", "upstream"]
            | ["audit", "path" | "encrypt_age" | "encrypt"]
            | [
                "remote",
                "name" | "provider" | "host" | "trust_level" | "allowed_servers" | "labels",
            ]
            | ["sequence", "enabled" | "threshold"]
            | ["spawn", "allowlist"]
            | ["spawn", "allowlist", "path" | "argv_prefix"]
    )
}

// toml.Key.String first quotes each non-bare segment using the TOML encoder's
// double-quoted replacement, then Go fmt %q quotes that complete dotted key.
fn toml_key(path: &[String]) -> String {
    path.iter()
        .map(|key| {
            if !key.is_empty()
                && key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            {
                return key.clone();
            }
            let mut quoted = String::from("\"");
            for c in key.chars() {
                match c {
                    '"' => quoted.push_str("\\\""),
                    '\\' => quoted.push_str("\\\\"),
                    '\u{8}' => quoted.push_str("\\b"),
                    '\t' => quoted.push_str("\\t"),
                    '\n' => quoted.push_str("\\n"),
                    '\u{c}' => quoted.push_str("\\f"),
                    '\r' => quoted.push_str("\\r"),
                    '\u{0}'..='\u{1f}' | '\u{7f}' => {
                        let _ = write!(quoted, "\\u{:04x}", u32::from(c));
                    }
                    _ => quoted.push(c),
                }
            }
            quoted.push('"');
            quoted
        })
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use toml_edit::Document;

    #[test]
    fn metadata_retains_declaration_order_and_repeated_array_paths() {
        let text = "z=[{nested=1},{nested=2}]\n[audit]\nlast=1\nfirst=2\n[owned.child]\nx=1\n[owned]\ny=2\n[proxy]\nunknown=3\n";
        let doc = Document::parse(text).unwrap();
        let warnings = collect(doc.as_table(), Path::new("owned.toml")).unwrap();
        let keys: Vec<_> = warnings
            .iter()
            .map(|line| std::str::from_utf8(line.as_ref()).unwrap())
            .collect();
        assert_eq!(
            keys,
            [
                "config: warning: unknown key \"z\" in owned.toml\n",
                "config: warning: unknown key \"z.nested\" in owned.toml\n",
                "config: warning: unknown key \"z.nested\" in owned.toml\n",
                "config: warning: unknown key \"audit.last\" in owned.toml\n",
                "config: warning: unknown key \"audit.first\" in owned.toml\n",
                "config: warning: unknown key \"owned.child\" in owned.toml\n",
                "config: warning: unknown key \"owned.child.x\" in owned.toml\n",
                "config: warning: unknown key \"owned\" in owned.toml\n",
                "config: warning: unknown key \"owned.y\" in owned.toml\n",
                "config: warning: unknown key \"proxy.unknown\" in owned.toml\n",
            ]
        );
    }

    #[test]
    fn dynamic_defaults_and_toml_key_representation_are_distinct() {
        let doc =
            Document::parse("defaults={custom=\"allow\"}\n\"owned.dot\"=1\n\"owned\\u0085\"=2\n")
                .unwrap();
        let warnings = collect(doc.as_table(), Path::new("owned.toml")).unwrap();
        assert_eq!(warnings.len(), 2);
        assert_eq!(
            warnings[0].as_ref(),
            b"config: warning: unknown key \"\\\"owned.dot\\\"\" in owned.toml\n"
        );
        assert_eq!(
            warnings[1].as_ref(),
            b"config: warning: unknown key \"\\\"owned\\u0085\\\"\" in owned.toml\n"
        );
    }
}
