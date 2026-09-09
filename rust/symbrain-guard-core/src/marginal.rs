//! Static capability effects, marginal-capability capping, and risk reasons.

use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapabilityEffect {
    ReadPublic,
    ReadPrivate,
    ReadSecret,
    WriteFile,
    Shell,
    Network,
    Browser,
    CredentialUse,
    Deploy,
    Destructive,
}

fn effect(capability: &str) -> Option<CapabilityEffect> {
    match capability {
        "read_public" => Some(CapabilityEffect::ReadPublic),
        "read_private" => Some(CapabilityEffect::ReadPrivate),
        "read_secret" => Some(CapabilityEffect::ReadSecret),
        "write_file" => Some(CapabilityEffect::WriteFile),
        "shell" => Some(CapabilityEffect::Shell),
        "network" => Some(CapabilityEffect::Network),
        "browser" => Some(CapabilityEffect::Browser),
        "credential_use" => Some(CapabilityEffect::CredentialUse),
        "deploy" => Some(CapabilityEffect::Deploy),
        "destructive" => Some(CapabilityEffect::Destructive),
        _ => None,
    }
}

fn encompasses(allowed: CapabilityEffect, requested: CapabilityEffect) -> bool {
    allowed == requested
        || matches!(
            (allowed, requested),
            (
                CapabilityEffect::Shell | CapabilityEffect::Network,
                CapabilityEffect::ReadPublic
            ) | (
                CapabilityEffect::Shell,
                CapabilityEffect::ReadPrivate
                    | CapabilityEffect::WriteFile
                    | CapabilityEffect::Network
            ) | (
                CapabilityEffect::Deploy | CapabilityEffect::Destructive,
                CapabilityEffect::WriteFile
            ) | (CapabilityEffect::Deploy, CapabilityEffect::Network)
                | (CapabilityEffect::Destructive, CapabilityEffect::Deploy)
        )
}

/// True when an already-allowed capability provides the same effect.
#[must_use]
pub fn marginal_capability_check(tool: &str, already_allowed: &[(String, bool)]) -> bool {
    let Some(requested) = effect(tool) else {
        return false;
    };
    already_allowed
        .iter()
        .filter(|(_, allowed)| *allowed)
        .filter_map(|(capability, _)| effect(capability))
        .any(|allowed| encompasses(allowed, requested))
}

/// Stable explanation for a marginal-capability cap.
#[must_use]
pub fn marginal_capability_reason(
    tool: &str,
    already_allowed: &[(String, bool)],
) -> Option<String> {
    let mut names: Vec<&str> = already_allowed
        .iter()
        .filter(|(_, allowed)| *allowed)
        .map(|(capability, _)| capability.as_str())
        .collect();
    names.sort_unstable();
    names.into_iter().find_map(|name| {
        let pair = [(name.to_string(), true)];
        marginal_capability_check(tool, &pair)
            .then(|| format!("no marginal capability over already-allowed tool: {name}"))
    })
}

/// Static risk level for a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Unknown,
    Low,
    Medium,
    High,
    Critical,
}

impl Serialize for RiskLevel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i32(match self {
            Self::Unknown => 0,
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
            Self::Critical => 4,
        })
    }
}

fn static_risk(capability: &str) -> RiskLevel {
    match capability {
        "read_public" => RiskLevel::Low,
        "read_private" | "write_file" | "network" => RiskLevel::Medium,
        "shell" | "browser" | "credential_use" => RiskLevel::High,
        "read_secret" | "deploy" | "destructive" => RiskLevel::Critical,
        _ => RiskLevel::Unknown,
    }
}

/// Classifies risk and caps it to low only for a marginal capability.
#[must_use]
pub fn classify_risk(capability: &str, marginal: bool) -> RiskLevel {
    let base = static_risk(capability);
    if marginal && !matches!(base, RiskLevel::Unknown | RiskLevel::Low) {
        RiskLevel::Low
    } else {
        base
    }
}

/// Classifies risk and returns the stable marginal reason when applicable.
#[must_use]
pub fn classify_risk_with_reason(
    capability: &str,
    marginal: bool,
    already_allowed: &[(String, bool)],
) -> (RiskLevel, Option<String>) {
    (
        classify_risk(capability, marginal),
        marginal
            .then(|| marginal_capability_reason(capability, already_allowed))
            .flatten(),
    )
}
