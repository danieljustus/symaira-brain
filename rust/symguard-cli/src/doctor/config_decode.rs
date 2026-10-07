//! Typed configuration decoding before Go-compatible semantic validation.
use super::{LoadedConfig, SpawnEntry};
use std::path::Path;
use symbrain_core::config::format_go_quoted;
use toml_edit::{Item, Table, TableLike};

const DECISIONS: [&str; 6] = ["allow", "ask", "deny", "redact", "readonly", "sandbox"];

pub(super) struct DecodedConfig {
    defaults: Vec<(String, String)>,
    rules: Vec<(String, bool)>,
    enabled: bool,
    threshold: i64,
    allowlist: Vec<SpawnEntry>,
}

impl DecodedConfig {
    /// `None` preserves the Go map-order gate when several defaults fail.
    pub(super) fn validate(self) -> Option<Result<LoadedConfig, String>> {
        let invalid: Vec<_> = self
            .defaults
            .iter()
            .filter(|(_, value)| !DECISIONS.contains(&value.as_str()))
            .collect();
        if invalid.len() > 1 {
            return None;
        }
        if let Some((name, value)) = invalid.first() {
            return Some(Err(format!(
                "defaults.{}: invalid decision {} (allowed: allow, ask, deny, redact, readonly, sandbox)",
                quote(name),
                quote(value)
            )));
        }
        for (index, (decision, criteria)) in self.rules.iter().enumerate() {
            if !DECISIONS.contains(&decision.as_str()) {
                return Some(Err(format!(
                    "rules[{index}].decision: invalid decision {}",
                    quote(decision)
                )));
            }
            if !criteria {
                return Some(Err(format!(
                    "rules[{index}]: match must specify at least one criterion (server, tool, capability, command_contains)"
                )));
            }
        }
        // Go resolves an explicit zero to three before applying this check.
        if self.enabled && self.threshold != 0 && self.threshold < 2 {
            return Some(Err(format!(
                "sequence.threshold: must be at least 2 when sequence is enabled (got {})",
                self.threshold
            )));
        }
        for (index, entry) in self.allowlist.iter().enumerate() {
            if entry.path.is_empty() {
                return Some(Err(format!("spawn.allowlist[{index}]: path is required")));
            }
            if !Path::new(&entry.path).is_absolute() {
                return Some(Err(format!(
                    "spawn.allowlist[{index}]: path {} must be absolute",
                    quote(&entry.path)
                )));
            }
        }
        Some(Ok(LoadedConfig {
            rules: self.rules.len(),
            allowlist: self.allowlist,
        }))
    }
}

/// Decode every known field first: Go TOML type errors precede validation.
/// Unknown fields are ignored by Go; unproven case-fold aliases remain gated.
pub(super) fn decode_config(doc: &Table) -> Option<DecodedConfig> {
    known_keys(
        doc,
        &[
            "defaults", "rules", "proxy", "audit", "remote", "sequence", "spawn",
        ],
    )?;
    let defaults = if let Some(item) = doc.get("defaults") {
        item.as_table_like()?
            .iter()
            .map(|(key, value)| Some((key.to_owned(), value.as_str()?.to_owned())))
            .collect::<Option<Vec<_>>>()?
    } else {
        Vec::new()
    };
    let mut rules = Vec::new();
    if let Some(item) = doc.get("rules") {
        for rule in table_array(item)? {
            known_keys(rule, &["decision", "match"])?;
            let decision = string_field(rule, "decision")?;
            let criteria = if let Some(item) = rule.get("match") {
                let matcher = item.as_table_like()?;
                known_keys(
                    matcher,
                    &["server", "tool", "capability", "command_contains"],
                )?;
                let server = string_field(matcher, "server")?;
                let tool = string_field(matcher, "tool")?;
                let capability = string_field(matcher, "capability")?;
                let commands = string_array_field(matcher, "command_contains")?;
                !server.is_empty()
                    || !tool.is_empty()
                    || !capability.is_empty()
                    || !commands.is_empty()
            } else {
                false
            };
            rules.push((decision, criteria));
        }
    }
    let (enabled, threshold) = if let Some(item) = doc.get("sequence") {
        let table = item.as_table_like()?;
        known_keys(table, &["enabled", "threshold"])?;
        let enabled = table.get("enabled").map_or(Some(false), Item::as_bool)?;
        let threshold = table.get("threshold").map_or(Some(3), Item::as_integer)?;
        (enabled, threshold)
    } else {
        (false, 3)
    };
    decode_unprinted(doc)?;
    let mut allowlist = Vec::new();
    if let Some(item) = doc.get("spawn") {
        let table = item.as_table_like()?;
        known_keys(table, &["allowlist"])?;
        if let Some(item) = table.get("allowlist") {
            for entry in table_array(item)? {
                known_keys(entry, &["path", "argv_prefix"])?;
                allowlist.push(SpawnEntry {
                    path: string_field(entry, "path")?,
                    argv_prefix: string_array_field(entry, "argv_prefix")?,
                });
            }
        }
    }
    Some(DecodedConfig {
        defaults,
        rules,
        enabled,
        threshold,
        allowlist,
    })
}

fn known_keys(table: &dyn TableLike, names: &[&str]) -> Option<()> {
    table
        .iter()
        .all(|(key, _)| {
            // BurntSushi matches struct fields with strings.EqualFold. Keep
            // aliases Go-owned until typed resolution/duplicate precedence is
            // proved; treating them as unknown would hide configured fields.
            names.contains(&key)
                || !names.iter().any(|name| {
                    key.chars()
                        .map(|c| match c {
                            '\u{017f}' => 's',
                            '\u{212a}' => 'k',
                            _ => c.to_ascii_lowercase(),
                        })
                        .eq(name.chars())
                })
        })
        .then_some(())
}

fn string_field(table: &dyn TableLike, name: &str) -> Option<String> {
    table.get(name).map_or(Some(String::new()), |value| {
        value.as_str().map(str::to_owned)
    })
}

fn string_array_field(table: &dyn TableLike, name: &str) -> Option<Vec<String>> {
    table.get(name).map_or(Some(Vec::new()), |value| {
        value
            .as_array()?
            .iter()
            .map(|value| value.as_str().map(str::to_owned))
            .collect()
    })
}

fn decode_unprinted(doc: &Table) -> Option<()> {
    if let Some(item) = doc.get("proxy") {
        let table = item.as_table_like()?;
        known_keys(table, &["upstream"])?;
        string_field(table, "upstream")?;
    }
    if let Some(item) = doc.get("audit") {
        let table = item.as_table_like()?;
        known_keys(table, &["path", "encrypt_age", "encrypt"])?;
        string_field(table, "path")?;
        string_field(table, "encrypt_age")?;
        if let Some(value) = table.get("encrypt") {
            value.as_bool()?;
        }
    }
    if let Some(item) = doc.get("remote") {
        for table in table_array(item)? {
            known_keys(
                table,
                &[
                    "name",
                    "provider",
                    "host",
                    "trust_level",
                    "allowed_servers",
                    "labels",
                ],
            )?;
            for name in ["name", "provider", "host", "trust_level"] {
                string_field(table, name)?;
            }
            for name in ["allowed_servers", "labels"] {
                string_array_field(table, name)?;
            }
        }
    }
    Some(())
}

// BurntSushi/toml accepts equivalent inline tables and arrays of inline tables,
// including empty arrays. Admit only typed table elements; scalar/array nesting
// stays a decoder-error boundary instead of being mistaken for an empty list.
fn table_array(item: &Item) -> Option<Vec<&dyn TableLike>> {
    if let Some(tables) = item.as_array_of_tables() {
        return Some(tables.iter().map(|table| table as &dyn TableLike).collect());
    }
    item.as_array()?
        .iter()
        .map(|value| value.as_inline_table().map(|table| table as &dyn TableLike))
        .collect()
}

fn quote(value: &str) -> String {
    format_go_quoted(std::ffi::OsStr::new(value))
}
