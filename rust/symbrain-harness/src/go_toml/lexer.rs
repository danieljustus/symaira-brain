//! Line-for-line port of `github.com/BurntSushi/toml` v1.6.0 `lex.go`.
//!
//! The Go lexer is a channel-fed state machine that the parser pulls lazily;
//! this port keeps that shape (an item queue drained before the next state
//! runs) because error ordering and line numbers depend on it.

use std::collections::VecDeque;

use crate::go_letters::is_letter;
use crate::go_quote::{decode_rune, quote_rune, quote_str};

const EOF: u32 = 0;
const RUNE_ERROR: u32 = 0xFFFD;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    Error,
    Eof,
    Text,
    String,
    StringEsc,
    RawString,
    MultilineString,
    RawMultilineString,
    Bool,
    Integer,
    Float,
    Datetime,
    Array,
    ArrayEnd,
    TableStart,
    TableEnd,
    ArrayTableStart,
    ArrayTableEnd,
    KeyStart,
    KeyEnd,
    CommentStart,
    InlineTableStart,
    InlineTableEnd,
}

#[derive(Debug)]
pub(super) struct Item {
    pub kind: Kind,
    pub val: String,
    pub line: i64,
    pub err: Option<String>,
}

#[derive(Clone, Copy, Debug)]
enum State {
    Top,
    TopEnd,
    TableStart,
    TableEnd,
    ArrayTableEnd,
    TableNameStart,
    TableNameEnd,
    BareName,
    QuotedName,
    KeyStart,
    KeyNameStart,
    KeyEnd,
    Value,
    ArrayValue,
    ArrayValueEnd,
    ArrayEnd,
    InlineTableValue,
    InlineTableValueEnd,
    InlineTableEnd,
    String,
    MultilineString,
    RawString,
    MultilineRawString,
    MultilineStringEscape,
    StringEscape,
    HexEscape,
    ShortUnicodeEscape,
    LongUnicodeEscape,
    NumberOrDateStart,
    NumberOrDate,
    Datetime,
    HexInteger,
    OctalInteger,
    BinaryInteger,
    DecimalNumber,
    DecimalNumberStart,
    BaseNumberOrDate,
    Float,
    Bool,
    CommentStart,
    Comment,
    Done,
}

pub(super) struct Lexer<'a> {
    input: &'a [u8],
    start: usize,
    pos: usize,
    line: i64,
    state: State,
    items: VecDeque<Item>,
    esc: bool,
    prev_widths: [usize; 4],
    nprev: usize,
    at_eof: bool,
    stack: Vec<State>,
    errored: bool,
}

fn is_whitespace(r: u32) -> bool {
    r == 0x09 || r == 0x20
}

fn is_nl(r: u32) -> bool {
    r == 0x0A || r == 0x0D
}

fn is_control(r: u32) -> bool {
    match r {
        0x09 | 0x0A | 0x0D => false,
        _ => r <= 0x1F || r == 0x7F,
    }
}

fn is_digit(r: u32) -> bool {
    (0x30..=0x39).contains(&r)
}

fn is_binary(r: u32) -> bool {
    r == 0x30 || r == 0x31
}

fn is_octal(r: u32) -> bool {
    (0x30..=0x37).contains(&r)
}

fn is_hex(r: u32) -> bool {
    is_digit(r) || ((r | 0x20) >= 0x61 && (r | 0x20) <= 0x66)
}

fn is_bare_key_char(r: u32) -> bool {
    (0x41..=0x5A).contains(&r)
        || (0x61..=0x7A).contains(&r)
        || is_digit(r)
        || r == u32::from(b'_')
        || r == u32::from(b'-')
}

fn c(byte: u8) -> u32 {
    u32::from(byte)
}

fn quote_u32(r: u32) -> String {
    quote_rune(char::from_u32(r).unwrap_or(char::REPLACEMENT_CHARACTER))
}

fn rune_or_eof(r: u32) -> String {
    if r == EOF {
        "end of file".to_owned()
    } else {
        format!(
            "'{}'",
            char::from_u32(r).unwrap_or(char::REPLACEMENT_CHARACTER)
        )
    }
}

fn letter(r: u32) -> bool {
    char::from_u32(r).is_some_and(is_letter)
}

impl<'a> Lexer<'a> {
    pub(super) fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            start: 0,
            pos: 0,
            line: 1,
            state: State::Top,
            items: VecDeque::new(),
            esc: false,
            prev_widths: [0; 4],
            nprev: 0,
            at_eof: false,
            stack: Vec::new(),
            errored: false,
        }
    }

    pub(super) fn next_item(&mut self) -> Item {
        loop {
            if let Some(item) = self.items.pop_front() {
                return item;
            }
            if matches!(self.state, State::Done) {
                return Item {
                    kind: Kind::Error,
                    val: String::new(),
                    line: self.line,
                    err: Some("BUG in lexer: no state to run".to_owned()),
                };
            }
            self.state = self.step(self.state);
        }
    }

    fn push_state(&mut self, state: State) {
        self.stack.push(state);
    }

    fn pop_state(&mut self) -> State {
        self.stack.pop().unwrap_or(State::Done)
    }

    fn current(&self) -> String {
        if self.start > self.pos {
            return String::new();
        }
        String::from_utf8_lossy(&self.input[self.start..self.pos]).into_owned()
    }

    fn current_bytes(&self) -> &[u8] {
        if self.start > self.pos {
            return &[];
        }
        &self.input[self.start..self.pos]
    }

    fn queue(&mut self, item: Item) {
        if !self.errored {
            if item.kind == Kind::Error {
                self.errored = true;
            }
            self.items.push_back(item);
        }
    }

    fn emit(&mut self, kind: Kind) {
        if self.start > self.pos {
            let byte = self.input.get(self.pos).copied().unwrap_or(0);
            self.error_text(format!("invalid UTF-8 byte: 0x{byte:02x}"));
            return;
        }
        let item = Item {
            kind,
            val: self.current(),
            line: self.line,
            err: None,
        };
        self.queue(item);
        self.start = self.pos;
    }

    fn emit_trim(&mut self, kind: Kind) {
        let item = Item {
            kind,
            val: self.current().trim().to_owned(),
            line: self.line,
            err: None,
        };
        self.queue(item);
        self.start = self.pos;
    }

    fn next(&mut self) -> u32 {
        if self.at_eof || self.pos >= self.input.len() {
            self.at_eof = true;
            return EOF;
        }
        if self.input[self.pos] == b'\n' {
            self.line += 1;
        }
        self.prev_widths[3] = self.prev_widths[2];
        self.prev_widths[2] = self.prev_widths[1];
        self.prev_widths[1] = self.prev_widths[0];
        if self.nprev < 4 {
            self.nprev += 1;
        }
        let (rune, width) = decode_rune(&self.input[self.pos..]);
        let r = u32::from(rune);
        if r == RUNE_ERROR && width == 1 {
            let byte = self.input[self.pos];
            self.error_text(format!("invalid UTF-8 byte: 0x{byte:02x}"));
            return RUNE_ERROR;
        }
        if is_control(r)
            || (r == 0x0D
                && (self.input.len() - 1 == self.pos || self.input[self.pos + 1] != b'\n'))
        {
            let line = self.line;
            self.queue(Item {
                kind: Kind::Error,
                val: String::new(),
                line,
                err: Some(format!(
                    "TOML files cannot contain control characters: '0x{r:02x}'"
                )),
            });
            return RUNE_ERROR;
        }
        self.prev_widths[0] = width;
        self.pos += width;
        r
    }

    fn ignore(&mut self) {
        self.start = self.pos;
    }

    fn backup(&mut self) {
        if self.at_eof {
            self.at_eof = false;
            return;
        }
        if self.nprev < 1 {
            return;
        }
        let width = self.prev_widths[0];
        self.prev_widths[0] = self.prev_widths[1];
        self.prev_widths[1] = self.prev_widths[2];
        self.prev_widths[2] = self.prev_widths[3];
        self.nprev -= 1;
        self.pos = self.pos.saturating_sub(width);
        if self.pos < self.input.len() && self.input[self.pos] == b'\n' {
            self.line -= 1;
        }
    }

    fn accept(&mut self, valid: u32) -> bool {
        if self.next() == valid {
            return true;
        }
        self.backup();
        false
    }

    fn peek(&mut self) -> u32 {
        let r = self.next();
        self.backup();
        r
    }

    fn skip(&mut self, pred: fn(u32) -> bool) {
        loop {
            let r = self.next();
            if pred(r) {
                continue;
            }
            self.backup();
            self.ignore();
            return;
        }
    }

    /// `lx.error(err)`: previous-line position when at EOF.
    fn error_text(&mut self, message: String) -> State {
        let line = if self.at_eof {
            self.line - 1
        } else {
            self.line
        };
        self.queue(Item {
            kind: Kind::Error,
            val: String::new(),
            line,
            err: Some(message),
        });
        State::Done
    }

    /// `lx.errorPrevLine(err)`.
    fn error_prev_line(&mut self, message: String) -> State {
        let line = self.line - 1;
        self.queue(Item {
            kind: Kind::Error,
            val: String::new(),
            line,
            err: Some(message),
        });
        State::Done
    }

    /// `lx.errorf(...)`.
    fn errorf(&mut self, message: String) -> State {
        let mut line = self.line;
        if self.at_eof && self.pos >= 1 && self.input[self.pos - 1] == b'\n' {
            line -= 1;
        }
        self.queue(Item {
            kind: Kind::Error,
            val: String::new(),
            line,
            err: Some(message),
        });
        State::Done
    }

    fn skip_to(&mut self, state: State) -> State {
        self.ignore();
        state
    }

    #[allow(clippy::too_many_lines)]
    fn step(&mut self, state: State) -> State {
        match state {
            State::Top => self.lex_top(),
            State::TopEnd => self.lex_top_end(),
            State::TableStart => self.lex_table_start(),
            State::TableEnd => {
                self.emit(Kind::TableEnd);
                State::TopEnd
            }
            State::ArrayTableEnd => {
                let r = self.next();
                if r != c(b']') {
                    return self.errorf(format!(
                        "expected end of table array name delimiter ']', but got {} instead",
                        quote_u32(r)
                    ));
                }
                self.emit(Kind::ArrayTableEnd);
                State::TopEnd
            }
            State::TableNameStart => self.lex_table_name_start(),
            State::TableNameEnd => self.lex_table_name_end(),
            State::BareName => {
                let r = self.next();
                if is_bare_key_char(r) {
                    return State::BareName;
                }
                self.backup();
                self.emit(Kind::Text);
                self.pop_state()
            }
            State::QuotedName => self.lex_quoted_name(),
            State::KeyStart => self.lex_key_start(),
            State::KeyNameStart => self.lex_key_name_start(),
            State::KeyEnd => self.lex_key_end(),
            State::Value => self.lex_value(),
            State::ArrayValue => self.lex_array_value(),
            State::ArrayValueEnd => self.lex_array_value_end(),
            State::ArrayEnd => {
                self.ignore();
                self.emit(Kind::ArrayEnd);
                self.pop_state()
            }
            State::InlineTableValue => self.lex_inline_table_value(),
            State::InlineTableValueEnd => self.lex_inline_table_value_end(),
            State::InlineTableEnd => {
                self.ignore();
                self.emit(Kind::InlineTableEnd);
                self.pop_state()
            }
            State::String => self.lex_string(),
            State::MultilineString => self.lex_multiline_string(),
            State::RawString => self.lex_raw_string(),
            State::MultilineRawString => self.lex_multiline_raw_string(),
            State::MultilineStringEscape => {
                if is_nl(self.next()) {
                    return State::MultilineString;
                }
                self.backup();
                self.push_state(State::MultilineString);
                self.lex_string_escape()
            }
            State::StringEscape => self.lex_string_escape(),
            State::HexEscape => self.lex_fixed_hex(2, "two", "\\x"),
            State::ShortUnicodeEscape => self.lex_fixed_hex(4, "four", "\\u"),
            State::LongUnicodeEscape => self.lex_fixed_hex(8, "eight", "\\U"),
            State::NumberOrDateStart => {
                if self.next() == c(b'0') {
                    State::BaseNumberOrDate
                } else {
                    State::NumberOrDate
                }
            }
            State::NumberOrDate => self.lex_number_or_date(),
            State::Datetime => self.lex_datetime(),
            State::HexInteger => self.lex_radix_integer(is_hex, State::HexInteger),
            State::OctalInteger => self.lex_radix_integer(is_octal, State::OctalInteger),
            State::BinaryInteger => self.lex_radix_integer(is_binary, State::BinaryInteger),
            State::DecimalNumber => self.lex_decimal_number(),
            State::DecimalNumberStart => self.lex_decimal_number_start(),
            State::BaseNumberOrDate => self.lex_base_number_or_date(),
            State::Float => self.lex_float(),
            State::Bool => self.lex_bool(),
            State::CommentStart => {
                self.ignore();
                self.emit(Kind::CommentStart);
                State::Comment
            }
            State::Comment => {
                let r = self.next();
                if is_nl(r) || r == EOF {
                    self.backup();
                    self.emit(Kind::Text);
                    return self.pop_state();
                }
                State::Comment
            }
            State::Done => State::Done,
        }
    }

    fn lex_top(&mut self) -> State {
        let r = self.next();
        if is_whitespace(r) || is_nl(r) {
            return self.skip_to(State::Top);
        }
        if r == c(b'#') {
            self.push_state(State::Top);
            return State::CommentStart;
        }
        if r == c(b'[') {
            return State::TableStart;
        }
        if r == EOF {
            if self.pos > self.start {
                return self.errorf("unexpected EOF".to_owned());
            }
            self.emit(Kind::Eof);
            return State::Done;
        }
        self.backup();
        self.push_state(State::TopEnd);
        State::KeyStart
    }

    fn lex_top_end(&mut self) -> State {
        let r = self.next();
        if r == c(b'#') {
            self.push_state(State::Top);
            return State::CommentStart;
        }
        if is_whitespace(r) {
            return State::TopEnd;
        }
        if is_nl(r) {
            self.ignore();
            return State::Top;
        }
        if r == EOF {
            self.emit(Kind::Eof);
            return State::Done;
        }
        self.errorf(format!(
            "expected a top-level item to end with a newline, comment, or EOF, but got {} instead",
            quote_u32(r)
        ))
    }

    fn lex_table_start(&mut self) -> State {
        if self.peek() == c(b'[') {
            self.next();
            self.emit(Kind::ArrayTableStart);
            self.push_state(State::ArrayTableEnd);
        } else {
            self.emit(Kind::TableStart);
            self.push_state(State::TableEnd);
        }
        State::TableNameStart
    }

    fn lex_table_name_start(&mut self) -> State {
        self.skip(is_whitespace);
        let r = self.peek();
        if r == c(b']') || r == EOF {
            return self
                .errorf("unexpected end of table name (table names cannot be empty)".to_owned());
        }
        if r == c(b'.') {
            return self
                .errorf("unexpected table separator (table names cannot be empty)".to_owned());
        }
        if r == c(b'"') || r == c(b'\'') {
            self.ignore();
            self.push_state(State::TableNameEnd);
            return State::QuotedName;
        }
        self.push_state(State::TableNameEnd);
        State::BareName
    }

    fn lex_table_name_end(&mut self) -> State {
        self.skip(is_whitespace);
        let r = self.next();
        if r == c(b'.') {
            self.ignore();
            return State::TableNameStart;
        }
        if r == c(b']') {
            return self.pop_state();
        }
        self.errorf(format!(
            "expected '.' or ']' to end table name, but got {} instead",
            quote_u32(r)
        ))
    }

    fn lex_quoted_name(&mut self) -> State {
        let r = self.next();
        if r == c(b'"') {
            self.ignore();
            State::String
        } else if r == c(b'\'') {
            self.ignore();
            State::RawString
        } else if is_whitespace(r) {
            self.skip_to(State::Value)
        } else if r == EOF {
            self.errorf("unexpected EOF; expected value".to_owned())
        } else {
            self.errorf(format!("expected value but found {} instead", quote_u32(r)))
        }
    }

    fn lex_key_start(&mut self) -> State {
        self.skip(is_whitespace);
        let r = self.peek();
        if r == c(b'=') || r == EOF {
            return self.errorf("unexpected '=': key name appears blank".to_owned());
        }
        if r == c(b'.') {
            return self.errorf("unexpected '.': keys cannot start with a '.'".to_owned());
        }
        if r == c(b'"') || r == c(b'\'') {
            self.ignore();
        }
        self.emit(Kind::KeyStart);
        State::KeyNameStart
    }

    fn lex_key_name_start(&mut self) -> State {
        self.skip(is_whitespace);
        let r = self.peek();
        if r == c(b'"') || r == c(b'\'') {
            self.ignore();
            self.push_state(State::KeyEnd);
            State::QuotedName
        } else if r == c(b'=') || r == EOF {
            self.errorf("unexpected '='".to_owned())
        } else if r == c(b'.') {
            self.errorf("unexpected '.'".to_owned())
        } else {
            self.push_state(State::KeyEnd);
            State::BareName
        }
    }

    fn lex_key_end(&mut self) -> State {
        self.skip(is_whitespace);
        let r = self.next();
        if is_whitespace(r) {
            self.skip_to(State::KeyEnd)
        } else if r == EOF {
            self.errorf("unexpected EOF; expected key separator '='".to_owned())
        } else if r == c(b'.') {
            self.ignore();
            State::KeyNameStart
        } else if r == c(b'=') {
            self.emit(Kind::KeyEnd);
            self.skip_to(State::Value)
        } else if r == c(b'\n') {
            self.error_prev_line(format!(
                "expected '.' or '=', but got {} instead",
                quote_u32(r)
            ))
        } else {
            self.errorf(format!(
                "expected '.' or '=', but got {} instead",
                quote_u32(r)
            ))
        }
    }

    fn lex_value(&mut self) -> State {
        let r = self.next();
        if is_whitespace(r) {
            return self.skip_to(State::Value);
        }
        if is_digit(r) {
            self.backup();
            return State::NumberOrDateStart;
        }
        match u8::try_from(r).unwrap_or(0xFF) {
            b'[' => {
                self.ignore();
                self.emit(Kind::Array);
                return State::ArrayValue;
            }
            b'{' => {
                self.ignore();
                self.emit(Kind::InlineTableStart);
                return State::InlineTableValue;
            }
            b'"' => {
                if self.accept(c(b'"')) {
                    if self.accept(c(b'"')) {
                        self.ignore();
                        return State::MultilineString;
                    }
                    self.backup();
                }
                self.ignore();
                return State::String;
            }
            b'\'' => {
                if self.accept(c(b'\'')) {
                    if self.accept(c(b'\'')) {
                        self.ignore();
                        return State::MultilineRawString;
                    }
                    self.backup();
                }
                self.ignore();
                return State::RawString;
            }
            b'.' => return self.errorf("floats must start with a digit, not '.'".to_owned()),
            b'i' | b'n' => {
                if (self.accept(c(b'n')) && self.accept(c(b'f')))
                    || (self.accept(c(b'a')) && self.accept(c(b'n')))
                {
                    self.emit(Kind::Float);
                    return self.pop_state();
                }
            }
            b'-' | b'+' => return State::DecimalNumberStart,
            _ => {}
        }
        if letter(r) {
            self.backup();
            return State::Bool;
        }
        if r == EOF {
            return self.errorf("unexpected EOF; expected value".to_owned());
        }
        if r == c(b'\n') {
            return self
                .error_prev_line(format!("expected value but found {} instead", quote_u32(r)));
        }
        self.errorf(format!("expected value but found {} instead", quote_u32(r)))
    }

    fn lex_array_value(&mut self) -> State {
        let r = self.next();
        if is_whitespace(r) || is_nl(r) {
            return self.skip_to(State::ArrayValue);
        }
        if r == c(b'#') {
            self.push_state(State::ArrayValue);
            return State::CommentStart;
        }
        if r == c(b',') {
            return self.errorf("unexpected comma".to_owned());
        }
        if r == c(b']') {
            return State::ArrayEnd;
        }
        self.backup();
        self.push_state(State::ArrayValueEnd);
        State::Value
    }

    fn lex_array_value_end(&mut self) -> State {
        let r = self.next();
        if is_whitespace(r) || is_nl(r) {
            self.skip_to(State::ArrayValueEnd)
        } else if r == c(b'#') {
            self.push_state(State::ArrayValueEnd);
            State::CommentStart
        } else if r == c(b',') {
            self.ignore();
            State::ArrayValue
        } else if r == c(b']') {
            State::ArrayEnd
        } else {
            self.errorf(format!(
                "expected a comma (',') or array terminator (']'), but got {}",
                rune_or_eof(r)
            ))
        }
    }

    fn lex_inline_table_value(&mut self) -> State {
        let r = self.next();
        if is_whitespace(r) || is_nl(r) {
            return self.skip_to(State::InlineTableValue);
        }
        if r == c(b'#') {
            self.push_state(State::InlineTableValue);
            return State::CommentStart;
        }
        if r == c(b',') {
            return self.errorf("unexpected comma".to_owned());
        }
        if r == c(b'}') {
            return State::InlineTableEnd;
        }
        self.backup();
        self.push_state(State::InlineTableValueEnd);
        State::KeyStart
    }

    fn lex_inline_table_value_end(&mut self) -> State {
        let r = self.next();
        if is_whitespace(r) || is_nl(r) {
            self.skip_to(State::InlineTableValueEnd)
        } else if r == c(b'#') {
            self.push_state(State::InlineTableValueEnd);
            State::CommentStart
        } else if r == c(b',') {
            self.ignore();
            self.skip(is_whitespace);
            if self.peek() == c(b'}') {
                return State::InlineTableValueEnd;
            }
            State::InlineTableValue
        } else if r == c(b'}') {
            State::InlineTableEnd
        } else {
            self.errorf(format!(
                "expected a comma or an inline table terminator '}}', but got {} instead",
                rune_or_eof(r)
            ))
        }
    }

    fn lex_string(&mut self) -> State {
        let r = self.next();
        if r == EOF {
            return self.errorf("unexpected EOF; expected '\"'".to_owned());
        }
        if is_nl(r) {
            return self.error_prev_line("strings cannot contain newlines".to_owned());
        }
        if r == c(b'\\') {
            self.push_state(State::String);
            return State::StringEscape;
        }
        if r == c(b'"') {
            self.backup();
            if self.esc {
                self.esc = false;
                self.emit(Kind::StringEsc);
            } else {
                self.emit(Kind::String);
            }
            self.next();
            self.ignore();
            return self.pop_state();
        }
        State::String
    }

    fn lex_multiline_string(&mut self) -> State {
        let r = self.next();
        if r == EOF {
            return self.errorf("unexpected EOF; expected '\"\"\"'".to_owned());
        }
        if r == c(b'\\') {
            return State::MultilineStringEscape;
        }
        if r != c(b'"') {
            return State::MultilineString;
        }
        if self.accept(c(b'"')) {
            if self.accept(c(b'"')) {
                if self.peek() == c(b'"') {
                    let current = self.current_bytes();
                    if current.ends_with(b"\"\"\"\"\"") && !current.ends_with(b"\\\"\"\"\"\"") {
                        return self.errorf("unexpected '\"\"\"\"\"\"'".to_owned());
                    }
                    self.backup();
                    self.backup();
                    return State::MultilineString;
                }
                self.backup();
                self.backup();
                self.backup();
                self.esc = false;
                self.emit(Kind::MultilineString);
                self.next();
                self.next();
                self.next();
                self.ignore();
                return self.pop_state();
            }
            self.backup();
        }
        State::MultilineString
    }

    fn lex_raw_string(&mut self) -> State {
        let r = self.next();
        if r == EOF {
            return self.errorf("unexpected EOF; expected \"'\"".to_owned());
        }
        if is_nl(r) {
            return self.error_prev_line("strings cannot contain newlines".to_owned());
        }
        if r == c(b'\'') {
            self.backup();
            self.emit(Kind::RawString);
            self.next();
            self.ignore();
            return self.pop_state();
        }
        State::RawString
    }

    fn lex_multiline_raw_string(&mut self) -> State {
        let r = self.next();
        if r == EOF {
            return self.errorf("unexpected EOF; expected \"'''\"".to_owned());
        }
        if r != c(b'\'') {
            return State::MultilineRawString;
        }
        if self.accept(c(b'\'')) {
            if self.accept(c(b'\'')) {
                if self.peek() == c(b'\'') {
                    if self.current_bytes().ends_with(b"'''''") {
                        return self.errorf("unexpected \"''''''\"".to_owned());
                    }
                    self.backup();
                    self.backup();
                    return State::MultilineRawString;
                }
                self.backup();
                self.backup();
                self.backup();
                self.emit(Kind::RawMultilineString);
                self.next();
                self.next();
                self.next();
                self.ignore();
                return self.pop_state();
            }
            self.backup();
        }
        State::MultilineRawString
    }

    fn lex_string_escape(&mut self) -> State {
        self.esc = true;
        let r = self.next();
        match u8::try_from(r).unwrap_or(0xFF) {
            b'e' | b'b' | b't' | b'n' | b'f' | b'r' | b'"' | b' ' | b'\t' | b'\\' => {
                self.pop_state()
            }
            b'x' => State::HexEscape,
            b'u' => State::ShortUnicodeEscape,
            b'U' => State::LongUnicodeEscape,
            _ => {
                // errLexEscape: `invalid escape in string '\%c'`.
                let shown = char::from_u32(r).unwrap_or(char::REPLACEMENT_CHARACTER);
                let message = format!("invalid escape in string '\\{shown}'");
                self.error_text(message)
            }
        }
    }

    fn lex_fixed_hex(&mut self, digits: usize, word: &str, escape: &str) -> State {
        for _ in 0..digits {
            let r = self.next();
            if !is_hex(r) {
                let current = quote_str(&self.current());
                return self.errorf(format!(
                    "expected {word} hexadecimal digits after '{escape}', but got {current} instead"
                ));
            }
        }
        self.pop_state()
    }

    fn lex_number_or_date(&mut self) -> State {
        let r = self.next();
        if is_digit(r) {
            return State::NumberOrDate;
        }
        match u8::try_from(r).unwrap_or(0xFF) {
            b'-' | b':' => return State::Datetime,
            b'_' => return State::DecimalNumber,
            b'.' | b'e' | b'E' => return State::Float,
            _ => {}
        }
        self.backup();
        self.emit(Kind::Integer);
        self.pop_state()
    }

    fn lex_datetime(&mut self) -> State {
        let r = self.next();
        if is_digit(r) {
            return State::Datetime;
        }
        if matches!(
            u8::try_from(r).unwrap_or(0xFF),
            b'-' | b':' | b'T' | b't' | b' ' | b'.' | b'Z' | b'z' | b'+'
        ) {
            return State::Datetime;
        }
        self.backup();
        self.emit_trim(Kind::Datetime);
        self.pop_state()
    }

    fn lex_radix_integer(&mut self, pred: fn(u32) -> bool, again: State) -> State {
        let r = self.next();
        if pred(r) || r == c(b'_') {
            return again;
        }
        self.backup();
        self.emit(Kind::Integer);
        self.pop_state()
    }

    fn lex_decimal_number(&mut self) -> State {
        let r = self.next();
        if is_digit(r) || r == c(b'_') {
            return State::DecimalNumber;
        }
        if r == c(b'.') || r == c(b'e') || r == c(b'E') {
            return State::Float;
        }
        self.backup();
        self.emit(Kind::Integer);
        self.pop_state()
    }

    fn lex_decimal_number_start(&mut self) -> State {
        let r = self.next();
        match u8::try_from(r).unwrap_or(0xFF) {
            b'i' => {
                if !self.accept(c(b'n')) || !self.accept(c(b'f')) {
                    return self.errorf(format!("invalid float: '{}'", self.current()));
                }
                self.emit(Kind::Float);
                return self.pop_state();
            }
            b'n' => {
                if !self.accept(c(b'a')) || !self.accept(c(b'n')) {
                    return self.errorf(format!("invalid float: '{}'", self.current()));
                }
                self.emit(Kind::Float);
                return self.pop_state();
            }
            b'0' => {
                let p = self.peek();
                if matches!(u8::try_from(p).unwrap_or(0xFF), b'b' | b'o' | b'x') {
                    let shown = char::from_u32(p).unwrap_or(char::REPLACEMENT_CHARACTER);
                    return self.errorf(format!(
                        "cannot use sign with non-decimal numbers: '{}{shown}'",
                        self.current()
                    ));
                }
            }
            b'.' => return self.errorf("floats must start with a digit, not '.'".to_owned()),
            _ => {}
        }
        if is_digit(r) {
            return State::DecimalNumber;
        }
        self.errorf(format!("expected a digit but got {}", quote_u32(r)))
    }

    fn lex_base_number_or_date(&mut self) -> State {
        let r = self.next();
        if is_digit(r) {
            return State::NumberOrDate;
        }
        match u8::try_from(r).unwrap_or(0xFF) {
            b'_' => return State::DecimalNumber,
            b'.' | b'e' | b'E' => return State::Float,
            b'b' => {
                let p = self.peek();
                if !is_binary(p) {
                    let _ = self.errorf(format!(
                        "not a binary number: '{}{}'",
                        self.current(),
                        rune_text(p)
                    ));
                }
                return State::BinaryInteger;
            }
            b'o' => {
                let p = self.peek();
                if !is_octal(p) {
                    let _ = self.errorf(format!(
                        "not an octal number: '{}{}'",
                        self.current(),
                        rune_text(p)
                    ));
                }
                return State::OctalInteger;
            }
            b'x' => {
                let p = self.peek();
                if !is_hex(p) {
                    let _ = self.errorf(format!(
                        "not a hexadecimal number: '{}{}'",
                        self.current(),
                        rune_text(p)
                    ));
                }
                return State::HexInteger;
            }
            _ => {}
        }
        self.backup();
        self.emit(Kind::Integer);
        self.pop_state()
    }

    fn lex_float(&mut self) -> State {
        let r = self.next();
        if is_digit(r)
            || matches!(
                u8::try_from(r).unwrap_or(0xFF),
                b'_' | b'.' | b'-' | b'+' | b'e' | b'E'
            )
        {
            return State::Float;
        }
        self.backup();
        self.emit(Kind::Float);
        self.pop_state()
    }

    fn lex_bool(&mut self) -> State {
        let mut collected = String::new();
        loop {
            let r = self.next();
            if !letter(r) {
                self.backup();
                break;
            }
            collected.push(char::from_u32(r).unwrap_or(char::REPLACEMENT_CHARACTER));
        }
        if collected == "true" || collected == "false" {
            self.emit(Kind::Bool);
            return self.pop_state();
        }
        self.errorf(format!(
            "expected value but found {} instead",
            quote_str(&collected)
        ))
    }
}

/// `%c` of a rune (`eof` prints a NUL byte).
fn rune_text(r: u32) -> String {
    char::from_u32(r)
        .unwrap_or(char::REPLACEMENT_CHARACTER)
        .to_string()
}
