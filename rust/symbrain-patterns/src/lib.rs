//! Recurring gateway-session pattern detection and private episode storage.

#![deny(unsafe_code)]

mod store;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use store::Store;

/// One names-only routed tool invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub server: String,
    pub tool: String,
}

/// Ordered tool-call sequence from one completed gateway session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    pub profile: String,
    pub steps: Vec<Step>,
    pub started_at: String,
    pub ended_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub profile: String,
    pub first_step: Step,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub recurrence_count: usize,
}

/// Reviewable promoted recurring sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pattern {
    pub name: String,
    pub version: u32,
    pub profile: String,
    pub steps: Vec<Step>,
    pub trigger: Trigger,
    pub provenance: Provenance,
}

fn sequence_key(profile: &str, steps: &[Step]) -> Vec<u8> {
    let mut key = Vec::new();
    key.extend_from_slice(profile.as_bytes());
    key.push(0);
    for step in steps {
        key.extend_from_slice(step.server.as_bytes());
        key.push(b'/');
        key.extend_from_slice(step.tool.as_bytes());
        key.push(0);
    }
    key
}

/// Derives the deterministic Go-compatible human-readable pattern name.
#[must_use]
pub fn name(profile: &str, steps: &[Step]) -> String {
    let mut parts = vec![profile.to_string()];
    parts.extend(
        steps
            .iter()
            .take(3)
            .map(|step| format!("{}_{}", step.server, step.tool)),
    );
    let slug = parts.join("_");
    let hash = format!("{:x}", Sha256::digest(sequence_key(profile, steps)));
    format!("{slug}_{}", &hash[..8])
}

#[derive(Debug)]
struct Accumulator {
    profile: String,
    steps: Vec<Step>,
    first: String,
    last: String,
    count: usize,
}

/// Promotes exact sequences recurring at least `threshold` times.
#[must_use]
pub fn promote(episodes: &[Episode], threshold: usize) -> Vec<Pattern> {
    let mut groups = BTreeMap::<Vec<u8>, Accumulator>::new();
    let mut order = Vec::new();
    for episode in episodes {
        if episode.steps.len() < 2 {
            continue;
        }
        let key = sequence_key(&episode.profile, &episode.steps);
        if !groups.contains_key(&key) {
            order.push(key.clone());
            groups.insert(
                key.clone(),
                Accumulator {
                    profile: episode.profile.clone(),
                    steps: episode.steps.clone(),
                    first: episode.started_at.clone(),
                    last: String::new(),
                    count: 0,
                },
            );
        }
        if let Some(accumulator) = groups.get_mut(&key) {
            accumulator.count += 1;
            accumulator.last.clone_from(&episode.ended_at);
        }
    }

    let mut patterns = order
        .into_iter()
        .filter_map(|key| {
            let accumulator = groups.remove(&key)?;
            (accumulator.count >= threshold).then(|| Pattern {
                name: name(&accumulator.profile, &accumulator.steps),
                version: 1,
                profile: accumulator.profile.clone(),
                steps: accumulator.steps.clone(),
                trigger: Trigger {
                    profile: accumulator.profile,
                    first_step: accumulator.steps[0].clone(),
                },
                provenance: Provenance {
                    first_seen_at: accumulator.first,
                    last_seen_at: accumulator.last,
                    recurrence_count: accumulator.count,
                },
            })
        })
        .collect::<Vec<_>>();
    patterns.sort_by(|left, right| left.name.cmp(&right.name));
    patterns
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(server: &str, tool: &str) -> Step {
        Step {
            server: server.to_string(),
            tool: tool.to_string(),
        }
    }

    fn episode(profile: &str, steps: &[Step], day: u8) -> Episode {
        Episode {
            profile: profile.to_string(),
            steps: steps.to_vec(),
            started_at: format!("2026-08-{day:02}T09:00:00Z"),
            ended_at: format!("2026-08-{day:02}T09:05:00Z"),
        }
    }

    #[test]
    fn promotion_preserves_order_threshold_and_provenance() {
        let sequence = [
            step("memory", "memory_search"),
            step("vault", "request_credential"),
        ];
        let episodes = [
            episode("p", &sequence, 1),
            episode("p", &sequence, 2),
            episode("p", &sequence, 3),
        ];
        let patterns = promote(&episodes, 3);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].provenance.first_seen_at, "2026-08-01T09:00:00Z");
        assert_eq!(patterns[0].provenance.last_seen_at, "2026-08-03T09:05:00Z");
        assert_eq!(patterns[0].trigger.first_step, sequence[0]);
    }

    #[test]
    fn single_steps_and_wrong_order_do_not_promote() {
        let one = [step("vault", "health")];
        assert!(promote(&[episode("p", &one, 1), episode("p", &one, 2)], 2).is_empty());
        let a = [step("a", "one"), step("b", "two")];
        let b = [step("b", "two"), step("a", "one")];
        assert!(promote(&[episode("p", &a, 1), episode("p", &b, 2)], 2).is_empty());
    }
}
