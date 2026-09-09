#![allow(clippy::too_many_lines)]

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use symbrain_guard_core::{
    Catalog, Decision, DecisionRequest, FailureMode, SourceType, classify_risk_with_reason,
    event_id, expired_at, marginal_capability_check, marginal_capability_reason, new_no_decision,
    validate_action_event_json, validate_request,
};
use symbrain_guard_core::{MatchCriteria, PolicyError, Result as PolicyResult, Rule, ToolCall};

#[derive(Debug, Deserialize)]
struct Suite {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    kind: String,
    input: Value,
    success: bool,
    #[serde(default)]
    output_json: String,
    #[serde(default)]
    error: String,
}

#[derive(Debug, Deserialize)]
struct PolicyInput {
    rules: Vec<Rule>,
    call: ToolCall,
    default: Decision,
    trace: bool,
}

#[derive(Debug, Deserialize)]
struct MergeInput {
    left: Catalog,
    right: Catalog,
}

#[derive(Debug, Deserialize)]
struct ScopeInput {
    scope: Vec<String>,
    capability: String,
    result: PolicyResult,
}

#[derive(Debug, Deserialize)]
struct MarginalInput {
    tool: String,
    already_allowed: BTreeMap<String, bool>,
}

#[derive(Debug, Deserialize)]
struct ExpiryInput {
    deadline: String,
    now: String,
}

#[derive(Debug, Deserialize)]
struct NoDecisionInput {
    failure_mode: String,
    diagnostic: String,
}

#[derive(Debug, serde::Serialize)]
struct MarginalOutput {
    marginal: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    reason: String,
    risk: symbrain_guard_core::RiskLevel,
    #[serde(skip_serializing_if = "String::is_empty")]
    risk_reason: String,
}

fn serialize<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("serialize oracle result")
}

fn assert_case(case: &Case, actual: std::result::Result<String, String>) {
    match (case.success, actual) {
        (true, Ok(output)) => assert_eq!(output, case.output_json, "case {}", case.id),
        (false, Err(error)) => assert_eq!(error, case.error, "case {}", case.id),
        (true, Err(error)) => panic!("case {}: unexpected error: {error}", case.id),
        (false, Ok(output)) => panic!(
            "case {}: expected error {:?}, got {output}",
            case.id, case.error
        ),
    }
}

fn evaluate_case(case: &Case) -> std::result::Result<String, String> {
    match case.kind.as_str() {
        "validate_source" => {
            let value: String =
                serde_json::from_value(case.input.clone()).map_err(|e| e.to_string())?;
            SourceType::validate(&value)
                .map(|_| serialize(&true))
                .map_err(|e| e.to_string())
        }
        "validate_decision" => {
            let value: String =
                serde_json::from_value(case.input.clone()).map_err(|e| e.to_string())?;
            Decision::validate(&value)
                .map(|_| serialize(&true))
                .map_err(|e| e.to_string())
        }
        "no_decision" => {
            let input: NoDecisionInput =
                serde_json::from_value(case.input.clone()).map_err(|e| e.to_string())?;
            let failure = match input.failure_mode.as_str() {
                "allow" => FailureMode::Allow,
                "deny" => FailureMode::Deny,
                other => FailureMode::Unknown(other.to_owned()),
            };
            Ok(serialize(
                &new_no_decision(Some(&failure), input.diagnostic).control,
            ))
        }
        "event_id" => {
            let source = SourceType::validate(
                case.input
                    .get("source")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "missing source".to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let counter = case
                .input
                .get("counter")
                .and_then(Value::as_i64)
                .ok_or_else(|| "missing counter".to_string())?;
            Ok(serialize(&event_id(&source, counter)))
        }
        "expired" => {
            let input: ExpiryInput =
                serde_json::from_value(case.input.clone()).map_err(|e| e.to_string())?;
            expired_at(Some(&input.deadline), &input.now)
                .map(|value| serialize(&value))
                .map_err(|e| e.to_string())
        }
        "validate_request" => {
            let input: DecisionRequest =
                serde_json::from_value(case.input.clone()).map_err(|e| e.to_string())?;
            validate_request(&input)
                .map(|()| serialize(&true))
                .map_err(|e| e.to_string())
        }
        "validate_event" => validate_action_event_json(&case.input)
            .map(|()| serialize(&true))
            .map_err(|e| e.to_string()),
        "catalog_validate" => Catalog::validate_json(&case.input)
            .map(|()| serialize(&true))
            .map_err(|e| e.to_string()),
        "evaluate" => {
            let input: PolicyInput =
                serde_json::from_value(case.input.clone()).map_err(|e| e.to_string())?;
            let catalog = Catalog::new(input.rules, "1.0.0").map_err(|e| e.to_string())?;
            let result = if input.trace {
                catalog.evaluate_with_options(
                    &input.call,
                    input.default,
                    symbrain_guard_core::Options { trace: true },
                )
            } else {
                catalog.evaluate(&input.call, input.default)
            };
            Ok(serialize(&result))
        }
        "merge" => {
            let input: MergeInput =
                serde_json::from_value(case.input.clone()).map_err(|e| e.to_string())?;
            input
                .left
                .merge(&input.right)
                .map(|value| serialize(&value))
                .map_err(|e| e.to_string())
        }
        "scope_ceiling" => {
            let input: ScopeInput =
                serde_json::from_value(case.input.clone()).map_err(|e| e.to_string())?;
            let result = symbrain_guard_core::scope::scope_ceiling(
                &input.scope,
                &input.capability,
                input.result,
            );
            Ok(serialize(&result))
        }
        "marginal" => {
            let input: MarginalInput =
                serde_json::from_value(case.input.clone()).map_err(|e| e.to_string())?;
            let allowed: Vec<(String, bool)> = input.already_allowed.into_iter().collect();
            let marginal = marginal_capability_check(&input.tool, &allowed);
            let reason = marginal_capability_reason(&input.tool, &allowed);
            let (risk, risk_reason) = classify_risk_with_reason(&input.tool, marginal, &allowed);
            let output = MarginalOutput {
                marginal,
                reason: reason.unwrap_or_default(),
                risk,
                risk_reason: risk_reason.unwrap_or_default(),
            };
            Ok(serialize(&output))
        }
        other => Err(format!("unknown oracle kind {other}")),
    }
}

#[test]
fn guard_core_matches_go_oracle_bytes() {
    let suite: Suite = serde_json::from_slice(include_bytes!("fixtures/oracle_expectations.json"))
        .expect("parse Guard oracle fixture");
    for case in &suite.cases {
        assert_case(case, evaluate_case(case));
    }
}

#[test]
fn static_scope_kernel_rejects_missing_capability() {
    let result = PolicyResult {
        rule: None,
        decision: Decision::Allow,
        reason: None,
        matched: true,
        precedence: 0,
        bucket: None,
        trace: Vec::new(),
    };
    let narrowed = symbrain_guard_core::scope::scope_ceiling(&[], "shell", result);
    assert_eq!(narrowed.decision, Decision::Deny);
}

#[test]
fn policy_match_criteria_is_conjunctive() {
    let rule = Rule {
        id: "r1".into(),
        version: "1.0.0".into(),
        precedence: 1,
        decision: Decision::Allow,
        r#match: MatchCriteria {
            server: "s".into(),
            tool: "t".into(),
            ..MatchCriteria::default()
        },
        reason: None,
        observe_only: false,
    };
    let catalog = Catalog::new(vec![rule], "1.0.0").expect("valid catalog");
    let result = catalog.evaluate(
        &ToolCall {
            server: "s".into(),
            tool: "other".into(),
            ..ToolCall::default()
        },
        Decision::Deny,
    );
    assert!(!result.matched);
    assert_eq!(result.decision, Decision::Deny);
}

#[test]
fn catalog_json_validation_preserves_unknown_decision_error() {
    let error = Catalog::validate_json(&serde_json::json!({
        "rules": [{
            "id": "bad-decision",
            "precedence": 10,
            "decision": "future",
            "match": {"server": "filesystem"}
        }],
        "version": "1.0.0"
    }))
    .expect_err("unknown decisions must fail closed");
    assert_eq!(
        error.to_string(),
        "rules[0] \"bad-decision\": model: unknown decision \"future\""
    );
}

#[test]
fn fail_closed_control_has_no_reason_bytes() {
    let output = serialize(&new_no_decision(None, "transport unavailable").control);
    assert_eq!(output, r#"{"decision":"deny"}"#);
}

#[allow(dead_code)]
fn _keep_policy_error_public(_: PolicyError) {}
