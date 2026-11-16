#![deny(unsafe_code)]

//! Scalar style selection of `gopkg.in/yaml.v3` for strings (plain,
//! single-quoted, double-quoted and literal block).

pub(crate) enum Encoded {
    Inline(String),
    /// Literal block: the header (`|`, `|-`, `|4+` ...); the body comes from
    /// [`literal_text`].
    Literal {
        header: String,
    },
}

/// yaml.v3 `is_printable`: line feed, ASCII, and U+00A0..U+FFFD without the
/// byte-order mark and the two non-characters. NEL and everything above the
/// basic plane (emoji) are *not* printable there, so they are escaped.
fn is_printable(c: char) -> bool {
    matches!(c,
        '\u{0a}' | '\u{20}'..='\u{7e}' | '\u{a0}'..='\u{d7ff}'
        | '\u{e000}'..='\u{fefe}' | '\u{ff00}'..='\u{fffd}')
}

fn is_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

struct Analysis {
    multiline: bool,
    block_plain_allowed: bool,
    single_quoted_allowed: bool,
    block_allowed: bool,
}

fn analyze(value: &str) -> Analysis {
    let chars: Vec<char> = value.chars().collect();
    let mut block_indicators = false;
    let mut flow_indicators = false;
    let mut line_breaks = false;
    let mut special = false;
    let mut has_tab = false;
    let mut leading_space = false;
    let mut leading_break = false;
    let mut trailing_space = false;
    let mut trailing_break = false;
    let mut break_space = false;
    let mut space_break = false;
    let mut preceded_by_whitespace = true;
    let mut previous_space = false;
    let mut previous_break = false;
    // yaml.v3: `is_blank` (space, tab) for look-ahead, `is_blankz` (blank,
    // break, NUL) for look-behind.
    let is_blank = |c: char| matches!(c, ' ' | '\t');
    let is_blankz = |c: char| is_blank(c) || is_break(c) || c == '\0';
    if value.starts_with("---") || value.starts_with("...") {
        block_indicators = true;
        flow_indicators = true;
    }
    for (i, &c) in chars.iter().enumerate() {
        let followed_by_whitespace = chars.get(i + 1).is_none_or(|n| is_blank(*n));
        if i == 0 {
            match c {
                '#' | ',' | '[' | ']' | '{' | '}' | '&' | '*' | '!' | '|' | '>' | '\'' | '"'
                | '%' | '@' | '`' => {
                    flow_indicators = true;
                    block_indicators = true;
                }
                '?' | ':' => {
                    flow_indicators = true;
                    if followed_by_whitespace {
                        block_indicators = true;
                    }
                }
                '-' if followed_by_whitespace => {
                    flow_indicators = true;
                    block_indicators = true;
                }
                _ => {}
            }
        } else {
            match c {
                ',' | '?' | '[' | ']' | '{' | '}' => flow_indicators = true,
                ':' => {
                    flow_indicators = true;
                    if followed_by_whitespace {
                        block_indicators = true;
                    }
                }
                '#' if preceded_by_whitespace => {
                    flow_indicators = true;
                    block_indicators = true;
                }
                _ => {}
            }
        }
        if c == '\t' {
            // yaml.v3 keeps a tab out of plain and single-quoted scalars but a
            // literal block may carry it (recorded Go behaviour).
            has_tab = true;
        } else if !is_printable(c) {
            special = true;
        }
        if c == ' ' {
            if i == 0 {
                leading_space = true;
            }
            if i + 1 == chars.len() {
                trailing_space = true;
            }
            if previous_break {
                break_space = true;
            }
            previous_space = true;
            previous_break = false;
        } else if is_break(c) {
            line_breaks = true;
            if i == 0 {
                leading_break = true;
            }
            if i + 1 == chars.len() {
                trailing_break = true;
            }
            if previous_space {
                space_break = true;
            }
            previous_space = false;
            previous_break = true;
        } else {
            previous_space = false;
            previous_break = false;
        }
        preceded_by_whitespace = is_blankz(c);
    }
    let mut flow_plain_allowed = true;
    let mut block_plain_allowed = true;
    let mut single_quoted_allowed = true;
    let mut block_allowed = true;
    if leading_space || leading_break || trailing_space || trailing_break {
        flow_plain_allowed = false;
        block_plain_allowed = false;
    }
    if trailing_space {
        block_allowed = false;
    }
    if break_space {
        flow_plain_allowed = false;
        block_plain_allowed = false;
        single_quoted_allowed = false;
    }
    if space_break || special {
        flow_plain_allowed = false;
        block_plain_allowed = false;
        single_quoted_allowed = false;
        block_allowed = false;
    }
    if has_tab {
        flow_plain_allowed = false;
        block_plain_allowed = false;
        single_quoted_allowed = false;
    }
    if line_breaks {
        flow_plain_allowed = false;
        block_plain_allowed = false;
    }
    if flow_indicators {
        flow_plain_allowed = false;
    }
    if block_indicators {
        block_plain_allowed = false;
    }
    let _ = flow_plain_allowed;
    Analysis {
        multiline: line_breaks,
        block_plain_allowed,
        single_quoted_allowed,
        block_allowed,
    }
}

fn is_int(value: &str) -> bool {
    let body = value.strip_prefix(['+', '-']).unwrap_or(value);
    let cleaned = body.replace('_', "");
    if cleaned.is_empty() {
        return false;
    }
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(rest) = cleaned.strip_prefix(prefix) {
            return !rest.is_empty() && rest.chars().all(|c| c.is_digit(radix));
        }
    }
    cleaned.chars().all(|c| c.is_ascii_digit())
}

fn is_float(value: &str) -> bool {
    let body = value.strip_prefix(['+', '-']).unwrap_or(value);
    let lower = body.to_ascii_lowercase();
    if matches!(lower.as_str(), ".inf" | ".nan") {
        return true;
    }
    let cleaned = body.replace('_', "");
    if cleaned.is_empty() || cleaned == "." {
        return false;
    }
    let mut seen_digit = false;
    let mut chars = cleaned.chars().peekable();
    let mut seen_dot = false;
    let mut seen_exp = false;
    while let Some(c) = chars.next() {
        match c {
            '0'..='9' => seen_digit = true,
            '.' if !seen_dot && !seen_exp => seen_dot = true,
            'e' | 'E' if seen_digit && !seen_exp => {
                seen_exp = true;
                if matches!(chars.peek(), Some('+' | '-')) {
                    chars.next();
                }
                if chars.peek().is_none() {
                    return false;
                }
            }
            _ => return false,
        }
    }
    seen_digit
}

fn is_timestamp(value: &str) -> bool {
    // yaml.v3 accepts `2006-1-2` optionally followed by `T`/`t`/space and a
    // clock part (`15:4:5[.frac]` with an optional zone).
    let (date, rest) = match value.split_once(['T', 't', ' ']) {
        Some((date, rest)) => (date, Some(rest)),
        None => (value, None),
    };
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3
        || parts[0].len() != 4
        || parts[1].is_empty()
        || parts[1].len() > 2
        || parts[2].is_empty()
        || parts[2].len() > 2
        || !parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    let Some(time) = rest else { return true };
    let time = time.trim_end_matches('Z');
    let clock = time.split(['+', '-']).next().unwrap_or("");
    let fields: Vec<&str> = clock.split(':').collect();
    fields.len() == 3
        && fields
            .iter()
            .all(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

fn resolves_to_string(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    if matches!(
        value,
        "~" | "null"
            | "Null"
            | "NULL"
            | "true"
            | "True"
            | "TRUE"
            | "false"
            | "False"
            | "FALSE"
            | "y"
            | "Y"
            | "yes"
            | "Yes"
            | "YES"
            | "n"
            | "N"
            | "no"
            | "No"
            | "NO"
            | "on"
            | "On"
            | "ON"
            | "off"
            | "Off"
            | "OFF"
            | "<<"
            | "="
    ) {
        return false;
    }
    !(is_int(value) || is_float(value) || is_timestamp(value) || is_base60_float(value))
}

fn is_base60_float(value: &str) -> bool {
    let body = value.strip_prefix(['+', '-']).unwrap_or(value);
    body.contains(':')
        && body.split(':').count() >= 2
        && body
            .split(':')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit() || c == '.'))
        && body.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn double_quote(value: &str) -> String {
    // yaml.v3 `is_bom(b, i)` tests `b[0]` instead of `b[i]`: a value that
    // starts with a byte-order mark has every character escaped.
    let bom_prefix = value.starts_with('\u{feff}');
    let mut out = String::from("\"");
    for c in value.chars() {
        if !(bom_prefix || !is_printable(c) || is_break(c) || c == '"' || c == '\\') {
            out.push(c);
            continue;
        }
        out.push('\\');
        match c {
            '\0' => out.push('0'),
            '\u{7}' => out.push('a'),
            '\u{8}' => out.push('b'),
            '\t' => out.push('t'),
            '\n' => out.push('n'),
            '\u{b}' => out.push('v'),
            '\u{c}' => out.push('f'),
            '\r' => out.push('r'),
            '\u{1b}' => out.push('e'),
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '\u{85}' => out.push('N'),
            '\u{a0}' => out.push('_'),
            '\u{2028}' => out.push('L'),
            '\u{2029}' => out.push('P'),
            c => {
                let code = c as u32;
                if code <= 0xff {
                    out.push_str(&format!("x{code:02X}"));
                } else if code <= 0xffff {
                    out.push_str(&format!("u{code:04X}"));
                } else {
                    out.push_str(&format!("U{code:08X}"));
                }
            }
        }
    }
    out.push('"');
    out
}

/// Chooses the yaml.v3 style for `value`. `key` selects the simple-key
/// context (no literal blocks).
pub(crate) fn encode(value: &str, key: bool) -> Encoded {
    let analysis = analyze(value);
    if value.contains('\n') {
        if analysis.block_allowed && !key {
            return Encoded::Literal {
                header: literal_header(value),
            };
        }
        return Encoded::Inline(double_quote(value));
    }
    if key && analysis.multiline {
        return Encoded::Inline(double_quote(value));
    }
    let plain_ok =
        resolves_to_string(value) && analysis.block_plain_allowed && !(key && analysis.multiline);
    if plain_ok {
        return Encoded::Inline(value.to_owned());
    }
    if !resolves_to_string(value) {
        return Encoded::Inline(double_quote(value));
    }
    if analysis.single_quoted_allowed {
        return Encoded::Inline(format!("'{}'", value.replace('\'', "''")));
    }
    Encoded::Inline(double_quote(value))
}

/// yaml.v3 `write_block_scalar_hints`: an indentation hint when the value
/// starts with a space or break, and a chomping hint (`-` strip, `+` keep).
fn literal_header(value: &str) -> String {
    let mut header = String::from("|");
    if value.starts_with(' ') || value.chars().next().is_some_and(is_break) {
        header.push('4');
    }
    let mut tail = value.chars().rev();
    match tail.next() {
        None => header.push('-'),
        Some(last) if !is_break(last) => header.push('-'),
        Some(_) => match tail.next() {
            None => header.push('+'),
            Some(previous) if is_break(previous) => header.push('+'),
            Some(_) => {}
        },
    }
    header
}

/// The text of a literal block scalar after its key: `header`, then the value
/// verbatim with every non-empty line indented by `pad`, and a final line
/// break unless the value already ends in one (the emitter's next
/// `write_indent` supplies it).
pub(crate) fn literal_text(header: &str, value: &str, pad: &str) -> String {
    let mut out = String::from(header);
    let mut at_line_start = false;
    let mut breaks = true;
    for c in value.chars() {
        if is_break(c) {
            out.push(c);
            at_line_start = true;
            breaks = true;
        } else {
            if breaks {
                if !at_line_start {
                    out.push('\n');
                }
                out.push_str(pad);
            }
            out.push(c);
            at_line_start = false;
            breaks = false;
        }
    }
    if !at_line_start {
        out.push('\n');
    }
    out
}

/// yaml.v3 writes U+2028/U+2029 in a single-quoted scalar as real line breaks:
/// the break is copied and the next text starts on the scalar's indent.
pub(crate) fn single_quoted_breaks(quoted: &str, pad: &str) -> String {
    let last = quoted.chars().count().saturating_sub(1);
    let mut out = String::with_capacity(quoted.len());
    let mut breaks = false;
    for (index, c) in quoted.chars().enumerate() {
        if matches!(c, '\u{2028}' | '\u{2029}') {
            out.push(c);
            breaks = true;
            continue;
        }
        // The closing quote is an indicator, not text: it gets no indent.
        if breaks && index != last {
            out.push_str(pad);
        }
        breaks = false;
        out.push(c);
    }
    out
}

pub fn inline(value: &str, key: bool) -> String {
    match encode(value, key) {
        Encoded::Inline(text) => text,
        Encoded::Literal { .. } => double_quote(value),
    }
}
