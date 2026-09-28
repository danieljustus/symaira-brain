use std::{
    collections::BTreeMap,
    path::Path,
    process::{Command, Output},
};

fn commands(help: &str) -> BTreeMap<String, String> {
    let mut in_commands = false;
    help.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.ends_with(" Commands:") {
                in_commands = true;
                return None;
            }
            if matches!(trimmed, "Flags:" | "Global Flags:") {
                in_commands = false;
                return None;
            }
            if !in_commands || !line.starts_with("  ") || line.trim_start().starts_with('-') {
                return None;
            }
            if trimmed.ends_with(':') || trimmed.is_empty() {
                return None;
            }
            let name = trimmed.split_whitespace().next()?;
            let description = trimmed[name.len()..].trim();
            (name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
            .then(|| (name.to_owned(), description.to_owned()))
        })
        .collect()
}

fn output(binary: &Path, args: &[&str]) -> Output {
    Command::new(binary).args(args).output().expect("run CLI")
}

fn filter_commands(help: &str, allowed: &BTreeMap<String, String>) -> String {
    let mut in_commands = false;
    help.lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.ends_with(" Commands:") {
                in_commands = true;
                return true;
            }
            if matches!(trimmed, "Flags:" | "Global Flags:") {
                in_commands = false;
                return true;
            }
            if in_commands && line.starts_with("  ") && !trimmed.starts_with('-') {
                if let Some(name) = trimmed.split_whitespace().next() {
                    return allowed.contains_key(name);
                }
            }
            true
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn completion_items(output: &[u8]) -> (BTreeMap<String, String>, Option<String>) {
    let text = String::from_utf8_lossy(output);
    let items = text
        .lines()
        .filter(|line| !line.starts_with(':'))
        .filter_map(|line| {
            let (name, description) = line.split_once('\t').unwrap_or((line, ""));
            (!name.is_empty()).then(|| (name.to_owned(), description.to_owned()))
        })
        .collect();
    let directive = text
        .lines()
        .find(|line| line.starts_with(':'))
        .map(str::to_owned);
    (items, directive)
}

#[test]
fn rust_help_and_completion_follow_the_go_source_tree() {
    let browse = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("browse crate parent");
    let temp = tempfile::tempdir().expect("temporary directory");
    let go_binary = temp.path().join("symbrowse-go-oracle");
    let build = Command::new("go")
        .args(["build", "-o"])
        .arg(&go_binary)
        .arg("./cmd/symbrowse")
        .current_dir(browse)
        .output()
        .expect("build source-bound Go oracle");
    assert!(
        build.status.success(),
        "Go oracle build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let rust_binary = Path::new(env!("CARGO_BIN_EXE_symbrowse"));
    let go_root = output(&go_binary, &["--help"]);
    let rust_root = output(rust_binary, &["--help"]);
    assert!(go_root.status.success(), "Go --help failed");
    assert!(
        rust_root.status.success(),
        "Rust --help failed: {}",
        String::from_utf8_lossy(&rust_root.stderr)
    );

    let go_commands = commands(std::str::from_utf8(&go_root.stdout).expect("Go help UTF-8"));
    let rust_commands = commands(std::str::from_utf8(&rust_root.stdout).expect("Rust help UTF-8"));
    assert!(!rust_commands.is_empty(), "Rust help has no commands");
    assert!(
        rust_commands
            .keys()
            .all(|command| go_commands.contains_key(command)),
        "Rust advertises commands absent from Go: {:?}",
        rust_commands
            .keys()
            .filter(|command| !go_commands.contains_key(*command))
            .collect::<Vec<_>>()
    );
    for (command, description) in &rust_commands {
        assert_eq!(
            description, &go_commands[command],
            "help summary drift for {command}"
        );
    }

    let mut pending = rust_commands
        .keys()
        .map(|command| vec![command.clone()])
        .collect::<Vec<_>>();
    let mut checked = BTreeMap::<String, ()>::new();
    while let Some(path) = pending.pop() {
        let key = path.join(" ");
        if checked.insert(key.clone(), ()).is_some() {
            continue;
        }
        let mut args = path.iter().map(String::as_str).collect::<Vec<_>>();
        args.push("--help");
        let go_help = output(&go_binary, &args);
        let rust_help = output(rust_binary, &args);
        assert!(go_help.status.success(), "Go help failed for {key}");
        assert!(
            rust_help.status.success(),
            "Rust advertises {key} but its help fails: {}",
            String::from_utf8_lossy(&rust_help.stderr)
        );
        let children = commands(std::str::from_utf8(&rust_help.stdout).expect("Rust help UTF-8"));
        let go_children = commands(std::str::from_utf8(&go_help.stdout).expect("Go help UTF-8"));
        assert!(
            children
                .iter()
                .all(|(name, description)| go_children.get(name) == Some(description)),
            "Rust help for {key} advertises source-absent or drifted children: {children:?}; Go: {go_children:?}"
        );
        let normalized_go = filter_commands(
            std::str::from_utf8(&go_help.stdout).expect("Go help UTF-8"),
            &children,
        );
        pending.extend(children.keys().map(|child| {
            path.iter()
                .cloned()
                .chain(std::iter::once(child.clone()))
                .collect()
        }));
        assert_eq!(
            String::from_utf8_lossy(&rust_help.stdout),
            normalized_go,
            "help output drift for {key} after filtering unsupported Go-only commands; Rust children: {children:?}"
        );
    }

    for path in [&["completion", "--help"][..]] {
        let go_help = output(&go_binary, path);
        let rust_help = output(rust_binary, path);
        assert!(go_help.status.success(), "Go help failed for {path:?}");
        assert!(rust_help.status.success(), "Rust help failed for {path:?}");
        assert_eq!(
            rust_help.stdout, go_help.stdout,
            "help output drift for {path:?}"
        );
    }

    for shell in ["bash", "zsh", "fish", "powershell"] {
        for no_descriptions in [false, true] {
            let mut args = vec!["completion", shell];
            if no_descriptions {
                args.push("--no-descriptions");
            }
            let go_script = output(&go_binary, &args);
            let rust_script = output(rust_binary, &args);
            assert!(
                go_script.status.success(),
                "Go completion failed for {shell}"
            );
            assert!(
                rust_script.status.success(),
                "Rust completion failed for {shell}"
            );
            assert_eq!(
                rust_script.stdout, go_script.stdout,
                "completion script drift for {shell}, no_descriptions={no_descriptions}"
            );
        }
        let args = ["completion", shell, "--help"];
        let go_help = output(&go_binary, &args);
        let rust_help = output(rust_binary, &args);
        assert!(
            go_help.status.success(),
            "Go completion help failed for {shell}"
        );
        assert!(
            rust_help.status.success(),
            "Rust completion help failed for {shell}"
        );
        assert_eq!(
            rust_help.stdout, go_help.stdout,
            "completion help drift for {shell}"
        );
    }

    for args in [
        &["--output", "json", "completion", "bash"][..],
        &["completion", "bash", "--output", "json"][..],
    ] {
        let go_completion = output(&go_binary, args);
        let rust_completion = output(rust_binary, args);
        assert!(
            go_completion.status.success(),
            "Go completion failed for global output placement {args:?}"
        );
        assert!(
            rust_completion.status.success(),
            "Rust completion failed for global output placement {args:?}: {}",
            String::from_utf8_lossy(&rust_completion.stderr)
        );
        assert_eq!(
            rust_completion.stdout, go_completion.stdout,
            "completion script differs for global output placement {args:?}"
        );
    }

    for args in [
        &["__complete", "s"][..],
        &["__complete", "state", "c"][..],
        &["__complete", "open", ""][..],
        &["__complete", "click", "--"][..],
        &["__completeNoDesc", "state", "c"][..],
    ] {
        let go_completion = output(&go_binary, args);
        let rust_completion = output(rust_binary, args);
        assert!(
            go_completion.status.success(),
            "Go completion failed for {args:?}"
        );
        assert!(
            rust_completion.status.success(),
            "Rust completion failed for {args:?}"
        );
        let (rust_items, rust_directive) = completion_items(&rust_completion.stdout);
        let (go_items, go_directive) = completion_items(&go_completion.stdout);
        assert_eq!(
            rust_directive, go_directive,
            "completion directive drift for {args:?}"
        );
        assert!(
            rust_items
                .iter()
                .all(
                    |(name, description)| go_items.get(name).is_some_and(|go_description| {
                        description.is_empty() || description == go_description
                    })
                ),
            "Rust completion items are absent from or drifted from Go for {args:?}: {rust_items:?}; Go: {go_items:?}"
        );
    }

    let go_completion = output(&go_binary, &["__complete", "s"]);
    let rust_completion = output(rust_binary, &["__complete", "s"]);
    assert!(go_completion.status.success());
    assert!(rust_completion.status.success());
    let parse_candidates = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .lines()
            .filter(|line| !line.starts_with(':'))
            .filter_map(|line| {
                let (name, description) = line.split_once('\t').unwrap_or((line, ""));
                Some((name.to_owned(), description.to_owned()))
            })
            .collect::<BTreeMap<_, _>>()
    };
    let rust_candidates = parse_candidates(&rust_completion.stdout);
    let expected_candidates = rust_commands
        .iter()
        .filter(|(command, _)| command.starts_with('s'))
        .map(|(command, description)| (command.clone(), description.clone()))
        .collect::<BTreeMap<_, _>>();
    let go_candidates = parse_candidates(&go_completion.stdout);
    assert_eq!(rust_candidates, expected_candidates);
    assert!(
        rust_candidates
            .iter()
            .all(|(command, description)| go_candidates.get(command) == Some(description)),
        "Rust completion candidate drift from the Go oracle"
    );
}
