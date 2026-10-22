//! Port of `github.com/BurntSushi/toml` v1.6.0 `parse.go`.
//!
//! Only the behavior observable through `toml.Unmarshal` into a
//! `map[string]any` is kept: the resulting tree and, above all, the exact
//! error text (`toml: line N[ (last key "k")]: message`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use super::lexer::{Item, Kind, Lexer};
use super::time;
use crate::go_quote::quote_str;

pub(crate) type Table = BTreeMap<String, Value>;

/// The `any` values BurntSushi/toml produces.
#[derive(Clone, Debug)]
pub(crate) enum Value {
    String(String),
    /// Integer, float, bool or datetime: never inspected by harness code.
    Scalar,
    /// `[]any`
    Array(Vec<Value>),
    /// `map[string]any`
    Table(Table),
    /// `[]map[string]any` (an array of tables)
    TableArray(Vec<Table>),
}

pub(super) enum Failure {
    Parse {
        message: String,
        line: i64,
        last_key: String,
    },
    /// Go would panic with a non-`ParseError` value ("BUG: ...").
    Bug(String),
}

type Result<T> = std::result::Result<T, Failure>;

pub(super) struct Parser<'a> {
    lexer: Lexer<'a>,
    context: Vec<String>,
    current_key: String,
    pos_line: i64,
    key_info: HashMap<String, bool>,
    mapping: Table,
    implicits: HashSet<String>,
}

/// `Key.String()`.
fn key_string(parts: &[String]) -> String {
    let mut out = String::new();
    'outer: for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            out.push('.');
        }
        if part.is_empty() {
            out.push_str("\"\"");
            continue;
        }
        for ch in part.chars() {
            if !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-') {
                out.push('"');
                out.push_str(&dbl_quoted(part));
                out.push('"');
                continue 'outer;
            }
        }
        out.push_str(part);
    }
    out
}

/// `dblQuotedReplacer`.
fn dbl_quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '\0'..='\u{1f}' | '\u{7f}' => {
                let _ = write!(out, "\\u{:04x}", u32::from(ch));
            }
            other => out.push(other),
        }
    }
    out
}

fn table_at<'t>(root: &'t mut Table, path: &[String]) -> Option<&'t mut Table> {
    let mut current = root;
    for part in path {
        current = match current.get_mut(part)? {
            Value::Table(table) => table,
            Value::TableArray(tables) => tables.last_mut()?,
            _ => return None,
        };
    }
    Some(current)
}

fn num_has_leading_zero(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() > 1 && bytes[0] == b'0' && !matches!(bytes[1], b'b' | b'o' | b'x') {
        return true;
    }
    bytes.len() > 2 && matches!(bytes[0], b'-' | b'+') && bytes[1] == b'0'
}

fn num_underscores_ok(text: &str) -> bool {
    if matches!(text, "nan" | "+nan" | "-nan" | "inf" | "-inf" | "+inf") {
        return true;
    }
    let mut accept = false;
    for ch in text.chars() {
        if ch == '_' && !accept {
            return false;
        }
        accept = ch.is_ascii_hexdigit();
    }
    accept
}

fn num_periods_ok(text: &str) -> bool {
    let mut period = false;
    for ch in text.chars() {
        if period && !ch.is_ascii_digit() {
            return false;
        }
        period = ch == '.';
    }
    !period
}

enum NumberError {
    Range,
    Syntax,
}

/// `strconv.ParseInt(s, 0, 64)`.
fn parse_int(text: &str) -> std::result::Result<(), NumberError> {
    let (negative, unsigned) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let bytes = unsigned.as_bytes();
    if bytes.is_empty() {
        return Err(NumberError::Syntax);
    }
    let lower = |index: usize| bytes[index].to_ascii_lowercase();
    let (base, digits): (u32, &[u8]) = if bytes[0] == b'0' {
        if bytes.len() >= 3 && lower(1) == b'b' {
            (2, &bytes[2..])
        } else if bytes.len() >= 3 && lower(1) == b'o' {
            (8, &bytes[2..])
        } else if bytes.len() >= 3 && lower(1) == b'x' {
            (16, &bytes[2..])
        } else {
            (8, &bytes[1..])
        }
    } else {
        (10, bytes)
    };
    let mut value: u128 = 0;
    for &byte in digits {
        if byte == b'_' {
            continue;
        }
        let Some(digit) = char::from(byte).to_digit(base) else {
            return Err(NumberError::Syntax);
        };
        value = value
            .saturating_mul(u128::from(base))
            .saturating_add(u128::from(digit));
    }
    let limit: u128 = if negative { 1 << 63 } else { (1 << 63) - 1 };
    if value > limit {
        return Err(NumberError::Range);
    }
    Ok(())
}

/// `strconv.ParseFloat(s, 64)` as far as validity and range are concerned.
fn parse_float(text: &str) -> std::result::Result<(), NumberError> {
    let (signed, rest) = match text.as_bytes().first() {
        Some(b'+' | b'-') => (true, &text[1..]),
        _ => (false, text),
    };
    let lower = rest.to_ascii_lowercase();
    if lower == "inf" || lower == "infinity" || (!signed && lower == "nan") {
        return Ok(());
    }
    let bytes = rest.as_bytes();
    let mut index = 0;
    let mut mantissa_digits = 0;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
        mantissa_digits += 1;
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
            mantissa_digits += 1;
        }
    }
    if mantissa_digits == 0 {
        return Err(NumberError::Syntax);
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let exponent_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == exponent_start {
            return Err(NumberError::Syntax);
        }
    }
    if index != bytes.len() {
        return Err(NumberError::Syntax);
    }
    match text.parse::<f64>() {
        Ok(value) if value.is_infinite() => Err(NumberError::Range),
        Ok(_) => Ok(()),
        Err(_) => Err(NumberError::Syntax),
    }
}

fn strip_first_newline(text: &str) -> &str {
    if let Some(rest) = text.strip_prefix('\n') {
        return rest;
    }
    if let Some(rest) = text.strip_prefix("\r\n") {
        return rest;
    }
    text
}

/// `stripEscapedNewlines`.
fn strip_escaped_newlines(text: &str) -> String {
    let mut remaining = text.as_bytes().to_vec();
    let mut output: Vec<u8> = Vec::with_capacity(remaining.len());
    let mut index = 0;
    loop {
        let Some(offset) = remaining[index..].iter().position(|&byte| byte == b'\\') else {
            output.extend_from_slice(&remaining);
            return String::from_utf8_lossy(&output).into_owned();
        };
        index += offset;
        if remaining.get(index + 1) == Some(&b'\\') {
            index += 2;
            continue;
        }
        let mut end = index + 1;
        while end < remaining.len() && matches!(remaining[end], b' ' | b'\t' | b'\r' | b'\n') {
            end += 1;
        }
        if end == index + 1 {
            index += 1;
            continue;
        }
        if !remaining[index..end].contains(&b'\n') {
            index += 1;
            continue;
        }
        output.extend_from_slice(&remaining[..index]);
        remaining = remaining[end..].to_vec();
        index = 0;
    }
}

impl<'a> Parser<'a> {
    pub(super) fn new(input: &'a [u8]) -> Self {
        Self {
            lexer: Lexer::new(input),
            context: Vec::new(),
            current_key: String::new(),
            pos_line: 0,
            key_info: HashMap::new(),
            mapping: Table::new(),
            implicits: HashSet::new(),
        }
    }

    pub(super) fn parse(mut self) -> Result<Table> {
        loop {
            let item = self.next()?;
            if item.kind == Kind::Eof {
                break;
            }
            self.top_level(&item)?;
        }
        Ok(self.mapping)
    }

    fn current(&self) -> String {
        if self.current_key.is_empty() {
            return key_string(&self.context);
        }
        if self.context.is_empty() {
            return self.current_key.clone();
        }
        format!("{}.{}", key_string(&self.context), self.current_key)
    }

    fn fail(&self, line: i64, message: String) -> Failure {
        Failure::Parse {
            message,
            line,
            last_key: self.current(),
        }
    }

    fn panicf(&self, message: String) -> Failure {
        self.fail(self.pos_line, message)
    }

    fn next(&mut self) -> Result<Item> {
        let item = self.lexer.next_item();
        if item.kind == Kind::Error {
            let message = item.err.clone().unwrap_or_else(|| item.val.clone());
            return Err(self.fail(item.line, message));
        }
        Ok(item)
    }

    fn next_pos(&mut self) -> Result<Item> {
        let item = self.next()?;
        self.pos_line = item.line;
        Ok(item)
    }

    fn expect(&mut self, kind: Kind) -> Result<Item> {
        let item = self.next()?;
        Self::assert_kind(kind, item.kind)?;
        Ok(item)
    }

    fn assert_kind(expected: Kind, got: Kind) -> Result<()> {
        if expected == got {
            Ok(())
        } else {
            Err(Failure::Bug(format!(
                "Expected '{expected:?}' but got '{got:?}'."
            )))
        }
    }

    fn top_level(&mut self, item: &Item) -> Result<()> {
        match item.kind {
            Kind::CommentStart => {
                self.expect(Kind::Text)?;
            }
            Kind::TableStart | Kind::ArrayTableStart => {
                let (end, array) = if item.kind == Kind::TableStart {
                    (Kind::TableEnd, false)
                } else {
                    (Kind::ArrayTableEnd, true)
                };
                let mut name = self.next_pos()?;
                let mut key = Vec::new();
                while name.kind != end && name.kind != Kind::Eof {
                    key.push(self.key_part(&name)?);
                    name = self.next()?;
                }
                Self::assert_kind(end, name.kind)?;
                self.add_context(&key, array)?;
                self.set_type("", false);
            }
            Kind::KeyStart => {
                let outer_context = self.context.clone();
                let key = self.read_key()?;
                let (last, parents) = key
                    .split_last()
                    .ok_or_else(|| Failure::Bug("empty key".to_owned()))?;
                self.current_key.clone_from(last);
                self.add_parent_contexts(parents)?;
                let value_item = self.next()?;
                let (value, is_array) = self.value(&value_item, false)?;
                let current = self.current_key.clone();
                self.set_value(&current, value)?;
                self.set_type(&current, is_array);
                self.context = outer_context;
                self.current_key.clear();
            }
            other => {
                return Err(Failure::Bug(format!(
                    "Unexpected type at top level: {other:?}"
                )));
            }
        }
        Ok(())
    }

    /// Reads key parts up to the `KeyEnd` item.
    fn read_key(&mut self) -> Result<Vec<String>> {
        let mut part = self.next_pos()?;
        let mut key = Vec::new();
        while part.kind != Kind::KeyEnd && part.kind != Kind::Eof {
            key.push(self.key_part(&part)?);
            part = self.next()?;
        }
        Self::assert_kind(Kind::KeyEnd, part.kind)?;
        Ok(key)
    }

    fn add_parent_contexts(&mut self, parents: &[String]) -> Result<()> {
        for part in parents {
            let mut key = self.context.clone();
            key.push(part.clone());
            self.add_implicit(&key);
            self.add_context(&key, false)?;
        }
        Ok(())
    }

    fn key_part(&mut self, item: &Item) -> Result<String> {
        match item.kind {
            Kind::Text => Ok(item.val.clone()),
            Kind::String
            | Kind::StringEsc
            | Kind::MultilineString
            | Kind::RawString
            | Kind::RawMultilineString => match self.value(item, false)?.0 {
                Value::String(text) => Ok(text),
                _ => Err(Failure::Bug("Unexpected key value".to_owned())),
            },
            other => Err(Failure::Bug(format!("Unexpected key type: {other:?}"))),
        }
    }

    fn value(&mut self, item: &Item, parent_is_array: bool) -> Result<(Value, bool)> {
        let text = |value: String| Ok((Value::String(value), false));
        match item.kind {
            Kind::String | Kind::RawString => text(item.val.clone()),
            Kind::StringEsc => text(self.replace_escapes(item, &item.val)?),
            Kind::MultilineString => {
                let stripped = strip_escaped_newlines(strip_first_newline(&item.val));
                text(self.replace_escapes(item, &stripped)?)
            }
            Kind::RawMultilineString => text(strip_first_newline(&item.val).to_owned()),
            Kind::Integer => self.value_integer(item),
            Kind::Float => self.value_float(item),
            Kind::Bool => {
                if item.val == "true" || item.val == "false" {
                    Ok((Value::Scalar, false))
                } else {
                    Err(Failure::Bug(format!(
                        "Expected boolean value, but got '{}'.",
                        item.val
                    )))
                }
            }
            Kind::Datetime => {
                let (replaced, valid) = time::validate(&item.val);
                if valid {
                    Ok((Value::Scalar, false))
                } else {
                    Err(self.fail(
                        item.line,
                        format!("invalid datetime: {}", quote_str(&replaced)),
                    ))
                }
            }
            Kind::Array => self.value_array(),
            Kind::InlineTableStart => self.value_inline_table(parent_is_array),
            other => Err(Failure::Bug(format!("Unexpected value type: {other:?}"))),
        }
    }

    fn value_integer(&self, item: &Item) -> Result<(Value, bool)> {
        let text = item.val.as_str();
        if !num_underscores_ok(text) {
            return Err(self.fail(
                item.line,
                format!(
                    "Invalid integer {}: underscores must be surrounded by digits",
                    quote_str(text)
                ),
            ));
        }
        if num_has_leading_zero(text) {
            return Err(self.fail(
                item.line,
                format!(
                    "Invalid integer {}: cannot have leading zeroes",
                    quote_str(text)
                ),
            ));
        }
        match parse_int(text) {
            Ok(()) => Ok((Value::Scalar, false)),
            Err(NumberError::Range) => {
                Err(self.fail(item.line, format!("{text} is out of range for int64")))
            }
            Err(NumberError::Syntax) => Err(Failure::Bug(format!(
                "Expected integer value, but got '{text}'."
            ))),
        }
    }

    fn value_float(&self, item: &Item) -> Result<(Value, bool)> {
        let text = item.val.as_str();
        let parts: Vec<&str> = text
            .split(['.', 'e', 'E'])
            .filter(|part| !part.is_empty())
            .collect();
        for part in &parts {
            if !num_underscores_ok(part) {
                return Err(self.fail(
                    item.line,
                    format!(
                        "Invalid float {}: underscores must be surrounded by digits",
                        quote_str(text)
                    ),
                ));
            }
        }
        if parts.first().is_some_and(|part| num_has_leading_zero(part)) {
            return Err(self.fail(
                item.line,
                format!(
                    "Invalid float {}: cannot have leading zeroes",
                    quote_str(text)
                ),
            ));
        }
        if !num_periods_ok(text) {
            return Err(self.fail(
                item.line,
                format!(
                    "Invalid float {}: '.' must be followed by one or more digits",
                    quote_str(text)
                ),
            ));
        }
        let mut cleaned = text.replace('_', "");
        if cleaned == "+nan" || cleaned == "-nan" {
            "nan".clone_into(&mut cleaned);
        }
        match parse_float(&cleaned) {
            Ok(()) => Ok((Value::Scalar, false)),
            Err(NumberError::Range) => {
                Err(self.fail(item.line, format!("{text} is out of range for float64")))
            }
            Err(NumberError::Syntax) => Err(self.fail(
                item.line,
                format!("Invalid float value: {}", quote_str(text)),
            )),
        }
    }

    fn value_array(&mut self) -> Result<(Value, bool)> {
        let key = self.current_key.clone();
        self.set_type(&key, true);
        let mut array = Vec::new();
        let mut item = self.next()?;
        while item.kind != Kind::ArrayEnd {
            if item.kind == Kind::CommentStart {
                self.expect(Kind::Text)?;
                item = self.next()?;
                continue;
            }
            array.push(self.value(&item, true)?.0);
            item = self.next()?;
        }
        Ok((Value::Array(array), true))
    }

    fn value_inline_table(&mut self, parent_is_array: bool) -> Result<(Value, bool)> {
        let mut top = Table::new();
        let outer_context = self.context.clone();
        let outer_key = self.current_key.clone();

        self.context.push(self.current_key.clone());
        let previous_context = self.context.clone();
        self.current_key.clear();

        let own_context = self.context.clone();
        self.add_implicit(&own_context);
        self.add_context(&own_context, parent_is_array)?;

        let mut item = self.next()?;
        while item.kind != Kind::InlineTableEnd {
            if item.kind == Kind::CommentStart {
                self.expect(Kind::Text)?;
                item = self.next()?;
                continue;
            }
            let key = self.read_key()?;
            let (last, parents) = key
                .split_last()
                .ok_or_else(|| Failure::Bug("empty key".to_owned()))?;
            self.current_key.clone_from(last);
            self.add_parent_contexts(parents)?;

            let value_item = self.next()?;
            let (value, is_array) = self.value(&value_item, false)?;
            let current = self.current_key.clone();
            self.set_value(&current, value.clone())?;
            self.set_type(&current, is_array);

            let mut hash = &mut top;
            for part in parents {
                let entry = hash
                    .entry(part.clone())
                    .or_insert_with(|| Value::Table(Table::new()));
                if let Value::Table(table) = entry {
                    hash = table;
                } else {
                    let message =
                        format!("{} is not a table", quote_str(&key_string(&self.context)));
                    return Err(self.panicf(message));
                }
            }
            hash.insert(current, value);

            self.context.clone_from(&previous_context);
            item = self.next()?;
        }
        self.context = outer_context;
        self.current_key = outer_key;
        Ok((Value::Table(top), false))
    }

    fn add_context(&mut self, key: &[String], array: bool) -> Result<()> {
        let (last, parents) = key
            .split_last()
            .ok_or_else(|| Failure::Bug("empty table name".to_owned()))?;
        let mut key_context: Vec<String> = Vec::with_capacity(parents.len());
        for (index, part) in parents.iter().enumerate() {
            key_context.push(part.clone());
            let exists = table_at(&mut self.mapping, &parents[..index])
                .is_some_and(|table| table.contains_key(part));
            if !exists {
                self.add_implicit(&key_context);
                if let Some(table) = table_at(&mut self.mapping, &parents[..index]) {
                    table.insert(part.clone(), Value::Table(Table::new()));
                }
            }
            let is_table = table_at(&mut self.mapping, &parents[..index])
                .and_then(|table| table.get(part))
                .is_some_and(|value| matches!(value, Value::Table(_) | Value::TableArray(_)));
            if !is_table {
                return Err(self.panicf(format!(
                    "Key '{}' was already created as a hash.",
                    key_string(&key_context)
                )));
            }
        }
        self.context = key_context;
        if array {
            let Some(table) = table_at(&mut self.mapping, parents) else {
                return Err(Failure::Bug("array context missing".to_owned()));
            };
            if !table.contains_key(last) {
                table.insert(last.clone(), Value::TableArray(Vec::new()));
            }
            if let Some(Value::TableArray(tables)) = table.get_mut(last) {
                tables.push(Table::new());
            } else {
                return Err(self.panicf(format!(
                    "Key '{}' was already created and cannot be used as an array.",
                    key_string(key)
                )));
            }
        } else {
            self.set_value(last, Value::Table(Table::new()))?;
        }
        self.context.push(last.clone());
        Ok(())
    }

    fn set_value(&mut self, key: &str, value: Value) -> Result<()> {
        let context = self.context.clone();
        let mut key_context: Vec<String> = Vec::with_capacity(context.len() + 1);
        for (index, part) in context.iter().enumerate() {
            key_context.push(part.clone());
            let kind = table_at(&mut self.mapping, &context[..index])
                .and_then(|table| table.get(part))
                .map(|value| matches!(value, Value::Table(_) | Value::TableArray(_)));
            match kind {
                None => {
                    return Err(Failure::Bug(format!(
                        "Context for key '{}' has not been established.",
                        key_string(&key_context)
                    )));
                }
                Some(false) => {
                    return Err(self.panicf(format!(
                        "Key '{}' has already been defined.",
                        key_string(&key_context)
                    )));
                }
                Some(true) => {}
            }
        }
        key_context.push(key.to_owned());
        let exists =
            table_at(&mut self.mapping, &context).is_some_and(|table| table.contains_key(key));
        if exists {
            if self.is_array(&key_context) {
                if !self.is_implicit(&key_context) {
                    return Err(self.panicf(format!(
                        "Key '{}' has already been defined.",
                        key_string(&key_context)
                    )));
                }
                self.remove_implicit(&key_context);
                if let Some(table) = table_at(&mut self.mapping, &context) {
                    table.insert(key.to_owned(), value);
                }
                return Ok(());
            }
            if self.is_implicit(&key_context) {
                self.remove_implicit(&key_context);
                return Ok(());
            }
            return Err(self.panicf(format!(
                "Key '{}' has already been defined.",
                key_string(&key_context)
            )));
        }
        if let Some(table) = table_at(&mut self.mapping, &context) {
            table.insert(key.to_owned(), value);
        }
        Ok(())
    }

    fn set_type(&mut self, key: &str, is_array: bool) {
        let mut key_context = self.context.clone();
        if !key.is_empty() {
            key_context.push(key.to_owned());
        }
        if key_context.is_empty() {
            key_context.push(String::new());
        }
        self.key_info.insert(key_string(&key_context), is_array);
    }

    fn add_implicit(&mut self, key: &[String]) {
        self.implicits.insert(key_string(key));
    }

    fn remove_implicit(&mut self, key: &[String]) {
        self.implicits.remove(&key_string(key));
    }

    fn is_implicit(&self, key: &[String]) -> bool {
        self.implicits.contains(&key_string(key))
    }

    fn is_array(&self, key: &[String]) -> bool {
        self.key_info
            .get(&key_string(key))
            .copied()
            .unwrap_or(false)
    }

    fn replace_escapes(&self, item: &Item, text: &str) -> Result<String> {
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut skip = 0;
        for (index, ch) in text.char_indices() {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if ch != '\\' {
                out.push(ch);
                continue;
            }
            let Some(&escape) = bytes.get(index + 1) else {
                return Err(Failure::Bug("Escape sequence at end of string.".to_owned()));
            };
            match escape {
                b' ' | b'\t' => {
                    return Err(self.fail(
                        item.line,
                        format!("invalid escape: '\\{}'", char::from(escape)),
                    ));
                }
                b'b' => out.push('\u{8}'),
                b't' => out.push('\t'),
                b'n' => out.push('\n'),
                b'f' => out.push('\u{c}'),
                b'r' => out.push('\r'),
                b'e' => out.push('\u{1b}'),
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'x' | b'u' | b'U' => {
                    let digits = match escape {
                        b'x' => 2,
                        b'u' => 4,
                        _ => 8,
                    };
                    let Some(hex) = text.get(index + 2..index + 2 + digits) else {
                        return Err(Failure::Bug("short escape".to_owned()));
                    };
                    out.push(self.escape_to_char(item, hex)?);
                    skip = digits + 1;
                    continue;
                }
                other => {
                    return Err(Failure::Bug(format!(
                        "Expected valid escape code after \\, but got {other}."
                    )));
                }
            }
            skip = 1;
        }
        Ok(out)
    }

    fn escape_to_char(&self, item: &Item, hex: &str) -> Result<char> {
        let Ok(code) = u32::from_str_radix(&hex.to_ascii_lowercase(), 16) else {
            return Err(Failure::Bug(format!(
                "Could not parse '{hex}' as a hexadecimal number, but the lexer claims it's OK"
            )));
        };
        char::from_u32(code).ok_or_else(|| {
            self.fail(
                item.line,
                format!("Escaped character '\\u{hex}' is not valid UTF-8."),
            )
        })
    }
}
