//! Versioned per-target harness delivery profiles.
//!
//! Every value records the harness version and date it was verified against,
//! so a stale profile is visible instead of silently trusted. A value of
//! `None` (or [`Behaviour::Unverified`]) means "not profiled": no rule fires
//! for it. The table is deliberately small and code-resident; it is consumed
//! by the advisory `validate --targets` rules ([`crate::harness_findings`])
//! and by the `doctor --catalog` listing budget ([`crate::catalog`]).

/// Where and when a profile value was verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verified {
    /// Harness version the value was checked against.
    pub harness_version: &'static str,
    /// ISO date of the check.
    pub verified_on: &'static str,
    /// How the value was established.
    pub source: &'static str,
}

/// One numeric delivery limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    /// Limit value in `unit` (see the field that holds the limit).
    pub value: usize,
    /// What the harness does once the limit is crossed.
    pub effect: &'static str,
    /// Verification record.
    pub verified: Verified,
}

/// How a harness treats a particular manifest layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behaviour {
    /// The harness registers the manifest as its own top-level skill.
    LoadsAsSkill,
    /// The harness ignores the manifest.
    Ignored,
    /// Not verified; no rule fires.
    Unverified,
}

/// A manifest behaviour with its verification record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManifestRule {
    /// Observed behaviour.
    pub behaviour: Behaviour,
    /// Verification record, absent while [`Behaviour::Unverified`].
    pub verified: Option<Verified>,
}

/// Share of the model context window a harness spends on its skill list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListingBudget {
    /// Percent of the context window.
    pub percent_of_window: usize,
    /// Characters per token used to convert tokens to characters.
    pub chars_per_token: usize,
    /// Upper bound in tokens regardless of window size.
    pub max_tokens: Option<usize>,
    /// Verification record.
    pub verified: Verified,
}

impl ListingBudget {
    /// Listing budget in characters for a context window of `window_tokens`.
    #[must_use]
    pub fn chars(&self, window_tokens: usize) -> usize {
        let tokens = window_tokens.saturating_mul(self.percent_of_window) / 100;
        let tokens = self.max_tokens.map_or(tokens, |max| tokens.min(max));
        tokens.saturating_mul(self.chars_per_token)
    }
}

/// Conservative context window assumed when the caller names none. At 1% and
/// four characters per token it yields Claude Code's documented 8,000-char
/// fallback.
pub const DEFAULT_CONTEXT_WINDOW_TOKENS: usize = 200_000;

/// Which base directory an external discovery root hangs off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootBase {
    /// The user's home directory.
    Home,
    /// The project directory.
    Project,
}

/// A discovery root a harness reads besides its own user and project roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalRoot {
    /// Base directory.
    pub base: RootBase,
    /// Slash-separated path below the base.
    pub path: &'static str,
}

/// Delivery profile of one harness target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessProfile {
    /// Stable target name.
    pub target: &'static str,
    /// Characters of a description the harness keeps in its skill list.
    pub description_cut: Option<Limit>,
    /// Leading characters of a description in which a trigger phrase must sit.
    pub listing_trigger_window: Option<Limit>,
    /// Largest `SKILL.md` delivery, in JSON-encoded characters.
    pub body_delivery_budget: Option<Limit>,
    /// Largest `SKILL.md` a shell read returns whole, in bytes.
    pub read_budget: Option<Limit>,
    /// Behaviour for a `SKILL.md` inside a subfolder of a skill.
    pub nested_manifest: ManifestRule,
    /// Behaviour for a symlinked `SKILL.md`.
    pub symlinked_manifest: ManifestRule,
    /// Skill-list budget relative to the context window.
    pub listing_budget: Option<ListingBudget>,
    /// Extra discovery roots, scanned for duplicate skill names.
    pub external_roots: &'static [ExternalRoot],
    /// Whether the harness descends into category folders below a root.
    pub recursive_discovery: bool,
}

const UNVERIFIED: ManifestRule = ManifestRule {
    behaviour: Behaviour::Unverified,
    verified: None,
};

const fn empty(target: &'static str) -> HarnessProfile {
    HarnessProfile {
        target,
        description_cut: None,
        listing_trigger_window: None,
        body_delivery_budget: None,
        read_budget: None,
        nested_manifest: UNVERIFIED,
        symlinked_manifest: UNVERIFIED,
        listing_budget: None,
        external_roots: &[],
        recursive_discovery: false,
    }
}

const fn nested_loads(verified: Verified) -> ManifestRule {
    ManifestRule {
        behaviour: Behaviour::LoadsAsSkill,
        verified: Some(verified),
    }
}

const DATE: &str = "2026-11-16";

const HERMES_SOURCE: Verified = Verified {
    harness_version: "hermes 0.23.1",
    verified_on: DATE,
    source: "installed source: tools/budget_config.py, tools/skills_tool.py",
};
const HERMES_LISTING: Verified = Verified {
    harness_version: "hermes 0.23.1",
    verified_on: DATE,
    source: "skill index shows the first 60 description characters",
};
const CODEX_READ: Verified = Verified {
    harness_version: "codex-cli 0.169.0",
    verified_on: DATE,
    source: "upstream probe: shell reads cut SKILL.md near 40 KB",
};
const CODEX_NESTED: Verified = Verified {
    harness_version: "codex-cli 0.169.0",
    verified_on: DATE,
    source: "discovery walk registers every SKILL.md below the root",
};
const OPENCODE_NESTED: Verified = Verified {
    harness_version: "opencode 2.3.0",
    verified_on: DATE,
    source: "discovery walk registers every SKILL.md below the root",
};
const CLAUDE_LISTING: Verified = Verified {
    harness_version: "claude-code 2.1.325",
    verified_on: DATE,
    source: "upstream probe: 1% of context at ~4 chars/token, 8,000 chars unknown",
};
const CODEX_LISTING: Verified = Verified {
    harness_version: "codex-cli 0.169.0",
    verified_on: DATE,
    source: "upstream probe: 2% of context, at most ~10k tokens",
};
const DISCOVERY: Verified = Verified {
    harness_version: "documented discovery paths",
    verified_on: DATE,
    source: "harness documentation, not a live probe",
};

const HERMES_EXTERNAL: &[ExternalRoot] = &[ExternalRoot {
    base: RootBase::Home,
    path: ".hermes/skills",
}];
const CODEX_EXTERNAL: &[ExternalRoot] = &[ExternalRoot {
    base: RootBase::Home,
    path: ".codex/skills",
}];
const OPENCODE_EXTERNAL: &[ExternalRoot] = &[
    ExternalRoot {
        base: RootBase::Home,
        path: ".claude/skills",
    },
    ExternalRoot {
        base: RootBase::Home,
        path: ".agents/skills",
    },
    ExternalRoot {
        base: RootBase::Project,
        path: ".claude/skills",
    },
    ExternalRoot {
        base: RootBase::Project,
        path: ".agents/skills",
    },
];

const PROFILES: [HarnessProfile; 6] = [
    HarnessProfile {
        description_cut: Some(Limit {
            value: 60,
            effect: "the skill list shows only the first 60 description characters",
            verified: HERMES_LISTING,
        }),
        listing_trigger_window: Some(Limit {
            value: 60,
            effect: "a trigger phrase after the window is invisible in the skill list",
            verified: HERMES_LISTING,
        }),
        body_delivery_budget: Some(Limit {
            value: 100_000,
            effect: "skill_view delivers a 1,500-char preview instead of the instructions",
            verified: HERMES_SOURCE,
        }),
        nested_manifest: nested_loads(HERMES_SOURCE),
        external_roots: HERMES_EXTERNAL,
        recursive_discovery: true,
        ..empty("hermes")
    },
    HarnessProfile {
        read_budget: Some(Limit {
            value: 40 * 1024,
            effect: "shell reads cut the file before the end",
            verified: CODEX_READ,
        }),
        nested_manifest: nested_loads(CODEX_NESTED),
        listing_budget: Some(ListingBudget {
            percent_of_window: 2,
            chars_per_token: 4,
            max_tokens: Some(10_000),
            verified: CODEX_LISTING,
        }),
        external_roots: CODEX_EXTERNAL,
        ..empty("codex")
    },
    HarnessProfile {
        nested_manifest: nested_loads(OPENCODE_NESTED),
        external_roots: OPENCODE_EXTERNAL,
        ..empty("opencode")
    },
    HarnessProfile {
        listing_budget: Some(ListingBudget {
            percent_of_window: 1,
            chars_per_token: 4,
            max_tokens: None,
            verified: CLAUDE_LISTING,
        }),
        ..empty("claude")
    },
    empty("antigravity"),
    empty("openclaw"),
];

/// Documents when the external discovery roots were last checked.
pub const EXTERNAL_ROOTS_VERIFIED: Verified = DISCOVERY;

/// Returns the profile of a registered target, if the table knows it.
#[must_use]
pub fn profile(target: &str) -> Option<&'static HarnessProfile> {
    PROFILES.iter().find(|profile| profile.target == target)
}

/// Returns every profile in table order.
#[must_use]
pub fn profiles() -> &'static [HarnessProfile] {
    &PROFILES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_target_has_a_profile() {
        for name in crate::target::target_names() {
            assert!(profile(&name).is_some(), "{name} has no profile");
        }
    }

    #[test]
    fn verified_values_name_version_and_date() {
        for profile in profiles() {
            for limit in [
                profile.description_cut,
                profile.listing_trigger_window,
                profile.body_delivery_budget,
                profile.read_budget,
            ]
            .into_iter()
            .flatten()
            {
                assert!(!limit.verified.harness_version.is_empty());
                assert!(!limit.verified.verified_on.is_empty());
            }
        }
    }

    #[test]
    fn listing_budget_conversion_is_table_driven() {
        let claude = profile("claude").unwrap().listing_budget.unwrap();
        let codex = profile("codex").unwrap().listing_budget.unwrap();
        for (budget, window, want) in [
            (claude, DEFAULT_CONTEXT_WINDOW_TOKENS, 8_000),
            (claude, 1_000_000, 40_000),
            (claude, 0, 0),
            (codex, DEFAULT_CONTEXT_WINDOW_TOKENS, 16_000),
            (codex, 1_000_000, 40_000),
            (codex, 10_000_000, 40_000),
        ] {
            assert_eq!(budget.chars(window), want, "window {window}");
        }
    }
}
