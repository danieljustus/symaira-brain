//! yaml.v3 v3.0.1 string-map ordering, before library serialization.
// Copyright (c) 2011-2019 Canonical Ltd. Apache-2.0, see repository LICENSE.
use std::cmp::Ordering;

use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use serde_json::Value;

#[path = "yaml_key_ranges.rs"]
mod ranges;

pub(super) struct Metadata<'a>(pub(super) &'a Value);

impl Serialize for Metadata<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Object(map) => {
                let mut entries: Vec<_> = map.iter().collect();
                entries.sort_by(|(a, _), (b, _)| {
                    if less(a, b) {
                        Ordering::Less
                    } else if less(b, a) {
                        Ordering::Greater
                    } else {
                        Ordering::Equal
                    }
                });
                let mut output = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    output.serialize_entry(key, &Metadata(value))?;
                }
                output.end()
            }
            Value::Array(items) => {
                let mut output = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    output.serialize_element(&Metadata(item))?;
                }
                output.end()
            }
            value => value.serialize(serializer),
        }
    }
}

fn in_ranges(ch: char, ranges: &[(u32, u32)]) -> bool {
    let value = u32::from(ch);
    ranges
        .binary_search_by(|&(start, end)| {
            if value < start {
                Ordering::Greater
            } else if value > end {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
        .is_ok()
}

// Exact string branch of yaml.v3 sorter.go. Go uses Unicode L/Nd, not
// Rust's wider Alphabetic/Numeric predicates, and wraps int64 accumulation.
fn less(a: &str, b: &str) -> bool {
    let ar: Vec<_> = a.chars().collect();
    let br: Vec<_> = b.chars().collect();
    let mut digits = false;
    for i in 0..ar.len().min(br.len()) {
        if ar[i] == br[i] {
            digits = in_ranges(ar[i], ranges::DIGIT);
            continue;
        }
        let al = in_ranges(ar[i], ranges::LETTER);
        let bl = in_ranges(br[i], ranges::LETTER);
        if al && bl {
            return ar[i] < br[i];
        }
        if al || bl {
            return if digits { al } else { bl };
        }
        let (mut an, mut bn) = (0_i64, 0_i64);
        if ar[i] == '0' || br[i] == '0' {
            for j in (0..i).rev() {
                if !in_ranges(ar[j], ranges::DIGIT) {
                    break;
                }
                if ar[j] != '0' {
                    an = 1;
                    bn = 1;
                    break;
                }
            }
        }
        let (mut ai, mut bi) = (i, i);
        while ai < ar.len() && in_ranges(ar[ai], ranges::DIGIT) {
            an = an
                .wrapping_mul(10)
                .wrapping_add(i64::from(u32::from(ar[ai])) - i64::from(u32::from('0')));
            ai += 1;
        }
        while bi < br.len() && in_ranges(br[bi], ranges::DIGIT) {
            bn = bn
                .wrapping_mul(10)
                .wrapping_add(i64::from(u32::from(br[bi])) - i64::from(u32::from('0')));
            bi += 1;
        }
        if an != bn {
            return an < bn;
        }
        if ai != bi {
            return ai < bi;
        }
        return ar[i] < br[i];
    }
    ar.len() < br.len()
}
