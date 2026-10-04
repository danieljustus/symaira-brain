//! Small parser for the immutable, line-terminated owned migration resources.

use crate::StoreError;
use rusqlite::{Connection, OptionalExtension};
use std::collections::BTreeSet;

pub(super) fn statements(sql: &str) -> Result<Vec<String>, StoreError> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut trigger = false;
    for line in sql.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("--") {
            continue;
        }
        if current.is_empty() {
            trigger = line.to_ascii_uppercase().starts_with("CREATE TRIGGER ");
        }
        current.push_str(line);
        current.push('\n');
        if (!trigger && line.ends_with(';')) || (trigger && line.eq_ignore_ascii_case("END;")) {
            result.push(std::mem::take(&mut current));
            trigger = false;
        }
    }
    if !current.is_empty() {
        return Err(StoreError::Invalid(
            "unterminated owned migration SQL".into(),
        ));
    }
    Ok(result)
}

/// Keyword/whitespace differences are harmless; quoted values retain their case
/// and whitespace. In particular 'TRUE' must not match the 'true' sync predicate.
pub(super) fn canonical(sql: &str) -> String {
    let mut result = String::new();
    let mut quote = None;
    for ch in sql.trim().trim_end_matches(';').chars() {
        if let Some(end) = quote {
            result.push(ch);
            if ch == end {
                quote = None;
            }
        } else if matches!(ch, '\'' | '"' | '[') {
            quote = Some(if ch == '[' { ']' } else { ch });
            result.push(ch);
        } else if !ch.is_ascii_whitespace() {
            result.push(ch.to_ascii_lowercase());
        }
    }
    for prefix in [
        "createindex",
        "createuniqueindex",
        "createtable",
        "createvirtualtable",
        "createtrigger",
    ] {
        let qualified = format!("{prefix}ifnotexists");
        if let Some(rest) = result.strip_prefix(&qualified) {
            return format!("{prefix}{rest}");
        }
    }
    result
}

pub(super) fn object_name<'a>(sql: &'a str, kind: &str) -> Option<&'a str> {
    let words = sql.split_whitespace().collect::<Vec<_>>();
    let index = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case(kind))?
        + 1;
    let index = if words
        .get(index)
        .is_some_and(|word| word.eq_ignore_ascii_case("IF"))
    {
        index
            + if words
                .get(index + 1)
                .is_some_and(|word| word.eq_ignore_ascii_case("NOT"))
            {
                3
            } else {
                2
            }
    } else {
        index
    };
    words.get(index).map(|word| word.trim_end_matches(';'))
}

pub(super) fn object(
    conn: &Connection,
    name: &str,
) -> Result<Option<(String, String)>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT type, COALESCE(sql,'') FROM sqlite_schema WHERE name=?",
            [name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

pub(super) fn column_present(
    conn: &Connection,
    table: &str,
    column: &str,
) -> Result<bool, StoreError> {
    // Identifiers originate in immutable owned SQL, never caller content.
    Ok(conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == column))
}

pub(super) fn alter(sql: &str) -> Option<(&str, &str)> {
    let words = sql.split_whitespace().collect::<Vec<_>>();
    (words.len() >= 7 && words[0..2] == ["ALTER", "TABLE"] && words[3..5] == ["ADD", "COLUMN"])
        .then(|| (words[2], words[5]))
}

pub(super) fn ensure_index(conn: &Connection, statement: &str) -> Result<(), StoreError> {
    let name = object_name(statement, "INDEX")
        .ok_or_else(|| StoreError::Invalid("owned index has no name".into()))?;
    if let Some((kind, stored)) = object(conn, name)? {
        if kind != "index" || canonical(&stored) != canonical(statement) {
            return Err(StoreError::Invalid(format!(
                "incompatible owned index {name}"
            )));
        }
    } else {
        conn.execute_batch(statement)?;
    }
    Ok(())
}

/// Extract real CHECK clauses, excluding literals/comments that merely mention
/// them. Definitions come from sqlite_schema after SQLite has parsed the DDL.
pub(super) fn checks(definition: &str) -> BTreeSet<String> {
    let text = definition.chars().collect::<Vec<_>>();
    let mut result = BTreeSet::new();
    let mut index = 0;
    while index < text.len() {
        if skip_quoted_or_comment(&text, &mut index) {
            continue;
        }
        if text[index].is_ascii_alphabetic() || text[index] == '_' {
            let start = index;
            while index < text.len() && (text[index].is_ascii_alphanumeric() || text[index] == '_')
            {
                index += 1;
            }
            if !text[start..index]
                .iter()
                .collect::<String>()
                .eq_ignore_ascii_case("CHECK")
            {
                continue;
            }
            while index < text.len() {
                if text[index].is_ascii_whitespace() {
                    index += 1;
                } else if !skip_comment(&text, &mut index) {
                    break;
                }
            }
            if text.get(index) != Some(&'(') {
                continue;
            }
            let start = index;
            let mut depth = 0;
            while index < text.len() {
                if skip_quoted_or_comment(&text, &mut index) {
                    continue;
                }
                match text[index] {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                index += 1;
                if depth == 0 {
                    result.insert(canonical(&text[start..index].iter().collect::<String>()));
                    break;
                }
            }
        } else {
            index += 1;
        }
    }
    result
}

fn skip_quoted_or_comment(text: &[char], index: &mut usize) -> bool {
    if skip_comment(text, index) {
        return true;
    }
    let Some(open @ ('\'' | '"' | '`' | '[')) = text.get(*index).copied() else {
        return false;
    };
    let close = if open == '[' { ']' } else { open };
    *index += 1;
    while *index < text.len() {
        if text[*index] == close {
            *index += 1;
            if close != ']' && text.get(*index) == Some(&close) {
                *index += 1;
            } else {
                break;
            }
        } else {
            *index += 1;
        }
    }
    true
}

fn skip_comment(text: &[char], index: &mut usize) -> bool {
    if text.get(*index..*index + 2) == Some(&['-', '-']) {
        *index += 2;
        while *index < text.len() && text[*index] != '\n' {
            *index += 1;
        }
        true
    } else if text.get(*index..*index + 2) == Some(&['/', '*']) {
        *index += 2;
        while *index < text.len() && text.get(*index..*index + 2) != Some(&['*', '/']) {
            *index += 1;
        }
        *index = (*index + 2).min(text.len());
        true
    } else {
        false
    }
}
