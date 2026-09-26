//! Cobra-compatible shell scripts backed by the Rust CLI's implemented help tree.

use std::collections::{BTreeSet, HashSet};

pub(super) fn script(shell: &str) -> Option<&'static str> {
    match shell {
        "bash" => Some(include_str!("completion/bash.txt")),
        "zsh" => Some(include_str!("completion/zsh.txt")),
        "fish" => Some(include_str!("completion/fish.txt")),
        "powershell" => Some(include_str!("completion/powershell.txt")),
        _ => None,
    }
}

/// The Cobra `--no-descriptions` generator changes the shell-compgen request
/// protocol while leaving the rest of the checked-in template unchanged.
pub(super) fn script_without_descriptions(shell: &str) -> Option<String> {
    script(shell).map(|script| script.replace("__complete", "__completeNoDesc"))
}

/// Implements the hidden request used by the generated Cobra V2 shell scripts.
pub(super) fn complete(args: &[String]) -> String {
    let Some(prefix) = args.last() else {
        return ":0\n".to_owned();
    };
    let context = &args[..args.len() - 1];
    let Some(help) = help_for_context(context) else {
        return ":0\n".to_owned();
    };

    let no_file_completion = prefix.is_empty() || prefix.starts_with('-');
    let candidates = if prefix.starts_with('-') {
        flag_candidates(&help, prefix)
    } else if context.iter().all(|value| !value.starts_with('-')) {
        command_candidates(&help, prefix)
    } else {
        Vec::new()
    };

    let mut output = String::new();
    for (name, description) in candidates {
        output.push_str(&name);
        if !description.is_empty() {
            output.push('\t');
            output.push_str(&description);
        }
        output.push('\n');
    }
    output.push_str(if no_file_completion { ":4\n" } else { ":0\n" });
    output
}

/// Implements Cobra's `__completeNoDesc` hidden request protocol.
pub(super) fn complete_no_descriptions(args: &[String]) -> String {
    let candidates = complete(args);
    let mut output = String::new();
    for line in candidates.lines() {
        if line.starts_with(':') {
            output.push_str(line);
        } else {
            output.push_str(line.split_once('\t').map_or(line, |(name, _)| name));
        }
        output.push('\n');
    }
    output
}

fn help_for_context(context: &[String]) -> Option<String> {
    if context.is_empty() {
        return Some(super::root_help());
    }
    let command = match context[0].as_str() {
        "workflow" => "flow",
        other => other,
    };
    let suffix: Vec<_> = context[1..]
        .iter()
        .take_while(|value| !value.starts_with('-'))
        .map(String::as_str)
        .collect();
    super::help_catalog::help(command, &suffix)
        .map(str::to_owned)
        .or_else(|| super::command_help(command, &suffix))
}

fn command_candidates(help: &str, prefix: &str) -> Vec<(String, String)> {
    let mut in_commands = false;
    let mut candidates = BTreeSet::new();
    for line in help.lines() {
        let trimmed = line.trim();
        if trimmed == "Available Commands:" || trimmed.ends_with(" Commands:") {
            in_commands = true;
            continue;
        }
        if trimmed == "Flags:" || trimmed == "Global Flags:" {
            in_commands = false;
        }
        if !in_commands || trimmed.is_empty() || trimmed.ends_with(':') {
            continue;
        }
        let Some((name, description)) = split_name_description(trimmed) else {
            continue;
        };
        if !name.starts_with('-') && name.starts_with(prefix) {
            candidates.insert((name.to_owned(), description.to_owned()));
        }
    }
    candidates.into_iter().collect()
}

fn flag_candidates(help: &str, prefix: &str) -> Vec<(String, String)> {
    let mut in_flags = false;
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let mut help_candidates = Vec::new();
    for line in help.lines() {
        let trimmed = line.trim();
        if trimmed == "Flags:" || trimmed == "Global Flags:" {
            in_flags = true;
            continue;
        }
        if !in_flags {
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with("Use ") {
            in_flags = false;
            continue;
        }
        let Some(name_end) = trimmed.find(char::is_whitespace) else {
            continue;
        };
        let first_name = &trimmed[..name_end];
        if !first_name.starts_with('-') {
            continue;
        }
        let mut remainder = trimmed[name_end..].trim_start();
        let mut names = vec![first_name.trim_end_matches(',')];
        if first_name.ends_with(',') {
            let Some(alias_end) = remainder.find(char::is_whitespace) else {
                continue;
            };
            names.push(&remainder[..alias_end]);
            remainder = remainder[alias_end..].trim_start();
        }
        let first_description_word = remainder.split_whitespace().next().unwrap_or_default();
        let value_type = matches!(
            first_description_word,
            "string"
                | "stringArray"
                | "stringToString"
                | "int"
                | "int64"
                | "uint"
                | "bool"
                | "duration"
                | "float"
        );
        let description = if value_type {
            remainder
                .split_once(char::is_whitespace)
                .map_or("", |(_, rest)| rest.trim_start())
        } else {
            remainder
        };
        for name in names {
            if name.starts_with(prefix) {
                let candidate = name.to_owned();
                // Cobra appends the help flag after inherited/global flags;
                // its completion protocol does not append '=' to value flags.
                let mut description = description.to_owned();
                if let Some((before_default, _)) = description.split_once(" (default ") {
                    description = before_default.to_owned();
                }
                let item = (candidate.clone(), description);
                if seen.insert(candidate) {
                    if item.0 == "--help" {
                        help_candidates.push(item);
                    } else {
                        candidates.push(item);
                    }
                }
            }
        }
    }
    candidates.extend(help_candidates);
    candidates
}

fn split_name_description(value: &str) -> Option<(&str, &str)> {
    let name_end = value.find(char::is_whitespace)?;
    let name = &value[..name_end];
    let rest = value[name_end..].trim_start();
    Some((name, rest))
}

#[cfg(test)]
mod tests {
    use super::{complete, complete_no_descriptions, script, script_without_descriptions};
    use crate::{Action, parse};
    use sha2::{Digest, Sha256};
    use std::ffi::OsString;

    #[test]
    fn cobra_scripts_match_the_pinned_go_oracle_bytes() {
        let expected = [
            (
                "bash",
                16_333,
                "d8aebb6e8b7b8ff15197dfadec7802ac0b5d7e624ae87e7c767ba2e0d3b81240",
            ),
            (
                "zsh",
                7_856,
                "17788f31f6b6703aca7bdeac9c904fec2f5cb510e047c19958b60df8f52b4359",
            ),
            (
                "fish",
                9_969,
                "d8e10e9dca934eef57a548953c9bf7e428b23eb786ad2e764a9b1626928f10d8",
            ),
            (
                "powershell",
                10_904,
                "c81fe9adbc6e5e4901b089e22b265737ad424aad258c68286f56ca57ce5cc0d3",
            ),
        ];
        for (shell, length, digest) in expected {
            let bytes = script(shell).expect("known shell").as_bytes();
            assert_eq!(bytes.len(), length, "{shell} byte length");
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                digest,
                "{shell} SHA-256"
            );
        }
        assert!(script("unknown").is_none());
    }

    #[test]
    fn no_description_script_uses_cobra_no_desc_request_protocol() {
        for shell in ["bash", "zsh", "fish", "powershell"] {
            let regular = script(shell).expect("known shell");
            let without_descriptions = script_without_descriptions(shell).expect("known shell");
            assert!(regular.contains(" __complete "));
            assert!(without_descriptions.contains(" __completeNoDesc "));
            assert!(!without_descriptions.contains(" __complete "));
        }
        assert!(script_without_descriptions("unknown").is_none());
    }

    #[test]
    fn hidden_completion_uses_only_the_implemented_command_tree() {
        let root = complete(&[String::new()]);
        assert!(root.contains("doctor\tCheck browser discovery"));
        assert!(root.contains("auth\tCredential management"));

        let children = complete(&["cookies".into(), String::new()]);
        assert!(children.contains("clear\tDelete one cookie"));
        assert!(children.contains("list\tList cookies"));
        assert!(children.contains("set\tSet a cookie"));
    }

    #[test]
    fn hidden_completion_matches_cobra_flag_candidates_and_order() {
        assert_eq!(
            complete(&["cookies".into(), "list".into(), "--".into()]),
            "--json\tprint the unified machine-readable output envelope (shorthand for --output json)\n--output\toutput format: text, json or yaml (--json is shorthand for --output json)\n--reveal\tshow cookie values (default: masked); accepts a comma-separated allowlist of cookie names or \"all\"\n--session\tsession name\n--help\thelp for list\n:4\n"
        );
    }

    #[test]
    fn no_description_protocol_drops_candidate_descriptions_only() {
        assert_eq!(
            complete_no_descriptions(&["cookies".into(), "list".into(), "--".into()]),
            "--json\n--output\n--reveal\n--session\n--help\n:4\n"
        );
        assert_eq!(
            complete_no_descriptions(&["cookies".into(), "".into()]),
            "clear\nlist\nset\n:4\n"
        );
    }

    #[test]
    fn parser_exposes_only_supported_shell_generators_and_hidden_protocol() {
        for shell in ["bash", "zsh", "fish", "powershell"] {
            let args = [OsString::from("completion"), OsString::from(shell)];
            assert!(
                matches!(parse(&args), Ok(Action::Completion { shell: actual, no_descriptions: false }) if actual == shell)
            );
        }
        let request = [
            OsString::from("__complete"),
            OsString::from("cookies"),
            OsString::new(),
        ];
        assert!(
            matches!(parse(&request), Ok(Action::CompletionRequest { args, no_descriptions: false }) if args == vec!["cookies".to_owned(), String::new()])
        );
        let no_desc_request = [
            OsString::from("__completeNoDesc"),
            OsString::from("cookies"),
            OsString::new(),
        ];
        assert!(matches!(
            parse(&no_desc_request),
            Ok(Action::CompletionRequest { args, no_descriptions: true })
                if args == vec!["cookies".to_owned(), String::new()]
        ));
        assert!(matches!(
            parse(&[OsString::from("completion"), OsString::from("bash"), OsString::from("--no-descriptions")]),
            Ok(Action::Completion { shell, no_descriptions: true }) if shell == "bash"
        ));
        assert!(matches!(
            parse(&[OsString::from("completion"), OsString::from("bash"), OsString::from("--help")]),
            Ok(Action::Help(help)) if help.contains("--no-descriptions") && help.contains("completion bash")
        ));
        assert!(parse(&[OsString::from("completion"), OsString::from("auth")]).is_err());
    }
}
