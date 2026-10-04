//! Typed assignment and Go's first recoverable type error.

use super::super::{SyncError, SyncTime};
use super::raw::Raw;
use chrono::{DateTime, Timelike};

#[derive(Default)]
pub(super) struct Decode {
    pub error: Option<SyncError>,
}
impl Decode {
    pub fn fail(&mut self, raw: Raw<'_>, ty: &str, path: &str) {
        let value = if raw.kind() == "number"
            && matches!(ty, "int" | "int64" | "uint8" | "float32" | "float64")
        {
            format!("number {}", String::from_utf8_lossy(raw.0))
        } else {
            raw.kind().into()
        };
        let target = if path.is_empty() {
            format!("Go value of type {ty}")
        } else {
            format!("Go struct field {path} of type {ty}")
        };
        self.error.get_or_insert_with(|| {
            SyncError(format!("json: cannot unmarshal {value} into {target}"))
        });
    }
    pub fn finish<T>(self, value: T) -> Result<T, SyncError> {
        self.error.map_or(Ok(value), Err)
    }
    pub fn string(
        &mut self,
        raw: Raw<'_>,
        target: &mut String,
        path: &str,
    ) -> Result<(), SyncError> {
        if raw.null() {
            return Ok(());
        }
        if raw.kind() == "string" {
            *target = super::strings::unquote(raw.0)?;
        } else {
            self.fail(raw, "string", path);
        }
        Ok(())
    }
    pub fn integer(&mut self, raw: Raw<'_>, target: &mut i64, ty: &str, path: &str) {
        if raw.null() {
            return;
        }
        if raw.kind() == "number" {
            if ty == "uint8" && raw.0.first() == Some(&b'-') {
                self.fail(raw, ty, path);
                return;
            }
            if let Ok(number) = std::str::from_utf8(raw.0).unwrap_or("").parse::<i64>() {
                *target = number;
                return;
            }
        }
        self.fail(raw, ty, path);
    }
    pub fn float(&mut self, raw: Raw<'_>, target: &mut f64, narrow: bool, path: &str) {
        if raw.null() {
            return;
        }
        if raw.kind() == "number" {
            let text = std::str::from_utf8(raw.0).unwrap_or("");
            let number = if narrow {
                text.parse::<f32>().map(f64::from)
            } else {
                text.parse::<f64>()
            };
            if let Ok(number) = number {
                if number.is_finite() {
                    *target = number;
                    return;
                }
            }
        }
        self.fail(raw, if narrow { "float32" } else { "float64" }, path);
    }
    pub fn clock(&mut self, raw: Raw<'_>, target: &mut SyncTime) -> Result<(), SyncError> {
        if raw.null() {
            return Ok(());
        }
        if raw.kind() != "string" {
            return Err(SyncError(
                "Time.UnmarshalJSON: input is not a JSON string".into(),
            ));
        }
        // Go time.UnmarshalJSON uses the quoted bytes directly, not JSON
        // unquoteBytes. Escaped time strings therefore remain invalid.
        let text = std::str::from_utf8(&raw.0[1..raw.0.len() - 1]).unwrap_or("");
        let normalized = normalize_time(text);
        let parsed = DateTime::parse_from_rfc3339(&normalized).map_err(|_| {
            SyncError(format!(
                "native sync time needs frozen Go diagnostic proof for {}",
                super::super::wire::quote(text)
            ))
        })?;
        // Chrono accepts leap seconds; Go's time.Parse rejects them.
        if parsed.nanosecond() >= 1_000_000_000 {
            return Err(SyncError(
                "native sync leap-second admission is unproven".into(),
            ));
        }
        *target = SyncTime(parsed);
        Ok(())
    }
}
fn normalize_time(text: &str) -> String {
    // Pinned parseStrictRFC3339 currently falls back to permissive time.Parse:
    // comma fraction, >9 fraction digits, and one-digit hours are accepted.
    let mut text = text.replace(',', '.');
    if text.as_bytes().get(12) == Some(&b':') {
        text.insert(11, '0');
    }
    if let Some(at) = text.find('.') {
        let end = text[at + 1..]
            .find(|c: char| !c.is_ascii_digit())
            .map_or(text.len(), |n| at + 1 + n);
        if end > at + 10 {
            text.replace_range(at + 10..end, "");
        }
    }
    text
}

pub(super) fn object<'a>(
    raw: Raw<'a>,
    decode: &mut Decode,
    ty: &str,
    path: &str,
) -> Result<Option<Vec<(String, Raw<'a>)>>, SyncError> {
    if raw.null() {
        return Ok(None);
    }
    if raw.kind() != "object" {
        decode.fail(raw, ty, path);
        return Ok(None);
    }
    Ok(Some(raw.members()?))
}

/// Visible length is separate from initialized backing slots. Nonempty shrink
/// must not destroy hidden values retained by a later null element; null/[]
/// reset allocation. This preserves the independently found Hermes slice law.
#[derive(Clone)]
pub(super) struct Slice<T> {
    pub slots: Vec<T>,
    pub len: usize,
    pub nil: bool,
}
impl<T> Default for Slice<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            len: 0,
            nil: true,
        }
    }
}
impl<T: Default + Clone> Slice<T> {
    pub fn assign(
        &mut self,
        raw: Raw<'_>,
        decode: &mut Decode,
        ty: &str,
        path: &str,
        mut assign: impl FnMut(Raw<'_>, &mut T, &mut Decode) -> Result<(), SyncError>,
    ) -> Result<(), SyncError> {
        if raw.null() {
            self.slots.clear();
            self.len = 0;
            self.nil = true;
            return Ok(());
        }
        if raw.kind() != "array" {
            decode.fail(raw, ty, path);
            return Ok(());
        }
        let elements = raw.elements()?;
        if elements.is_empty() {
            self.slots.clear();
        }
        if self.slots.len() < elements.len() {
            self.slots.resize_with(elements.len(), T::default);
        }
        for (index, item) in elements.iter().enumerate() {
            assign(*item, &mut self.slots[index], decode)?;
        }
        self.len = elements.len();
        self.nil = false;
        Ok(())
    }
    pub fn visible(&self) -> Option<Vec<T>> {
        (!self.nil).then(|| self.slots[..self.len].to_vec())
    }
}
