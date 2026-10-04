use std::collections::BTreeSet;

use chrono::{DateTime, Duration, NaiveDateTime, Timelike};

use super::{
    Metadata,
    bytes::{equal_fold, quotes, set, trim},
    model::Time,
};

pub(super) const PROMOTION: &[u8] = b"recurring_only;prefer_6h;single_10min_not_promotable";

pub(super) fn codex(data: &[u8]) -> (Metadata, Vec<u8>) {
    let mut normalized = Vec::with_capacity(data.len());
    let mut index = 0;
    while index < data.len() {
        if data[index..].starts_with(b"\r\n") {
            index += 1;
        }
        normalized.push(data[index]);
        index += 1;
    }
    let mut metadata = Metadata::new();
    if !normalized.starts_with(b"---\n") {
        return (metadata, normalized);
    }
    let Some(end) = normalized[4..]
        .windows(4)
        .position(|b| b == b"\n---")
        .map(|n| n + 4)
    else {
        return (metadata, normalized);
    };
    let mut current = Vec::new();
    for line in normalized[4..end].split(|b| *b == b'\n') {
        let line = trim(line);
        if line.is_empty() || line.starts_with(b"#") {
            continue;
        }
        if line.starts_with(b"-") && current == b"applications" {
            let value = trim(&line[1..]);
            if !value.is_empty() {
                let existing = metadata.entry(current.clone()).or_default();
                if !existing.is_empty() {
                    existing.push(b',');
                }
                existing.extend(value);
            }
            continue;
        }
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            current.clear();
            continue;
        };
        let key = trim(&line[..colon]);
        if key.is_empty() {
            current.clear();
            continue;
        }
        current = key.to_vec();
        set(
            &mut metadata,
            key,
            trim(quotes(trim(&line[colon + 1..]))).to_vec(),
        );
    }
    let body = &normalized[end + 4..];
    (metadata, body.strip_prefix(b"\n").unwrap_or(body).to_vec())
}

pub(super) fn applications(raw: &[u8]) -> Vec<Vec<u8>> {
    let raw = trim(raw);
    if raw.is_empty() {
        return Vec::new();
    }
    let raw = if raw.starts_with(b"[") && raw.ends_with(b"]") {
        // Go accepts null elements in []string as empty strings. Serde's
        // Option<String> models that limited array contract; malformed JSON
        // deliberately falls back to comma splitting, as the Go owner does.
        if let Ok(values) = serde_json::from_slice::<Vec<Option<String>>>(&json_strings(raw)) {
            return clean_applications(
                values
                    .into_iter()
                    .map(|v| v.unwrap_or_default().into_bytes()),
            );
        }
        trim(&raw[1..raw.len() - 1])
    } else {
        raw
    };
    clean_applications(raw.split(|b| *b == b',').map(<[u8]>::to_vec))
}

fn clean_applications(values: impl Iterator<Item = Vec<u8>>) -> Vec<Vec<u8>> {
    values
        .map(|v| quotes(trim(&v)).to_vec())
        .filter(|v| !v.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn resource(base: &[u8]) -> Option<(&'static str, Time)> {
    // Match Go's anchored filename RE without projecting arbitrary path bytes.
    if base.len() < 27 || !base.ends_with(b".md") {
        return None;
    }
    let timestamp = std::str::from_utf8(base.get(..19)?).ok()?;
    if base.get(19) != Some(&b'-') {
        return None;
    }
    let remainder = base.get(20..)?;
    let dash = remainder.iter().position(|b| *b == b'-')?;
    if dash == 0 {
        return None;
    }
    let tail = &remainder[dash + 1..];
    let (kind, name) = tail
        .strip_prefix(b"10min-")
        .map(|n| ("10min", n))
        .or_else(|| tail.strip_prefix(b"6h-").map(|n| ("6h", n)))?;
    if name.len() <= 3 || name[..name.len() - 3].contains(&b'\n') {
        return None;
    }
    if !timestamp.bytes().enumerate().all(|(i, b)| match i {
        4 | 7 | 13 | 16 => b == b'-',
        10 => b == b'T',
        _ => b.is_ascii_digit(),
    }) {
        return None;
    }
    let parsed = NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H-%M-%S").ok()?;
    // Chrono encodes second 60 as second 59 plus >=1e9 nanoseconds. Go's
    // time.Parse rejects it; retain this file as ordinary consolidated Markdown.
    if parsed.nanosecond() >= 1_000_000_000 {
        return None;
    }
    Some((kind, parsed.and_utc().fixed_offset()))
}

pub(super) fn activity_metadata(metadata: &mut Metadata, kind: &str, start: Time) {
    let duration = if kind == "6h" {
        Duration::hours(6)
    } else {
        Duration::minutes(10)
    };
    set(metadata, b"granularity", kind.as_bytes().to_vec());
    set(
        metadata,
        b"activity_started_at",
        super::files::utc_seconds(start),
    );
    set(
        metadata,
        b"activity_ends_at",
        super::files::utc_seconds(start + duration),
    );
    set(
        metadata,
        b"promotable",
        if kind == "10min" {
            b"false".to_vec()
        } else {
            b"candidate".to_vec()
        },
    );
}

pub(super) fn citations(body: &[u8]) -> Vec<u8> {
    let lines = body.split(|b| *b == b'\n').collect::<Vec<_>>();
    let Some(start) = lines
        .iter()
        .position(|line| equal_fold(trim(line), b"## Citations"))
    else {
        return Vec::new();
    };
    let end = lines[start + 1..]
        .iter()
        .position(|line| trim(line).starts_with(b"## "))
        .map_or(lines.len(), |i| i + start + 1);
    trim(&lines[start + 1..end].join(&b'\n')).to_vec()
}

pub(super) fn simple_yaml(lines: &[Vec<u8>]) -> Metadata {
    let mut result = Metadata::new();
    for line in lines {
        let line = trim(line);
        if line.is_empty() || line.starts_with(b"#") {
            continue;
        }
        if let Some(colon) = line.iter().position(|b| *b == b':') {
            set(
                &mut result,
                trim(&line[..colon]),
                quotes(trim(&line[colon + 1..])).to_vec(),
            );
        }
    }
    result
}

pub(super) fn links(body: &[u8]) -> Vec<Vec<u8>> {
    let re = regex::bytes::Regex::new(r"(?-u:\[\[([^\]|]+)(?:\|[^\]]+)?\]\])")
        .expect("fixed link regex");
    let mut seen = BTreeSet::new();
    re.captures_iter(body)
        .filter_map(|captures| {
            let link = trim(captures.get(1)?.as_bytes()).to_vec();
            if seen.insert(link.clone()) {
                Some(link)
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn rfc_seconds(timestamp: Option<Time>) -> Vec<u8> {
    timestamp.map_or_else(
        || b"0001-01-01T00:00:00Z".to_vec(),
        |time| {
            if time.offset().local_minus_utc() == 0 {
                super::files::utc_seconds(time)
            } else {
                time.format("%Y-%m-%dT%H:%M:%S%:z").to_string().into_bytes()
            }
        },
    )
}

pub(super) fn epoch(seconds: i64) -> Option<Time> {
    DateTime::from_timestamp(seconds, 0)
        .map(|time| time.with_timezone(&chrono::Local).fixed_offset())
}

// Only the applications []string decoder needs this projection. It never
// touches path bytes or Markdown bodies. Go Unmarshal replaces every invalid
// UTF-8 byte and lone escaped UTF-16 surrogate inside JSON strings.
fn json_strings(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut index = 0;
    let mut in_string = false;
    while index < raw.len() {
        if raw[index] == b'"' {
            in_string = !in_string;
        }
        if in_string && raw[index] == b'\\' && index + 1 < raw.len() {
            if raw[index + 1] == b'u' {
                if let Some(unit) = hex_unit(raw.get(index + 2..index + 6)) {
                    if (0xd800..=0xdbff).contains(&unit) {
                        let low = raw
                            .get(index + 6..index + 8)
                            .filter(|v| *v == b"\\u")
                            .and_then(|_| hex_unit(raw.get(index + 8..index + 12)));
                        if low.is_some_and(|v| (0xdc00..=0xdfff).contains(&v)) {
                            out.extend_from_slice(&raw[index..index + 12]);
                            index += 12;
                            continue;
                        }
                    }
                    if (0xd800..=0xdfff).contains(&unit) {
                        out.extend_from_slice(b"\\ufffd");
                        index += 6;
                        continue;
                    }
                }
            }
            out.extend_from_slice(&raw[index..index + 2]);
            index += 2;
            continue;
        }
        let (ch, width) = super::bytes::rune(&raw[index..]);
        let mut encoded = [0; 4];
        out.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
        index += width;
    }
    out
}

fn hex_unit(raw: Option<&[u8]>) -> Option<u16> {
    let text = std::str::from_utf8(raw?).ok()?;
    if text.len() != 4 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(text, 16).ok()
}
