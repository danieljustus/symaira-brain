//! Keyless, deterministic lookalike-description detection.
//!
//! Descriptions are reduced to sets of lowercase alphanumeric words of at
//! least [`MIN_WORD_CHARS`] characters, and two descriptions are lookalikes
//! when the Jaccard overlap of their sets reaches [`THRESHOLD_PERMILLE`]. The
//! computation is integer-only so output bytes never depend on float
//! formatting. No model, embedding or network is involved.

use std::collections::BTreeSet;

use serde::Serialize;

/// Minimum similarity, in permille of the Jaccard index, to report a pair.
///
/// 800 is deliberately conservative: near-duplicate descriptions (the pair
/// that motivated the check scored 830) are reported, while skills in the same
/// domain that merely share vocabulary stay far below it.
pub const THRESHOLD_PERMILLE: usize = 800;
/// Descriptions with fewer distinct words are never compared; short strings
/// reach a high overlap by chance.
pub const MIN_WORDS: usize = 6;
/// Words shorter than this are dropped before comparison.
pub const MIN_WORD_CHARS: usize = 3;

/// One pair of skills whose descriptions overlap above the threshold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Lookalike {
    /// Skill name sorting first.
    pub a: String,
    /// Skill name sorting second.
    pub b: String,
    /// Jaccard index in permille (1000 = identical word sets).
    pub similarity_permille: usize,
}

/// Lowercase alphanumeric word set of a description.
#[must_use]
pub fn word_set(description: &str) -> BTreeSet<String> {
    description
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.chars().count() >= MIN_WORD_CHARS)
        .map(str::to_owned)
        .collect()
}

/// Jaccard index in permille, or `None` when either set is below
/// [`MIN_WORDS`] or both are empty.
#[must_use]
pub fn similarity_permille(left: &BTreeSet<String>, right: &BTreeSet<String>) -> Option<usize> {
    if left.len() < MIN_WORDS || right.len() < MIN_WORDS {
        return None;
    }
    let shared = left.intersection(right).count();
    let union = left.len() + right.len() - shared;
    Some(shared * 1000 / union)
}

/// Finds every lookalike pair among `(name, description)` entries.
///
/// The result is sorted by `(a, b)`; entries with identical names are
/// compared like any other pair.
#[must_use]
pub fn find_lookalikes(entries: &[(String, String)]) -> Vec<Lookalike> {
    let mut sorted: Vec<(&str, BTreeSet<String>)> = entries
        .iter()
        .map(|(name, description)| (name.as_str(), word_set(description)))
        .collect();
    sorted.sort_by(|left, right| left.0.cmp(right.0));
    let mut pairs = Vec::new();
    for (index, (name_a, words_a)) in sorted.iter().enumerate() {
        for (name_b, words_b) in &sorted[index + 1..] {
            if let Some(permille) = similarity_permille(words_a, words_b)
                && permille >= THRESHOLD_PERMILLE
            {
                pairs.push(Lookalike {
                    a: (*name_a).to_owned(),
                    b: (*name_b).to_owned(),
                    similarity_permille: permille,
                });
            }
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, description: &str) -> (String, String) {
        (name.to_owned(), description.to_owned())
    }

    #[test]
    fn similarity_is_table_driven() {
        const BASE: &str = "Use when reviewing pull requests for security flaws before merge";
        for (left, right, want) in [
            (BASE, BASE, Some(1000)),
            // Case and punctuation do not matter.
            (
                BASE,
                "USE WHEN, reviewing pull-requests for security flaws before merge!",
                Some(1000),
            ),
            (
                BASE,
                "Use when reviewing pull requests for security flaws before release",
                Some(818),
            ),
            (
                BASE,
                "Draft quarterly budget spreadsheets with currency conversion",
                Some(0),
            ),
            // Too few distinct words: never comparable.
            ("Use when ready", "Use when ready", None),
            (BASE, "", None),
        ] {
            assert_eq!(
                similarity_permille(&word_set(left), &word_set(right)),
                want,
                "{left:?} vs {right:?}"
            );
        }
    }

    #[test]
    fn near_duplicates_are_paired_and_unrelated_are_not() {
        let entries = [
            entry(
                "zeta-review",
                "Use when reviewing pull requests for security flaws in rust code before merge",
            ),
            entry(
                "alpha-review",
                "Use when reviewing pull requests for security flaws in rust code before merge today",
            ),
            entry(
                "budget",
                "Draft quarterly budget spreadsheets with currency conversion and charts",
            ),
            entry("tiny", "Use when ready"),
        ];
        let pairs = find_lookalikes(&entries);
        assert_eq!(
            pairs,
            [Lookalike {
                a: "alpha-review".into(),
                b: "zeta-review".into(),
                similarity_permille: 923,
            }]
        );
        assert_eq!(find_lookalikes(&entries), pairs, "deterministic");
    }

    #[test]
    fn threshold_boundary_is_inclusive() {
        // 8 shared words of 10 in the union = 800 permille exactly.
        let shared = "alpha bravo charlie delta echo foxtrot golf hotel";
        let left = entry("left", &format!("{shared} india"));
        let right = entry("right", &format!("{shared} juliet"));
        assert_eq!(find_lookalikes(&[left, right])[0].similarity_permille, 800);
        let off = entry("off", &format!("{shared} kilo lima"));
        let wide = entry("wide", &format!("{shared} mike november"));
        assert!(find_lookalikes(&[off, wide]).is_empty(), "8/12 stays below");
    }
}
