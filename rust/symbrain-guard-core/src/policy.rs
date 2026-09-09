//! Deterministic static rule buckets, precedence, and composition.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::missing_errors_doc,
    clippy::too_many_lines,
    clippy::trivially_copy_pass_by_ref
)]

use crate::model::{Decision, RuleTraceEntry, ToolCall};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Stable rule identifier.
pub type RuleId = String;

/// Rule version string.
pub type Version = String;

/// Lower numbers are evaluated first within each bucket.
pub type Precedence = i32;

/// A validated policy rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: RuleId,
    pub version: Version,
    pub precedence: Precedence,
    pub decision: Decision,
    pub r#match: MatchCriteria,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub observe_only: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Criteria are conjunctive; every non-empty criterion must match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MatchCriteria {
    #[serde(skip_serializing_if = "is_empty")]
    pub server: String,
    #[serde(skip_serializing_if = "is_empty")]
    pub tool: String,
    #[serde(skip_serializing_if = "is_empty")]
    pub capability: String,
    #[serde(skip_serializing_if = "is_empty")]
    pub remote: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub command_contains: Vec<String>,
}

fn is_empty(value: &str) -> bool {
    value.is_empty()
}

/// Fixed evaluation bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bucket {
    Deny,
    Require,
    Allow,
}

impl Bucket {
    /// Returns the bucket implied by a decision.
    #[must_use]
    pub const fn for_decision(decision: &Decision) -> Self {
        match decision {
            Decision::Deny => Self::Deny,
            Decision::Require => Self::Require,
            _ => Self::Allow,
        }
    }
}

/// Structured policy result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Result {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<Rule>,
    pub decision: Decision,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub matched: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub precedence: Precedence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket: Option<Bucket>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trace: Vec<RuleTraceEntry>,
}

fn is_zero(value: &i32) -> bool {
    *value == 0
}

/// Optional diagnostics. Tracing is never part of the control response.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    pub trace: bool,
}

/// A validated catalog of static rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub rules: Vec<Rule>,
    pub version: Version,
}

/// Catalog validation errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyError(String);

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PolicyError {}

impl Catalog {
    /// Creates a catalog after applying the same structural checks as Go.
    pub fn new(
        rules: Vec<Rule>,
        version: impl Into<String>,
    ) -> std::result::Result<Self, PolicyError> {
        let catalog = Self {
            rules,
            version: version.into(),
        };
        catalog.validate()?;
        Ok(catalog)
    }

    /// Validates unique IDs, decisions, criteria, and increasing precedence.
    pub fn validate(&self) -> std::result::Result<(), PolicyError> {
        let mut seen = std::collections::BTreeSet::new();
        let mut previous = -1;
        for (index, rule) in self.rules.iter().enumerate() {
            if rule.id.is_empty() {
                return Err(PolicyError(format!("rules[{index}]: empty rule ID")));
            }
            if !seen.insert(&rule.id) {
                return Err(PolicyError(format!(
                    "rules[{index}]: duplicate rule ID {:?}",
                    rule.id
                )));
            }
            if !has_criteria(&rule.r#match) {
                return Err(PolicyError(format!(
                    "rules[{index}] {:?}: match must specify at least one criterion",
                    rule.id
                )));
            }
            if rule.precedence <= previous {
                return Err(PolicyError(format!(
                    "rules[{index}] {:?}: precedence {} not strictly greater than previous {}",
                    rule.id, rule.precedence, previous
                )));
            }
            previous = rule.precedence;
        }
        Ok(())
    }

    /// Validates a catalog JSON value while preserving Go's validation order.
    ///
    /// A typed `Catalog` cannot retain an unknown decision value, but the Go
    /// model reports that value as a rule validation error rather than a wire
    /// decode error. This entrypoint retains the untyped decision long enough
    /// to preserve that fail-closed public diagnostic.
    pub fn validate_json(value: &Value) -> std::result::Result<(), PolicyError> {
        let catalog: WireCatalog = serde_json::from_value(value.clone())
            .map_err(|error| PolicyError(error.to_string()))?;
        let mut seen = std::collections::BTreeSet::new();
        let mut previous = -1;

        for (index, rule) in catalog.rules.iter().enumerate() {
            if rule.id.is_empty() {
                return Err(PolicyError(format!("rules[{index}]: empty rule ID")));
            }
            if !seen.insert(&rule.id) {
                return Err(PolicyError(format!(
                    "rules[{index}]: duplicate rule ID {:?}",
                    rule.id
                )));
            }
            Decision::validate(&rule.decision)
                .map_err(|error| PolicyError(format!("rules[{index}] {:?}: {error}", rule.id)))?;
            if !has_criteria(&rule.r#match) {
                return Err(PolicyError(format!(
                    "rules[{index}] {:?}: match must specify at least one criterion",
                    rule.id
                )));
            }
            if rule.precedence <= previous {
                return Err(PolicyError(format!(
                    "rules[{index}] {:?}: precedence {} not strictly greater than previous {}",
                    rule.id, rule.precedence, previous
                )));
            }
            previous = rule.precedence;
        }
        Ok(())
    }

    /// Evaluates deny, require, then allow buckets in that fixed order.
    #[must_use]
    pub fn evaluate(&self, call: &ToolCall, default: Decision) -> Result {
        self.evaluate_with_options(call, default, Options::default())
    }

    /// Evaluates a catalog and optionally returns one trace row per rule.
    #[must_use]
    pub fn evaluate_with_options(
        &self,
        call: &ToolCall,
        default: Decision,
        options: Options,
    ) -> Result {
        let mut trace = Vec::new();
        let mut deny_match = None;
        let mut deny_error = None;
        let mut require_bad = None;
        let mut require_error = None;
        let mut allow_match = None;

        for rule in &self.rules {
            let bucket = Bucket::for_decision(&rule.decision);
            let (matched, error) = matches(&rule.r#match, call);
            if options.trace {
                trace.push(RuleTraceEntry {
                    rule_id: rule.id.clone(),
                    matched,
                    decision: rule.decision.clone(),
                    bucket: bucket.to_string(),
                });
            }
            match bucket {
                Bucket::Deny => match error {
                    Some(_) if deny_error.is_none() => deny_error = Some(rule.clone()),
                    None if matched && deny_match.is_none() => deny_match = Some(rule.clone()),
                    _ => {}
                },
                Bucket::Require => match error {
                    Some(_) if require_error.is_none() => require_error = Some(rule.clone()),
                    None if !matched && require_bad.is_none() => require_bad = Some(rule.clone()),
                    _ => {}
                },
                Bucket::Allow => {
                    if error.is_none() && matched && allow_match.is_none() {
                        allow_match = Some(rule.clone());
                    }
                }
            }
        }

        if let Some(rule) = deny_match {
            return result(
                Some(rule.clone()),
                Decision::Deny,
                rule.reason,
                true,
                rule.precedence,
                Some(Bucket::Deny),
                trace,
            );
        }
        if let Some(rule) = deny_error {
            let reason = format!("deny rule {:?} could not be evaluated", rule.id);
            return result(
                Some(rule.clone()),
                Decision::Deny,
                Some(reason),
                false,
                rule.precedence,
                Some(Bucket::Deny),
                trace,
            );
        }
        if let Some(rule) = require_bad {
            let reason = format!("require rule {:?} did not hold", rule.id);
            return result(
                Some(rule.clone()),
                Decision::Deny,
                Some(reason),
                false,
                rule.precedence,
                Some(Bucket::Require),
                trace,
            );
        }
        if let Some(rule) = require_error {
            let reason = format!("require rule {:?} could not be evaluated", rule.id);
            return result(
                Some(rule.clone()),
                Decision::Deny,
                Some(reason),
                false,
                rule.precedence,
                Some(Bucket::Require),
                trace,
            );
        }
        if let Some(rule) = allow_match {
            return result(
                Some(rule.clone()),
                rule.decision.clone(),
                rule.reason,
                true,
                rule.precedence,
                Some(Bucket::Allow),
                trace,
            );
        }
        result(
            None,
            default,
            Some("default capability decision".into()),
            false,
            0,
            None,
            trace,
        )
    }

    /// Concatenates catalogs, renumbers precedence, and revalidates IDs.
    pub fn merge(&self, other: &Self) -> std::result::Result<Self, PolicyError> {
        let mut rules = self.rules.clone();
        rules.extend(other.rules.clone());
        for (index, rule) in rules.iter_mut().enumerate() {
            rule.precedence = (index + 1) as i32;
        }
        Self::new(rules, self.version.clone())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireCatalog {
    rules: Vec<WireRule>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireRule {
    id: RuleId,
    precedence: Precedence,
    decision: String,
    r#match: MatchCriteria,
}

fn result(
    rule: Option<Rule>,
    decision: Decision,
    reason: Option<String>,
    matched: bool,
    precedence: i32,
    bucket: Option<Bucket>,
    trace: Vec<RuleTraceEntry>,
) -> Result {
    Result {
        rule,
        decision,
        reason,
        matched,
        precedence,
        bucket,
        trace,
    }
}

fn has_criteria(criteria: &MatchCriteria) -> bool {
    !criteria.server.is_empty()
        || !criteria.tool.is_empty()
        || !criteria.capability.is_empty()
        || !criteria.remote.is_empty()
        || !criteria.command_contains.is_empty()
}

fn matches(criteria: &MatchCriteria, call: &ToolCall) -> (bool, Option<String>) {
    if !criteria.server.is_empty() && criteria.server != call.server {
        return (false, None);
    }
    if !criteria.tool.is_empty() && criteria.tool != call.tool {
        return (false, None);
    }
    if !criteria.capability.is_empty()
        && Some(criteria.capability.as_str()) != call.capability.as_deref()
    {
        return (false, None);
    }
    if !criteria.remote.is_empty() && Some(criteria.remote.as_str()) != call.remote.as_deref() {
        return (false, None);
    }
    if !criteria.command_contains.is_empty() {
        let Some(args) = call.args.as_ref().and_then(Value::as_str) else {
            let kind = call.args.as_ref().map_or("<nil>", json_type);
            return (
                false,
                Some(format!(
                    "command_contains match requires string args, got {kind}"
                )),
            );
        };
        if !criteria
            .command_contains
            .iter()
            .any(|part| args.contains(part))
        {
            return (false, None);
        }
    }
    (true, None)
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "<nil>",
        Value::Bool(_) => "bool",
        Value::Number(_) => "float64",
        Value::String(_) => "string",
        Value::Array(_) => "[]interface {}",
        Value::Object(_) => "map[string]interface {}",
    }
}

impl fmt::Display for Bucket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Deny => f.write_str("deny"),
            Self::Require => f.write_str("require"),
            Self::Allow => f.write_str("allow"),
        }
    }
}
