//! Capability-token scope ceiling for static policy results.

use crate::model::Decision;
use crate::policy::Result;

/// Narrows a non-deny result to the token's granted capability scope.
#[must_use]
pub fn scope_ceiling(scope: &[String], capability: &str, result: Result) -> Result {
    if result.decision == Decision::Deny {
        return result;
    }
    if capability.is_empty()
        || !scope
            .iter()
            .any(|entry| entry == "*" || entry == capability)
    {
        return Result {
            rule: None,
            decision: Decision::Deny,
            reason: Some(format!(
                "capability {capability:?} not granted by token scope"
            )),
            matched: false,
            precedence: 0,
            bucket: None,
            trace: Vec::new(),
        };
    }
    result
}
