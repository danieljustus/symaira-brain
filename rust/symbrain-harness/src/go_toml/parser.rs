//! Port of `github.com/BurntSushi/toml` v1.6.0 `parse.go`.
//!
//! Only the behavior observable through `toml.Unmarshal` into a
//! `map[string]any` is kept: the resulting tree and, above all, the exact
//! error text (`toml: line N[ (last key "k")]: message`).

use std::collections::{BTreeMap, HashMap, HashSet, btree_map};
use std::fmt::Write as _;
use std::rc::Rc;

use super::lexer::{Item, Kind, Lexer};
use super::time;
use crate::go_quote::quote_str;

pub(crate) type Table = BTreeMap<String, Value>;

/// One key component. Contexts are copied for every dotted-key part, so the
/// components are shared instead of re-allocated (Go copies string headers).
type Part = Rc<str>;

/// The `any` values BurntSushi/toml produces.
///
/// Go's decoder accepts any nesting depth below the file-size cap, so every
/// tree walk (drop, clone) is iterative and never touches the native stack
/// in proportion to the depth.
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

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Not recursive: a derived `Debug` would overflow on deep trees.
        match self {
            Self::String(text) => write!(f, "String({text:?})"),
            Self::Scalar => f.write_str("Scalar"),
            Self::Array(values) => write!(f, "Array(len {})", values.len()),
            Self::Table(table) => write!(f, "Table(len {})", table.len()),
            Self::TableArray(tables) => write!(f, "TableArray(len {})", tables.len()),
        }
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        take_children(self, &mut pending);
        while let Some(mut value) = pending.pop() {
            take_children(&mut value, &mut pending);
        }
    }
}

/// Moves the direct children of `value` into `pending`, leaving it empty.
fn take_children(value: &mut Value, pending: &mut Vec<Value>) {
    match value {
        Value::Array(values) => pending.append(values),
        Value::Table(table) => pending.extend(std::mem::take(table).into_values()),
        Value::TableArray(tables) => {
            for table in std::mem::take(tables) {
                pending.extend(table.into_values());
            }
        }
        Value::String(_) | Value::Scalar => {}
    }
}

/// A source node of a clone: a value, or the bare table of a table array.
#[derive(Clone, Copy)]
enum Src<'a> {
    Value(&'a Value),
    Table(&'a Table),
}

/// A container being cloned: the source iterator and the clone built so far.
enum CloneFrame<'a> {
    Array(std::slice::Iter<'a, Value>, Vec<Value>),
    /// The flag tells whether the finished table is a `Value::Table` (true)
    /// or an element of a table array (false).
    Table(
        btree_map::Iter<'a, String, Value>,
        Table,
        Option<&'a str>,
        bool,
    ),
    Tables(std::slice::Iter<'a, Table>, Vec<Table>),
}

enum Cloned {
    Value(Value),
    Table(Table),
}

/// Starts cloning `src`: a leaf is cloned at once, a container yields a frame.
fn open_clone(src: Src<'_>) -> std::result::Result<CloneFrame<'_>, Cloned> {
    match src {
        Src::Value(Value::String(text)) => Err(Cloned::Value(Value::String(text.clone()))),
        Src::Value(Value::Scalar) => Err(Cloned::Value(Value::Scalar)),
        Src::Value(Value::Array(values)) => Ok(CloneFrame::Array(values.iter(), Vec::new())),
        Src::Value(Value::Table(table)) => {
            Ok(CloneFrame::Table(table.iter(), Table::new(), None, true))
        }
        Src::Value(Value::TableArray(tables)) => Ok(CloneFrame::Tables(tables.iter(), Vec::new())),
        Src::Table(table) => Ok(CloneFrame::Table(table.iter(), Table::new(), None, false)),
    }
}

impl<'a> CloneFrame<'a> {
    fn next_child(&mut self) -> Option<Src<'a>> {
        match self {
            Self::Array(iter, _) => iter.next().map(Src::Value),
            Self::Table(iter, _, pending, _) => iter.next().map(|(key, child)| {
                *pending = Some(key.as_str());
                Src::Value(child)
            }),
            Self::Tables(iter, _) => iter.next().map(Src::Table),
        }
    }

    fn accept(&mut self, child: Cloned) {
        match (self, child) {
            (Self::Array(_, out), Cloned::Value(value)) => out.push(value),
            (Self::Table(_, out, pending, _), Cloned::Value(value)) => {
                if let Some(key) = pending.take() {
                    out.insert(key.to_owned(), value);
                }
            }
            (Self::Tables(_, out), Cloned::Table(table)) => out.push(table),
            _ => unreachable!("clone frames only receive matching children"),
        }
    }

    fn finish(self) -> Cloned {
        match self {
            Self::Array(_, out) => Cloned::Value(Value::Array(out)),
            Self::Table(_, out, _, true) => Cloned::Value(Value::Table(out)),
            Self::Table(_, out, _, false) => Cloned::Table(out),
            Self::Tables(_, out) => Cloned::Value(Value::TableArray(out)),
        }
    }
}

impl Clone for Value {
    fn clone(&self) -> Self {
        let mut stack: Vec<CloneFrame<'_>> = Vec::new();
        let mut done = match open_clone(Src::Value(self)) {
            Ok(frame) => {
                stack.push(frame);
                None
            }
            Err(leaf) => Some(leaf),
        };
        loop {
            if done.is_none() {
                let frame = stack.last_mut().expect("an open frame while unfinished");
                match frame.next_child() {
                    Some(child) => match open_clone(child) {
                        Ok(next) => stack.push(next),
                        Err(leaf) => done = Some(leaf),
                    },
                    None => done = stack.pop().map(CloneFrame::finish),
                }
                continue;
            }
            let finished = done.take().expect("checked above");
            match stack.last_mut() {
                None => {
                    return match finished {
                        Cloned::Value(value) => value,
                        Cloned::Table(table) => Self::Table(table),
                    };
                }
                Some(parent) => parent.accept(finished),
            }
        }
    }
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
    context: Vec<Part>,
    current_key: String,
    pos_line: i64,
    key_info: HashMap<String, bool>,
    mapping: Table,
    implicits: HashSet<String>,
}

/// `Key.String()` over the key components.
fn key_string_iter<'p>(parts: impl Iterator<Item = &'p str>) -> String {
    let mut out = String::new();
    'outer: for (index, part) in parts.enumerate() {
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

fn key_string(parts: &[Part]) -> String {
    key_string_iter(parts.iter().map(|part| &**part))
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

/// Why [`walk_context`] stopped: the index of the offending path component.
enum WalkFailure {
    Missing(usize),
    NotTable(usize),
}

/// Descends from `root` along `context` in one pass; an array of tables is
/// entered through its last element. `Ok(None)` is an empty array of tables
/// at the end of the path (unreachable through the parser, kept for parity
/// with the lookup it replaces).
fn walk_context<'t>(
    root: &'t mut Table,
    context: &[Part],
) -> std::result::Result<Option<&'t mut Table>, WalkFailure> {
    let mut hash = root;
    for (index, part) in context.iter().enumerate() {
        let next = match hash.get_mut(&**part) {
            None => return Err(WalkFailure::Missing(index)),
            Some(Value::Table(table)) => table,
            Some(Value::TableArray(tables)) => match tables.last_mut() {
                Some(table) => table,
                None if index + 1 < context.len() => {
                    return Err(WalkFailure::Missing(index + 1));
                }
                None => return Ok(None),
            },
            Some(_) => return Err(WalkFailure::NotTable(index)),
        };
        hash = next;
    }
    Ok(Some(hash))
}

/// The walk of `addContext` over the parent components: a missing table is
/// created and marked implicit, anything that is not a table is an error
/// (the index of that component).
fn descend_creating<'t>(
    root: &'t mut Table,
    implicits: &mut HashSet<String>,
    parents: &[Part],
) -> std::result::Result<&'t mut Table, usize> {
    let mut hash = root;
    for (index, part) in parents.iter().enumerate() {
        if !hash.contains_key(&**part) {
            implicits.insert(key_string(&parents[..=index]));
            hash.insert(part.to_string(), Value::Table(Table::new()));
        }
        hash = match hash.get_mut(&**part) {
            Some(Value::Table(table)) => table,
            Some(Value::TableArray(tables)) => match tables.last_mut() {
                Some(table) => table,
                None => return Err(index),
            },
            _ => return Err(index),
        };
    }
    Ok(hash)
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

/// An open container of [`Parser::value`].
enum Frame {
    Array(Vec<Value>),
    Inline(InlineFrame),
}

/// State Go keeps in the locals of its recursive `value` for an inline table.
struct InlineFrame {
    top: Table,
    /// Context length before the table's own key was pushed.
    outer_len: usize,
    outer_key: String,
    /// Context length inside the table, restored after every pair.
    previous_len: usize,
    /// Parent components of the pair being read.
    parents: Vec<Part>,
}

/// What [`Parser::value`] does next.
enum Step {
    /// Start parsing this item (`parent_is_array` for inline tables).
    Open(Item, bool),
    /// A value is complete.
    Done(Value, bool),
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
                self.add_context(key, array)?;
                self.set_type("", false);
            }
            Kind::KeyStart => {
                // Every context change below only extends this prefix.
                let outer_len = self.context.len();
                let key = self.read_key()?;
                let (last, parents) = key
                    .split_last()
                    .ok_or_else(|| Failure::Bug("empty key".to_owned()))?;
                self.current_key.clear();
                self.current_key.push_str(last);
                self.add_parent_contexts(parents)?;
                let value_item = self.next()?;
                let (value, is_array) = self.value(&value_item, false)?;
                let current = self.current_key.clone();
                self.set_value(&current, move || value)?;
                self.set_type(&current, is_array);
                self.context.truncate(outer_len);
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
    fn read_key(&mut self) -> Result<Vec<Part>> {
        let mut part = self.next_pos()?;
        let mut key = Vec::new();
        while part.kind != Kind::KeyEnd && part.kind != Kind::Eof {
            key.push(self.key_part(&part)?);
            part = self.next()?;
        }
        Self::assert_kind(Kind::KeyEnd, part.kind)?;
        Ok(key)
    }

    fn add_parent_contexts(&mut self, parents: &[Part]) -> Result<()> {
        for part in parents {
            let mut key = self.context.clone();
            key.push(Rc::clone(part));
            self.add_implicit(&key);
            self.add_context(key, false)?;
        }
        Ok(())
    }

    fn key_part(&mut self, item: &Item) -> Result<Part> {
        match item.kind {
            Kind::Text => Ok(Part::from(item.val.as_str())),
            Kind::String
            | Kind::StringEsc
            | Kind::MultilineString
            | Kind::RawString
            | Kind::RawMultilineString => match self.value(item, false)?.0 {
                Value::String(ref text) => Ok(Part::from(text.as_str())),
                _ => Err(Failure::Bug("Unexpected key value".to_owned())),
            },
            other => Err(Failure::Bug(format!("Unexpected key type: {other:?}"))),
        }
    }

    /// Parses one value. Arrays and inline tables are walked with an explicit
    /// frame stack: Go's parser recurses on the goroutine stack (which grows
    /// on demand), so any nesting below the file-size cap must work here too.
    fn value(&mut self, item: &Item, parent_is_array: bool) -> Result<(Value, bool)> {
        let mut stack: Vec<Frame> = Vec::new();
        let mut step = self.start_value(item, parent_is_array, &mut stack)?;
        loop {
            step = match step {
                Step::Open(item, parent_is_array) => {
                    self.start_value(&item, parent_is_array, &mut stack)?
                }
                Step::Done(value, is_array) => {
                    if stack.is_empty() {
                        return Ok((value, is_array));
                    }
                    self.resume(&mut stack, value, is_array)?
                }
            };
        }
    }

    fn start_value(
        &mut self,
        item: &Item,
        parent_is_array: bool,
        stack: &mut Vec<Frame>,
    ) -> Result<Step> {
        match item.kind {
            Kind::Array => {
                let key = self.current_key.clone();
                self.set_type(&key, true);
                stack.push(Frame::Array(Vec::new()));
                self.advance(stack)
            }
            Kind::InlineTableStart => {
                let outer_len = self.context.len();
                let outer_key = self.current_key.clone();
                self.context.push(Part::from(self.current_key.as_str()));
                let previous_len = self.context.len();
                self.current_key.clear();
                let own_context = self.context.clone();
                self.add_implicit(&own_context);
                self.add_context(own_context, parent_is_array)?;
                stack.push(Frame::Inline(InlineFrame {
                    top: Table::new(),
                    outer_len,
                    outer_key,
                    previous_len,
                    parents: Vec::new(),
                }));
                self.advance(stack)
            }
            _ => self
                .leaf_value(item)
                .map(|(value, is_array)| Step::Done(value, is_array)),
        }
    }

    /// Reads the next element of the innermost open container: either the
    /// start of a nested value or the closing item, which finishes the frame.
    fn advance(&mut self, stack: &mut Vec<Frame>) -> Result<Step> {
        loop {
            let item = self.next()?;
            if matches!(stack.last(), Some(Frame::Array(_))) {
                if item.kind == Kind::ArrayEnd {
                    let Some(Frame::Array(values)) = stack.pop() else {
                        unreachable!("checked above");
                    };
                    return Ok(Step::Done(Value::Array(values), true));
                }
                if item.kind == Kind::CommentStart {
                    self.expect(Kind::Text)?;
                    continue;
                }
                return Ok(Step::Open(item, true));
            }
            if item.kind == Kind::InlineTableEnd {
                let Some(Frame::Inline(frame)) = stack.pop() else {
                    unreachable!("an inline table frame is open");
                };
                self.context.truncate(frame.outer_len);
                self.current_key = frame.outer_key;
                return Ok(Step::Done(Value::Table(frame.top), false));
            }
            if item.kind == Kind::CommentStart {
                self.expect(Kind::Text)?;
                continue;
            }
            let key = self.read_key()?;
            let (last, parents) = key
                .split_last()
                .ok_or_else(|| Failure::Bug("empty key".to_owned()))?;
            self.current_key.clear();
            self.current_key.push_str(last);
            self.add_parent_contexts(parents)?;
            if let Some(Frame::Inline(frame)) = stack.last_mut() {
                frame.parents = parents.to_vec();
            }
            let value_item = self.next()?;
            return Ok(Step::Open(value_item, false));
        }
    }

    /// Hands a finished nested value to the innermost open container.
    fn resume(&mut self, stack: &mut Vec<Frame>, value: Value, is_array: bool) -> Result<Step> {
        match stack.last_mut() {
            Some(Frame::Array(values)) => values.push(value),
            Some(Frame::Inline(frame)) => {
                let current = self.current_key.clone();
                // Go shares the decoded value between the mapping and the
                // table being built; here it is copied only when the mapping
                // actually takes it.
                self.set_value(&current, || value.clone())?;
                self.set_type(&current, is_array);

                let mut hash = &mut frame.top;
                for part in &frame.parents {
                    let entry = hash
                        .entry(part.to_string())
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

                self.context.truncate(frame.previous_len);
            }
            None => unreachable!("resume needs an open container"),
        }
        self.advance(stack)
    }

    /// Values that are not containers.
    fn leaf_value(&mut self, item: &Item) -> Result<(Value, bool)> {
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

    /// `addContext`. `key` is the full key; its last component names the new
    /// table (or array of tables) below the table named by the parents.
    fn add_context(&mut self, mut key: Vec<Part>, array: bool) -> Result<()> {
        let Some(last) = key.pop() else {
            return Err(Failure::Bug("empty table name".to_owned()));
        };
        let parents = key;
        let hash = match descend_creating(&mut self.mapping, &mut self.implicits, &parents) {
            Ok(table) => table,
            Err(index) => {
                return Err(self.panicf(format!(
                    "Key '{}' was already created as a hash.",
                    key_string(&parents[..=index])
                )));
            }
        };
        self.context = parents;
        if array {
            if !hash.contains_key(&*last) {
                hash.insert(last.to_string(), Value::TableArray(Vec::new()));
            }
            let pushed = if let Some(Value::TableArray(tables)) = hash.get_mut(&*last) {
                tables.push(Table::new());
                true
            } else {
                false
            };
            if !pushed {
                let name = key_string_iter(
                    self.context
                        .iter()
                        .map(|part| &**part)
                        .chain(std::iter::once(&*last)),
                );
                return Err(self.panicf(format!(
                    "Key '{name}' was already created and cannot be used as an array."
                )));
            }
        } else {
            self.set_value(&last, || Value::Table(Table::new()))?;
        }
        self.context.push(last);
        Ok(())
    }

    /// `setValue`. `value` is only built when the mapping takes it.
    fn set_value(&mut self, key: &str, value: impl FnOnce() -> Value) -> Result<()> {
        let Self {
            mapping,
            context,
            key_info,
            implicits,
            ..
        } = self;
        let table = match walk_context(mapping, context) {
            Ok(Some(table)) => table,
            Ok(None) => return Ok(()),
            Err(WalkFailure::Missing(index)) => {
                return Err(Failure::Bug(format!(
                    "Context for key '{}' has not been established.",
                    key_string(&context[..=index])
                )));
            }
            Err(WalkFailure::NotTable(index)) => {
                let message = format!(
                    "Key '{}' has already been defined.",
                    key_string(&context[..=index])
                );
                return Err(self.panicf(message));
            }
        };
        if table.contains_key(key) {
            let name = key_string_iter(
                context
                    .iter()
                    .map(|part| &**part)
                    .chain(std::iter::once(key)),
            );
            if key_info.get(&name).copied().unwrap_or(false) {
                if !implicits.remove(&name) {
                    return Err(self.panicf(format!("Key '{name}' has already been defined.")));
                }
                table.insert(key.to_owned(), value());
                return Ok(());
            }
            if implicits.remove(&name) {
                return Ok(());
            }
            return Err(self.panicf(format!("Key '{name}' has already been defined.")));
        }
        table.insert(key.to_owned(), value());
        Ok(())
    }

    fn set_type(&mut self, key: &str, is_array: bool) {
        let name = if self.context.is_empty() && key.is_empty() {
            key_string_iter(std::iter::once(""))
        } else {
            key_string_iter(
                self.context
                    .iter()
                    .map(|part| &**part)
                    .chain((!key.is_empty()).then_some(key)),
            )
        };
        self.key_info.insert(name, is_array);
    }

    fn add_implicit(&mut self, key: &[Part]) {
        self.implicits.insert(key_string(key));
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
