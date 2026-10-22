//! Diagnostic ownership only: never constructs or admits a Request.
//! Inspect direct routing fields over the entire physical frame. Malformed
//! opaque values may be skipped only while their lexical boundaries remain
//! unambiguous; broken routing tokens/structure fall back to the old parser.
use super::{SKILLS_TOOL_NAMES, fold_key, strings, syntax};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Owner {
    KnownSkills,
    Other,
    Indeterminate,
}

#[derive(Default)]
struct Route {
    jsonrpc: Option<String>,
    method: Option<String>,
    name: Option<String>,
}

pub(super) fn diagnostic_owner(raw: &[u8]) -> Owner {
    if raw.len() > crate::frame::MAX_MESSAGE_BYTES {
        return Owner::Indeterminate;
    }
    let mut probe = Probe { raw, index: 0 };
    let mut route = Route::default();
    probe.space();
    if probe.object(&mut route, false).is_err() {
        return Owner::Indeterminate;
    }
    probe.space();
    if probe.index != raw.len() {
        // Do not classify a second envelope or ambiguous trailing structure.
        return Owner::Indeterminate;
    }
    match (
        route.jsonrpc.as_deref(),
        route.method.as_deref(),
        route.name.as_deref(),
    ) {
        (Some(version), Some(method), Some(name))
            if version == crate::JSONRPC_VERSION
                && method == "tools/call"
                && SKILLS_TOOL_NAMES.contains(&name) =>
        {
            Owner::KnownSkills
        }
        (Some(version), Some(method), _)
            if version != crate::JSONRPC_VERSION || method != "tools/call" =>
        {
            Owner::Other
        }
        (_, _, Some(_)) if route.jsonrpc.is_some() && route.method.is_some() => Owner::Other,
        _ => Owner::Indeterminate,
    }
}

struct Probe<'a> {
    raw: &'a [u8],
    index: usize,
}

impl Probe<'_> {
    fn space(&mut self) {
        while matches!(self.raw.get(self.index), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.index += 1;
        }
    }

    // Recursion is limited to the envelope and its direct params object.
    // All argument/metadata containers are skipped iteratively below.
    fn object(&mut self, route: &mut Route, params: bool) -> Result<(), ()> {
        if self.raw.get(self.index) != Some(&b'{') {
            return Err(());
        }
        self.index += 1;
        loop {
            self.space();
            match self.raw.get(self.index) {
                None => return Ok(()),
                Some(b'}') => {
                    self.index += 1;
                    return Ok(());
                }
                Some(b'"') => {}
                _ => return Err(()),
            }
            let start = self.index;
            if !self.string_end() {
                // An unfinished key may be a routing field. No completion guessed.
                return Err(());
            }
            let key = fold_key(&decode_string(&self.raw[start..self.index])?);
            self.space();
            if self.raw.get(self.index) != Some(&b':') {
                return Err(());
            }
            self.index += 1;
            self.space();
            let start = self.index;
            if !params && key == "params" {
                // RawMessage assignment replaces params, unlike string nulls.
                route.name = None;
                if self.raw.get(self.index) == Some(&b'{') {
                    self.object(route, true)?;
                } else {
                    self.skip_value()?;
                }
            } else {
                let complete = self.skip_value()?;
                let field = match (params, key.as_str()) {
                    (false, "jsonrpc") => Some(&mut route.jsonrpc),
                    (false, "method") => Some(&mut route.method),
                    (true, "name") => Some(&mut route.name),
                    _ => None,
                };
                if let Some(field) = field {
                    let raw = &self.raw[start..self.index];
                    if !complete || syntax::validate(raw).is_err() {
                        return Err(());
                    }
                    if raw.first() == Some(&b'"') {
                        *field = Some(decode_string(raw)?);
                    }
                    // Go leaves strings unchanged on null and typed failures.
                    // Syntax was checked above; no incomplete owner is ignored.
                }
            }
            self.space();
            match self.raw.get(self.index) {
                Some(b',') => self.index += 1,
                Some(b'}') => {
                    self.index += 1;
                    return Ok(());
                }
                None => return Ok(()),
                _ => return Err(()),
            }
        }
    }

    // Quotes/backslash pairs establish lexical boundaries even for invalid
    // escapes. The real syntax scanner alone chooses the error text. A routing
    // string must additionally pass decode_string, whereas opaque text cannot
    // smuggle braces, quotes or nested name/method fields into the envelope.
    fn string_end(&mut self) -> bool {
        self.index += 1;
        while let Some(&byte) = self.raw.get(self.index) {
            self.index += 1;
            match byte {
                b'"' => return true,
                b'\\' => self.index += usize::from(self.index < self.raw.len()),
                _ => {}
            }
        }
        false
    }

    fn skip_value(&mut self) -> Result<bool, ()> {
        let Some(&first) = self.raw.get(self.index) else {
            return Ok(false);
        };
        if first == b'"' {
            return Ok(self.string_end());
        }
        if first != b'{' && first != b'[' {
            let start = self.index;
            while let Some(&byte) = self.raw.get(self.index) {
                if matches!(
                    byte,
                    b' ' | b'\t' | b'\r' | b'\n' | b',' | b'}' | b']' | b'{' | b'[' | b'"'
                ) {
                    break;
                }
                self.index += 1;
            }
            return (self.index != start).then_some(true).ok_or(());
        }
        // Bounded by the physical frame (<=1 MiB), not Go's admission depth:
        // depth-rejected Skills must still be diagnosed without Value recursion.
        let mut stack = vec![first];
        self.index += 1;
        while let Some(&byte) = self.raw.get(self.index) {
            match byte {
                b'"' => {
                    if !self.string_end() {
                        return Ok(false);
                    }
                    continue;
                }
                b'{' | b'[' => stack.push(byte),
                b'}' | b']' => {
                    let expected = if byte == b'}' { b'{' } else { b'[' };
                    if stack.pop() != Some(expected) {
                        return Err(());
                    }
                    if stack.is_empty() {
                        self.index += 1;
                        return Ok(true);
                    }
                }
                _ => {}
            }
            self.index += 1;
        }
        Ok(false)
    }
}

fn decode_string(raw: &[u8]) -> Result<String, ()> {
    syntax::validate(raw).map_err(|_| ())?;
    serde_json::from_str(&strings::repair(raw, true)).map_err(|_| ())
}
