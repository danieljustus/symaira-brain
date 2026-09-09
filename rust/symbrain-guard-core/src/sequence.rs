//! Bounded repetition detection and outcome accounting for Guard events.
//!
//! This is the Rust counterpart of `guard/internal/sequence`. The Go
//! implementation remains the production oracle until the complete Guard
//! contract matrix is green.

#![allow(clippy::cast_precision_loss)]

use crate::model::{ActionEvent, ActionState, Decision};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Defaults used when configuration values are zero or negative.
pub const DEFAULT_THRESHOLD: i32 = 3;
pub const DEFAULT_WINDOW_SIZE: i32 = 100;
pub const DEFAULT_FUZZY_RATIO: f64 = 0.8;

/// Prefix identifying detector-generated reasons.
pub const REASON_PREFIX: &str = "sequence: ";

/// Detector configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    pub enabled: bool,
    pub threshold: i32,
    pub window_size: i32,
    pub fuzzy_ratio: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: DEFAULT_THRESHOLD,
            window_size: DEFAULT_WINDOW_SIZE,
            fuzzy_ratio: DEFAULT_FUZZY_RATIO,
        }
    }
}

impl Config {
    fn normalized(self) -> Self {
        Self {
            enabled: self.enabled,
            threshold: if self.threshold <= 0 {
                DEFAULT_THRESHOLD
            } else {
                self.threshold
            },
            window_size: if self.window_size <= 0 {
                DEFAULT_WINDOW_SIZE
            } else {
                self.window_size
            },
            fuzzy_ratio: if self.fuzzy_ratio <= 0.0 {
                DEFAULT_FUZZY_RATIO
            } else {
                self.fuzzy_ratio.min(1.0)
            },
        }
    }
}

/// Result of evaluating one action event.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    pub decision: Decision,
    pub reason: String,
    pub recoverable: bool,
    pub key: String,
    pub count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    key: String,
    tool: String,
    search: bool,
    terms: Vec<String>,
    count: i32,
    last_seq: u64,
}

/// Aggregated terminal outcomes for one tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    pub tool: String,
    pub success: i32,
    pub failed: i32,
    pub last_error: String,
}

/// Unbounded outcome summary maintained separately from the bounded window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ledger {
    entries: BTreeMap<String, LedgerEntry>,
}

impl Ledger {
    /// Creates an empty outcome ledger.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records only completed and failed events.
    pub fn record(&mut self, event: &ActionEvent) {
        let failed = match event.state {
            ActionState::Completed => !event.call.error.as_deref().unwrap_or_default().is_empty(),
            ActionState::Failed => true,
            _ => return,
        };
        let tool = event.call.tool.clone();
        let entry = self
            .entries
            .entry(tool.clone())
            .or_insert_with(|| LedgerEntry {
                tool,
                success: 0,
                failed: 0,
                last_error: String::new(),
            });
        if failed {
            entry.failed += 1;
            if let Some(error) = event.call.error.as_deref()
                && !error.is_empty()
            {
                entry.last_error = error.to_string();
            }
        } else {
            entry.success += 1;
        }
    }

    /// Returns entries sorted by tool name, matching Go's explicit sort.
    #[must_use]
    pub fn summarize(&self) -> Vec<LedgerEntry> {
        self.entries.values().cloned().collect()
    }
}

/// Stateful detector with a bounded LRU signature window.
#[derive(Debug, Clone)]
pub struct Detector {
    config: Config,
    sequence: u64,
    entries: BTreeMap<String, Entry>,
    ledger: Ledger,
}

impl Detector {
    /// Creates a detector and applies the Go defaults.
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            config: config.normalized(),
            sequence: 0,
            entries: BTreeMap::new(),
            ledger: Ledger::new(),
        }
    }

    /// Evaluates one event without mutating state while disabled.
    pub fn evaluate(&mut self, event: &ActionEvent) -> Evaluation {
        if !self.config.enabled {
            return Evaluation {
                decision: Decision::Allow,
                reason: format!("{REASON_PREFIX}disabled"),
                recoverable: false,
                key: String::new(),
                count: 0,
            };
        }

        self.ledger.record(event);
        if !matches!(event.state, ActionState::Requested | ActionState::Started) {
            return Evaluation {
                decision: Decision::Allow,
                reason: format!(
                    "{REASON_PREFIX}not an attempt (state {})",
                    state_name(&event.state)
                ),
                recoverable: false,
                key: String::new(),
                count: 0,
            };
        }

        let tool = &event.call.tool;
        let search = is_search_tool(tool);
        let (key, terms) = self.key_for(tool, event.call.args.as_ref(), search);
        self.sequence += 1;

        if let Some(entry) = self.entries.get_mut(&key) {
            entry.count += 1;
            entry.last_seq = self.sequence;
            let count = entry.count;
            let is_search = entry.search;
            if count >= self.config.threshold {
                return Evaluation {
                    decision: Decision::Deny,
                    reason: deny_reason(tool, is_search, count),
                    recoverable: true,
                    key,
                    count,
                };
            }
            return allowed_evaluation(key, count);
        }

        self.entries.insert(
            key.clone(),
            Entry {
                key: key.clone(),
                tool: tool.clone(),
                search,
                terms,
                count: 1,
                last_seq: self.sequence,
            },
        );
        self.evict();
        allowed_evaluation(key, 1)
    }

    /// Returns the detector's terminal-outcome ledger.
    #[must_use]
    pub const fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// Number of signatures currently retained in the bounded window.
    #[must_use]
    pub fn window_len(&self) -> usize {
        self.entries.len()
    }

    fn key_for(&self, tool: &str, args: Option<&Value>, search: bool) -> (String, Vec<String>) {
        if !search {
            let input = canonicalize_input(args);
            return (exact_key(tool, &input), Vec::new());
        }
        let terms = tokenize(&canonicalize_input(args));
        (self.fuzzy_key(tool, &terms), terms)
    }

    fn fuzzy_key(&self, tool: &str, terms: &[String]) -> String {
        if terms.is_empty() {
            return search_key(tool, terms);
        }
        let mut best: Option<&Entry> = None;
        let mut best_overlap = 0.0;
        for entry in self.entries.values() {
            if !entry.search || entry.tool != tool || entry.terms.is_empty() {
                continue;
            }
            let overlap = term_overlap(&entry.terms, terms);
            if overlap >= self.config.fuzzy_ratio
                && (best.is_none()
                    || overlap > best_overlap
                    || (overlap.total_cmp(&best_overlap).is_eq()
                        && entry.count > best.expect("best is checked above").count))
            {
                best = Some(entry);
                best_overlap = overlap;
            }
        }
        best.map_or_else(|| search_key(tool, terms), |entry| entry.key.clone())
    }

    fn window_size(&self) -> usize {
        usize::try_from(self.config.window_size).expect("normalized window size is positive")
    }

    fn evict(&mut self) {
        while self.entries.len() > self.window_size() {
            let victim = self
                .entries
                .values()
                .min_by_key(|entry| entry.last_seq)
                .map(|entry| entry.key.clone());
            if let Some(key) = victim {
                self.entries.remove(&key);
            } else {
                break;
            }
        }
    }
}

fn allowed_evaluation(key: String, count: i32) -> Evaluation {
    Evaluation {
        decision: Decision::Allow,
        reason: format!("{REASON_PREFIX}no repetition detected"),
        recoverable: false,
        key,
        count,
    }
}

fn deny_reason(tool: &str, search: bool, count: i32) -> String {
    let kind = if search {
        "fuzzy search duplicate"
    } else {
        "exact repetition"
    };
    format!("{REASON_PREFIX}{kind} of tool {tool:?} ({count} calls in window)")
}

fn state_name(state: &ActionState) -> &'static str {
    match state {
        ActionState::Requested => "requested",
        ActionState::Approved => "approved",
        ActionState::Denied => "denied",
        ActionState::Started => "started",
        ActionState::Completed => "completed",
        ActionState::Failed => "failed",
    }
}

fn exact_key(tool: &str, input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    format!("exact:{tool}\0{digest:x}")
}

fn search_key(tool: &str, terms: &[String]) -> String {
    format!("search:{tool}\0{}", terms.join("\0"))
}

fn canonicalize_input(args: Option<&Value>) -> String {
    let Some(value) = args else {
        return String::new();
    };
    serde_json::to_string(&canonical_value(value)).expect("serde_json::Value is serializable")
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut canonical = Map::new();
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for key in keys {
                canonical.insert(key.clone(), canonical_value(&object[key]));
            }
            Value::Object(canonical)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_value).collect()),
        other => other.clone(),
    }
}

fn tokenize(input: &str) -> Vec<String> {
    let lower = input.to_lowercase();
    let mut terms = Vec::new();
    for token in lower.split(|character: char| !character.is_alphanumeric()) {
        if token.len() >= 2 {
            terms.push(token.to_string());
        }
    }
    terms
}

fn term_overlap(left: &[String], right: &[String]) -> f64 {
    let left = left.iter().collect::<BTreeSet<_>>();
    let right = right.iter().collect::<BTreeSet<_>>();
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }
    let intersection = left.intersection(&right).count();
    let union = left.len() + right.len() - intersection;
    if union == 0 {
        1.0
    } else {
        intersection as f64 / union as f64
    }
}

fn is_search_tool(tool: &str) -> bool {
    let lower = tool.to_lowercase();
    lower.contains("search") || lower.contains("find")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AgentIdentity, ClientIdentity, SourceType, ToolCall};

    fn event(tool: &str, args: Value, state: ActionState) -> ActionEvent {
        ActionEvent {
            id: "evt_1_proxy_test".to_string(),
            schema_version: 1,
            source: SourceType::Proxy,
            prev_event_id: None,
            agent: AgentIdentity {
                agent_id: "test-agent".to_string(),
                ..AgentIdentity::default()
            },
            client: ClientIdentity::default(),
            call: ToolCall {
                server: "test-server".to_string(),
                tool: tool.to_string(),
                args: Some(args),
                ..ToolCall::default()
            },
            state,
            evaluation: None,
            control_response: None,
            timestamp: "2026-09-09T00:00:00Z".to_string(),
        }
    }

    fn attempt(tool: &str, args: Value) -> ActionEvent {
        event(tool, args, ActionState::Requested)
    }

    fn decisions(detector: &mut Detector, events: &[ActionEvent]) -> Vec<Decision> {
        events
            .iter()
            .map(|event| detector.evaluate(event).decision)
            .collect()
    }

    #[test]
    fn exact_repetition_blocks_at_default_threshold() {
        let mut detector = Detector::new(Config {
            enabled: true,
            ..Config::default()
        });
        let events = (0..3)
            .map(|_| attempt("read_file", serde_json::json!({"path": "a"})))
            .collect::<Vec<_>>();
        assert_eq!(
            decisions(&mut detector, &events),
            vec![Decision::Allow, Decision::Allow, Decision::Deny]
        );
    }

    #[test]
    fn threshold_and_tool_are_part_of_exact_contract() {
        let mut detector = Detector::new(Config {
            enabled: true,
            threshold: 2,
            ..Config::default()
        });
        let events = vec![
            attempt("read_file", serde_json::json!("a")),
            attempt("write_file", serde_json::json!("a")),
            attempt("read_file", serde_json::json!("a")),
        ];
        assert_eq!(
            decisions(&mut detector, &events),
            vec![Decision::Allow, Decision::Allow, Decision::Deny]
        );
    }

    #[test]
    fn canonicalizes_object_key_order() {
        let mut detector = Detector::new(Config {
            enabled: true,
            ..Config::default()
        });
        let events = vec![
            attempt(
                "read_file",
                serde_json::json!({"path": "/tmp/x", "recursive": true}),
            ),
            attempt(
                "read_file",
                serde_json::json!({"recursive": true, "path": "/tmp/x"}),
            ),
            attempt(
                "read_file",
                serde_json::json!({"path": "/tmp/x", "recursive": true}),
            ),
        ];
        assert_eq!(
            decisions(&mut detector, &events),
            vec![Decision::Allow, Decision::Allow, Decision::Deny]
        );
    }

    #[test]
    fn fuzzy_search_and_find_tools_deduplicate_queries() {
        let mut detector = Detector::new(Config {
            enabled: true,
            ..Config::default()
        });
        let events = vec![
            attempt(
                "search_web",
                serde_json::json!("find notes about project alpha"),
            ),
            attempt(
                "search_web",
                serde_json::json!("find notes about project alpha please"),
            ),
            attempt(
                "search_web",
                serde_json::json!("find notes about project alpha"),
            ),
        ];
        assert_eq!(
            decisions(&mut detector, &events),
            vec![Decision::Allow, Decision::Allow, Decision::Deny]
        );

        let mut find_detector = Detector::new(Config {
            enabled: true,
            ..Config::default()
        });
        let events = vec![
            attempt(
                "find_files",
                serde_json::json!("please locate the todo list"),
            ),
            attempt(
                "find_files",
                serde_json::json!("please locate the todo list now"),
            ),
            attempt(
                "find_files",
                serde_json::json!("please locate the todo list"),
            ),
        ];
        assert_eq!(
            decisions(&mut find_detector, &events),
            vec![Decision::Allow, Decision::Allow, Decision::Deny]
        );
    }

    #[test]
    fn window_eviction_forgets_least_recent_signature() {
        let mut detector = Detector::new(Config {
            enabled: true,
            window_size: 2,
            ..Config::default()
        });
        let events = vec![
            attempt("read_file", serde_json::json!("a")),
            attempt("read_file", serde_json::json!("b")),
            attempt("read_file", serde_json::json!("c")),
            attempt("read_file", serde_json::json!("a")),
            attempt("read_file", serde_json::json!("a")),
            attempt("read_file", serde_json::json!("a")),
        ];
        assert_eq!(
            decisions(&mut detector, &events),
            vec![
                Decision::Allow,
                Decision::Allow,
                Decision::Allow,
                Decision::Allow,
                Decision::Allow,
                Decision::Deny
            ]
        );
        assert_eq!(detector.window_len(), 2);
    }

    #[test]
    fn non_positive_window_size_uses_default() {
        let mut detector = Detector::new(Config {
            enabled: true,
            window_size: -1,
            ..Config::default()
        });
        let event = attempt("read_file", serde_json::json!("a"));
        detector.evaluate(&event);
        detector.evaluate(&event);
        assert_eq!(detector.evaluate(&event).decision, Decision::Deny);
    }

    #[test]
    fn disabled_detector_does_not_record() {
        let mut detector = Detector::new(Config::default());
        let event = attempt("read_file", serde_json::json!("a"));
        assert_eq!(detector.evaluate(&event).decision, Decision::Allow);
        assert_eq!(detector.evaluate(&event).reason, "sequence: disabled");
        assert!(detector.ledger().summarize().is_empty());
    }

    #[test]
    fn only_attempt_states_count_and_denies_are_recoverable() {
        let mut detector = Detector::new(Config {
            enabled: true,
            ..Config::default()
        });
        let mut completed = event("read_file", serde_json::json!("a"), ActionState::Completed);
        completed.call.error = Some("boom".to_string());
        let events = vec![
            attempt("read_file", serde_json::json!("a")),
            event("read_file", serde_json::json!("a"), ActionState::Approved),
            event("read_file", serde_json::json!("a"), ActionState::Started),
            completed,
            attempt("read_file", serde_json::json!("a")),
        ];
        assert_eq!(
            decisions(&mut detector, &events),
            vec![
                Decision::Allow,
                Decision::Allow,
                Decision::Allow,
                Decision::Allow,
                Decision::Deny
            ]
        );

        let mut deny_detector = Detector::new(Config {
            enabled: true,
            ..Config::default()
        });
        let repeated = attempt("read_file", serde_json::json!("a"));
        deny_detector.evaluate(&repeated);
        deny_detector.evaluate(&repeated);
        let third = deny_detector.evaluate(&repeated);
        assert_eq!(third.decision, Decision::Deny);
        assert!(third.recoverable);
        assert!(third.reason.starts_with(REASON_PREFIX));
        assert_eq!(third.count, 3);
    }

    #[test]
    fn ledger_summarizes_successes_failures_and_last_error_sorted() {
        let mut ledger = Ledger::new();
        ledger.record(&event(
            "write_file",
            serde_json::json!("w"),
            ActionState::Completed,
        ));
        let mut failed = event("read_file", serde_json::json!("a"), ActionState::Failed);
        failed.call.error = Some("timeout".to_string());
        ledger.record(&failed);
        ledger.record(&event(
            "read_file",
            serde_json::json!("b"),
            ActionState::Completed,
        ));
        assert_eq!(
            ledger.summarize(),
            vec![
                LedgerEntry {
                    tool: "read_file".to_string(),
                    success: 1,
                    failed: 1,
                    last_error: "timeout".to_string()
                },
                LedgerEntry {
                    tool: "write_file".to_string(),
                    success: 1,
                    failed: 0,
                    last_error: String::new()
                },
            ]
        );
    }
}
