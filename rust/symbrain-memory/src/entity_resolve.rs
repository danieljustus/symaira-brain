//! Pure entity-matching primitives (ported for #760 from
//! `internal/memory/entity/resolve.go` at v0.12.0).
//!
//! `normalize` is Unicode-aware (NFD, drop combining marks, NFC, lowercase,
//! whitespace collapse); `best_match` scores a set of hints against one
//! entity's name and aliases; `is_pii` rejects email/phone-shaped hints.
//!
//! Deviation from Go: combining marks are removed by general category `M`
//! (`Mn`, `Mc`, `Me`) because the normalization crate exposes no `Mn`-only
//! table. Both sides of every comparison go through the same function, so
//! matching stays symmetric; only exotic spacing marks differ from Go.

use regex::Regex;
use std::sync::OnceLock;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

/// Which comparison produced a candidate match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    /// Case-insensitive equal to the entity name.
    ExactName,
    /// Case-insensitive equal to an alias.
    ExactAlias,
    /// Equal to the name after normalization.
    NormalizedName,
    /// Equal to an alias after normalization.
    NormalizedAlias,
}

impl MatchKind {
    /// The `snake_case` wire name used by Go.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExactName => "exact_name",
            Self::ExactAlias => "exact_alias",
            Self::NormalizedName => "normalized_name",
            Self::NormalizedAlias => "normalized_alias",
        }
    }

    const fn score(self) -> f64 {
        match self {
            Self::ExactName => 1.0,
            Self::ExactAlias => 0.9,
            Self::NormalizedName => 0.75,
            Self::NormalizedAlias => 0.65,
        }
    }
}

/// Bounds a single query or alias hint.
pub const MAX_HINT_LENGTH: usize = 500;

/// The strongest match found by [`best_match`].
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    /// Comparison that matched.
    pub kind: MatchKind,
    /// Human-readable explanation (Go `%q` formatting).
    pub reason: String,
    /// Fixed score for the kind.
    pub score: f64,
}

/// Comparison-safe form of `s`.
#[must_use]
pub fn normalize(s: &str) -> String {
    let folded: String = s.nfd().filter(|c| !is_combining_mark(*c)).nfc().collect();
    folded
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn equal_fold(a: &str, b: &str) -> bool {
    a == b || a.to_lowercase() == b.to_lowercase()
}

fn email_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").expect("static regex")
    })
}

fn phone_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\+?[0-9()\-.\s]+$").expect("static regex"))
}

/// Reports whether `s` looks like an email address or phone number.
#[must_use]
pub fn is_pii(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    if email_re().is_match(s) {
        return true;
    }
    if !phone_re().is_match(s) {
        return false;
    }
    s.chars().filter(|c| c.is_numeric()).count() >= 6
}

fn reason_for(kind: MatchKind, hint: &str, target: &str) -> String {
    match kind {
        MatchKind::ExactName => format!("{hint:?} exactly matches entity name {target:?}"),
        MatchKind::ExactAlias => format!("{hint:?} exactly matches alias {target:?}"),
        MatchKind::NormalizedName => {
            format!("{hint:?} matches entity name {target:?} after case/diacritic normalization")
        }
        MatchKind::NormalizedAlias => {
            format!("{hint:?} matches alias {target:?} after case/diacritic normalization")
        }
    }
}

/// Strongest match of any hint against `name` and `aliases`; ties keep the
/// first hint in input order. `None` when nothing matched.
#[must_use]
pub fn best_match(hints: &[&str], name: &str, aliases: &[&str]) -> Option<Match> {
    let norm_name = normalize(name);
    let mut best: Option<Match> = None;
    let mut consider = |kind: MatchKind, hint: &str, target: &str| {
        let score = kind.score();
        if best.as_ref().is_some_and(|b| score <= b.score) {
            return;
        }
        best = Some(Match {
            kind,
            reason: reason_for(kind, hint, target),
            score,
        });
    };
    for hint in hints {
        if equal_fold(hint, name) {
            consider(MatchKind::ExactName, hint, name);
        } else if normalize(hint) == norm_name {
            consider(MatchKind::NormalizedName, hint, name);
        }
        for alias in aliases {
            if equal_fold(hint, alias) {
                consider(MatchKind::ExactAlias, hint, alias);
            } else if normalize(hint) == normalize(alias) {
                consider(MatchKind::NormalizedAlias, hint, alias);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_table() {
        for (input, want) in [
            ("  Zoë   Müller ", "zoe muller"),
            ("Jose\u{301}", "jose"),
            ("José", "jose"),
            ("", ""),
            ("A\tB\nC", "a b c"),
        ] {
            assert_eq!(normalize(input), want, "{input:?}");
        }
    }

    #[test]
    fn pii_table() {
        for (input, want) in [
            ("a@b.com", true),
            (" Max.Muster+x@example.org ", true),
            ("+49 (151) 234-5678", true),
            ("12345", false),
            ("123456", true),
            ("Max Muster", false),
            ("", false),
        ] {
            assert_eq!(is_pii(input), want, "{input:?}");
        }
    }

    type Case<'a> = (
        &'a [&'a str],
        &'a str,
        &'a [&'a str],
        Option<(MatchKind, f64)>,
    );

    #[test]
    fn best_match_table() {
        let cases: &[Case] = &[
            (&["acme"], "Acme", &[], Some((MatchKind::ExactName, 1.0))),
            (
                &["Zoe"],
                "Zoë",
                &[],
                Some((MatchKind::NormalizedName, 0.75)),
            ),
            (
                &["Bob"],
                "Robert",
                &["bob"],
                Some((MatchKind::ExactAlias, 0.9)),
            ),
            (
                &["jose"],
                "X",
                &["José"],
                Some((MatchKind::NormalizedAlias, 0.65)),
            ),
            (&["nope"], "Acme", &["Foo"], None),
            (
                &["Zoe", "ZOË"],
                "zoë",
                &[],
                Some((MatchKind::ExactName, 1.0)),
            ),
        ];
        for (hints, name, aliases, want) in cases {
            let got = best_match(hints, name, aliases).map(|m| (m.kind, m.score));
            assert_eq!(got, *want, "{hints:?} vs {name}");
        }
    }

    #[test]
    fn reason_uses_go_quoting() {
        let m = best_match(&["Zoe"], "Zoë", &[]).unwrap();
        assert_eq!(
            m.reason,
            "\"Zoe\" matches entity name \"Zoë\" after case/diacritic normalization"
        );
        assert_eq!(MatchKind::NormalizedAlias.as_str(), "normalized_alias");
    }
}
