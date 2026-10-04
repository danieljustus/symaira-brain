//! Frozen semantic memory kind aliases.

pub(super) const VALID: &str = "user, feedback, project, reference";

pub(super) fn normalize(value: &str) -> Option<&'static str> {
    let normalized = value.trim().to_lowercase();
    match normalized.as_str() {
        "user" | "preference" | "preferences" | "personal" | "identity" | "user-pref"
        | "user-prefs" | "about-user" => Some("user"),
        "feedback" | "correction" | "corrections" | "critique" | "evaluation" | "review"
        | "praise" | "complaint" => Some("feedback"),
        "project"
        | "project-rule"
        | "project-rules"
        | "rule"
        | "rules"
        | "guideline"
        | "guidelines"
        | "constraint"
        | "constraints"
        | "architectural-decision"
        | "adr"
        | "decision"
        | "decisions"
        | "architecture" => Some("project"),
        "reference" | "fact" | "facts" | "documentation" | "doc" | "docs" | "howto" | "how-to"
        | "api" | "external" => Some("reference"),
        _ => {
            let compact = normalized.replace(['-', '_', ' '], "");
            match compact.as_str() {
                "user" => Some("user"),
                "feedback" => Some("feedback"),
                "project" => Some("project"),
                "reference" => Some("reference"),
                _ => None,
            }
        }
    }
}
