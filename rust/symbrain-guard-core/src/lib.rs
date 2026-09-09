//! Safe, deterministic Guard model and static policy kernel.
//!
//! This crate intentionally excludes approvals, grants, sequence state, audit,
//! discovery, spawning, and CLI dispatch. The Go Guard implementation remains
//! the production oracle until the full Guard matrix is green.

#![deny(unsafe_code)]

pub mod marginal;
pub mod model;
pub mod model_validation;
pub mod policy;
pub mod scope;
pub mod sequence;

pub use marginal::{
    RiskLevel, classify_risk, classify_risk_with_reason, marginal_capability_check,
    marginal_capability_reason,
};
pub use model::{
    ActionEvent, ActionState, AgentIdentity, ClientIdentity, ControlResponse, Decision,
    DecisionRequest, Evaluation, FailureMode, ModelError, NoDecision, SCHEMA_VERSION, SourceType,
    ToolCall, event_id, expired_at, new_no_decision, validate_control, validate_request,
    validate_tool_call,
};
pub use model_validation::{validate_action_event, validate_action_event_json};
pub use policy::{
    Bucket, Catalog, MatchCriteria, Options, PolicyError, Precedence, Result, Rule, RuleId, Version,
};
pub use sequence::{
    Config as SequenceConfig, Detector as SequenceDetector, Evaluation as SequenceEvaluation,
    Ledger as SequenceLedger, LedgerEntry as SequenceLedgerEntry,
    REASON_PREFIX as SEQUENCE_REASON_PREFIX,
};
