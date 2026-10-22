//! Replays `skills_tools_go_oracle.json` — real Go `symbrain mcp` replies and filesystem effects recorded
//! by `scripts/skills-native-oracle/tools_oracle.py` from the pinned Go sources — against the native
//! `symbrain mcp` for `skills_profile_list`, `skills_profile_resolve`, `skills_discover_sources`,
//! `skills_history` and `skills_restore`.
//!
//! Every case recreates the Go pre-state (files, symlinks, pinned-date git repositories) in a fresh
//! isolated HOME/XDG tree, sends one `tools/call` per server process and requires
//!   * the reply line to be byte-identical to Go's (temp root and discovery hashes expanded), and
//!   * the post-call tree (files, modes, content hashes, symlinks, git history/status) to equal Go's.
#![cfg(unix)]

use regex::Regex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const FIXTURE: &str = include_str!("fixtures/skills_tools_go_oracle.json");
const GATEWAY_PARENTS: [&str; 2] = ["config/symbrain", "data/symbrain"];
const LOCK_PREFIX: &str = ".symskills-lock-";
/// Lock files are only masked where the install/render transactions create them; one inside the library or a
/// victim repository is a real side effect and stays compared.
const LOCK_ROOTS: [&str; 3] = [
    "home/",
    "data/symbrain/skills/rendered/",
    "data/symbrain/skills/base/",
];
const GATEWAY_STATE: [&str; 4] = [
    "data/symbrain/memory",
    "config/symbrain/memory",
    "data/symbrain/audit",
    "data/symbrain/recipes",
];

/// Cases whose Go reply text comes from a third-party parser the Rust port does not embed (`BurntSushi` TOML,
/// `yaml.v3`). Class (`isError`, `_meta` code) and the Skills-owned message prefix must still match.
const MESSAGE_RESIDUALS: [(&str, &str); 4] = [
    ("profile-list-malformed-toml", "parse profile zbad.toml: "),
    (
        "profile-list-wrong-type-field",
        "parse profile typed.toml: ",
    ),
    ("profile-resolve-broken", "parse profile broken.toml: "),
    (
        "discover-explicit-garbage-frontmatter",
        "parse SKILL.md frontmatter: ",
    ),
];

fn git(args: &[&str], cwd: &Path, env: &BTreeMap<String, String>) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(env)
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

fn pinned_git_env(oracle: &Value, root: &Path) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = oracle["git_env"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
        .collect();
    env.insert(
        "PATH".into(),
        oracle["path_with_git"].as_str().unwrap().into(),
    );
    env.insert("HOME".into(), root.join("home").display().to_string());
    env
}

fn materialize(oracle: &Value, case: &Value, root: &Path) {
    for name in ["home", "config", "data", "cache", "project", "tmp"] {
        std::fs::create_dir_all(root.join(name)).unwrap();
    }
    for rel in case["dirs"].as_array().unwrap() {
        std::fs::create_dir_all(root.join(rel.as_str().unwrap())).unwrap();
    }
    let env = pinned_git_env(oracle, root);
    for repo in case["repos"].as_array().unwrap() {
        let base = root.join(
            repo["dir"]
                .as_str()
                .unwrap()
                .replace("<ROOT>", &root.display().to_string()),
        );
        std::fs::create_dir_all(&base).unwrap();
        git(&["init", "-q", "-b", "main"], &base, &env);
        for (rel, value) in repo["files"].as_object().unwrap() {
            put(&base, rel, value, root);
        }
        let commits = repo["commits"].as_array().unwrap();
        if let Some(first) = commits.first() {
            git(&["add", "-A"], &base, &env);
            git(
                &["commit", "-q", "-m", first[0].as_str().unwrap()],
                &base,
                &env,
            );
        }
        for commit in commits.iter().skip(1) {
            for (rel, value) in commit[1].as_object().unwrap() {
                put(&base, rel, value, root);
            }
            git(&["add", "-A"], &base, &env);
            git(
                &["commit", "-q", "-m", commit[0].as_str().unwrap()],
                &base,
                &env,
            );
        }
    }
    for (rel, text) in case["files"].as_object().unwrap() {
        write(
            &root.join(rel.replace("<ROOT>", &root.display().to_string())),
            &text
                .as_str()
                .unwrap()
                .replace("<ROOT>", &root.display().to_string()),
        );
    }
    for link in case["symlinks"].as_array().unwrap() {
        let path = root.join(link[0].as_str().unwrap());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let target = link[1]
            .as_str()
            .unwrap()
            .replace("<ROOT>", &root.display().to_string());
        std::os::unix::fs::symlink(target, path).unwrap();
    }
    for step in case["pre_go"].as_array().unwrap() {
        let path = root.join(step[1].as_str().unwrap());
        match step[0].as_str().unwrap() {
            "chmod" => {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    &path,
                    std::fs::Permissions::from_mode(
                        u32::try_from(step[2].as_u64().unwrap()).unwrap(),
                    ),
                )
                .unwrap();
            }
            "edit" => std::fs::write(&path, step[2].as_str().unwrap()).unwrap(),
            other => panic!("unknown pre-state step {other}"),
        }
    }
}

/// Mirrors `tools_oracle.put`: string -> file, null -> delete, `["symlink", target]` -> symlink.
fn put(base: &Path, rel: &str, value: &Value, root: &Path) {
    let path = base.join(rel);
    match value {
        Value::Null => std::fs::remove_file(path).unwrap(),
        Value::Array(link) => {
            assert_eq!(link[0], "symlink");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            if std::fs::symlink_metadata(&path).is_ok() {
                std::fs::remove_file(&path).unwrap();
            }
            let target = link[1]
                .as_str()
                .unwrap()
                .replace("<ROOT>", &root.display().to_string());
            std::os::unix::fs::symlink(target, path).unwrap();
        }
        text => write(&path, text.as_str().unwrap()),
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// One server process per request; returns (`tools/list` line, call reply line).
fn serve(oracle: &Value, case: &Value, root: &Path, request: &str) -> (String, String) {
    let mut input = String::new();
    for line in oracle["bootstrap"].as_array().unwrap() {
        input.push_str(line.as_str().unwrap());
        input.push('\n');
    }
    input.push_str(&request.replace("<ROOT>", &root.display().to_string()));
    input.push('\n');
    let mut env = pinned_git_env(oracle, root);
    env.remove("HOME");
    let home = root.join("home");
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command.env_clear();
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    command
        .envs(&env)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("TMPDIR", root.join("tmp"))
        .current_dir(root.join("project"))
        .args(["mcp", "--profile-file"])
        .arg(root.join("profile.toml"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in case["env"].as_object().unwrap() {
        command.env(key, value.as_str().unwrap());
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = text.split('\n').filter(|line| !line.is_empty()).collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    (lines[1].to_owned(), lines[2].to_owned())
}

fn write_profile(root: &Path) {
    std::fs::write(
        root.join("profile.toml"),
        "[profile]\nname = \"tools-oracle\"\n[servers.vault]\nenabled = false\n[servers.memory]\n\
         enabled = false\n[servers.usage]\nenabled = false\n[servers.skills]\nenabled = true\n\
         [audit]\nenabled = false\n",
    )
    .unwrap();
}

/// Go's discovery hash: sha256(real path, NUL, SKILL.md), first 16 hex digits.
fn expand(expected: &str, root: &Path) -> String {
    let marker = Regex::new(r"<sid:([^>]*)>").unwrap();
    marker
        .replace_all(
            &expected.replace("<ROOT>", &root.display().to_string()),
            |caps: &regex::Captures<'_>| {
                let real = root.join(&caps[1]);
                let mut hash = Sha256::new();
                hash.update(real.as_os_str().as_encoded_bytes());
                hash.update([0]);
                hash.update(std::fs::read(real.join("SKILL.md")).unwrap_or_default());
                format!("sha256:{:.16}", format!("{:x}", hash.finalize()))
            },
        )
        .into_owned()
}

fn masked(name: &str, data: Vec<u8>) -> Vec<u8> {
    if matches!(name, ".symskills.json" | "manifest.json") {
        let timestamp =
            regex::bytes::Regex::new(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z").unwrap();
        return timestamp.replace_all(&data, &b"<TS>"[..]).into_owned();
    }
    data
}

fn replace_bytes(data: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut at = 0;
    while at < data.len() {
        if data[at..].starts_with(from) {
            out.extend_from_slice(to);
            at += from.len();
        } else {
            out.push(data[at]);
            at += 1;
        }
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        out.push(path.clone());
        let meta = std::fs::symlink_metadata(&path).unwrap();
        if meta.is_dir() {
            walk(&path, out);
        }
    }
}

/// Mirrors `tools_oracle.snapshot`.
fn snapshot(oracle: &Value, root: &Path) -> Value {
    use std::os::unix::fs::PermissionsExt;
    let mut paths = Vec::new();
    walk(root, &mut paths);
    let mut entries = serde_json::Map::new();
    let mut repos = Vec::new();
    for path in paths {
        let rel = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if GATEWAY_STATE.iter().any(|prefix| rel.starts_with(prefix))
            || GATEWAY_PARENTS.contains(&rel.as_str())
            || (path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(LOCK_PREFIX)
                && LOCK_ROOTS.iter().any(|prefix| rel.starts_with(prefix))
                && !path.is_dir())
        {
            continue;
        }
        if rel == "profile.toml" || format!("/{rel}/").contains("/.git/") || rel.ends_with("/.git")
        {
            if rel.ends_with("/.git") && path.is_dir() {
                repos.push(rel[..rel.len() - 5].to_owned());
            }
            continue;
        }
        let meta = std::fs::symlink_metadata(&path).unwrap();
        let value = if meta.file_type().is_symlink() {
            json!({"type": "symlink", "target": std::fs::read_link(&path).unwrap().to_string_lossy()})
        } else if meta.is_dir() {
            json!({"type": "dir", "mode": meta.permissions().mode() & 0o777})
        } else {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let root_bytes = root.as_os_str().as_encoded_bytes();
            let raw = std::fs::read(&path).unwrap();
            let raw = replace_bytes(&raw, root_bytes, b"<ROOT>");
            let data = masked(&name, raw);
            let mode = meta.permissions().mode() & 0o777;
            if name == ".symskills.json" {
                // Rust-only marker fields (`output_digest`, `output_manifest`) are a documented residual.
                let mut marker: Value = serde_json::from_slice(&data).unwrap();
                for extra in ["output_digest", "output_manifest"] {
                    marker.as_object_mut().unwrap().remove(extra);
                }
                json!({"type": "file", "mode": mode, "marker": marker})
            } else {
                json!({"type": "file", "mode": mode,
                       "sha256": format!("{:x}", Sha256::digest(&data)), "size": data.len()})
            }
        };
        entries.insert(rel, value);
    }
    let env = pinned_git_env(oracle, root);
    let mut histories = serde_json::Map::new();
    for repo in repos {
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(root.join(&repo))
                .env_clear()
                .envs(&env)
                .output()
                .unwrap();
            String::from_utf8_lossy(&output.stdout).into_owned()
        };
        histories.insert(
            repo.clone(),
            json!({"log": run(&["log", "--all", "--format=%H|%P|%an|%ae|%aI|%cI|%s", "--name-status"]),
                   "status": run(&["status", "--porcelain"])}),
        );
    }
    json!({"entries": entries, "git": histories})
}

struct Observed {
    replies: Vec<String>,
    after: Value,
    root: PathBuf,
    _temp: tempfile::TempDir,
}

fn requests(case: &Value) -> Vec<String> {
    case["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap().to_owned())
        .collect()
}

fn restore_permissions(case: &Value, root: &Path) {
    use std::os::unix::fs::PermissionsExt;
    for step in case["pre_go"].as_array().unwrap() {
        if step[0] == "chmod" {
            let _ = std::fs::set_permissions(
                root.join(step[1].as_str().unwrap()),
                std::fs::Permissions::from_mode(0o755),
            );
        }
    }
}

/// Recreates the pre-state, sends each request to its own native server and snapshots the result.
fn replay(oracle: &Value, case: &Value, requests: &[String]) -> Observed {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    materialize(oracle, case, &root);
    write_profile(&root);
    let root_text = root.display().to_string();
    let replies = requests
        .iter()
        .map(|request| {
            serve(oracle, case, &root, request)
                .1
                .replace(&root_text, "<ROOT>")
        })
        .collect();
    restore_permissions(case, &root);
    let after: Value = serde_json::from_str(
        &snapshot(oracle, &root)
            .to_string()
            .replace(&root_text, "<ROOT>"),
    )
    .unwrap();
    Observed {
        replies,
        after,
        root,
        _temp: temp,
    }
}

fn reply_parts(line: &str) -> (bool, Option<String>, String) {
    let value: Value = serde_json::from_str(line).unwrap();
    let result = &value["result"];
    (
        result["isError"].as_bool().unwrap_or(false),
        result["_meta"]["symaira.dev/tool_error"]["code"]
            .as_str()
            .map(str::to_owned),
        result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    )
}

/// Byte-exact reply comparison plus tree comparison; documented message residuals keep class and prefix.
fn compare(case: &Value, observed: &Observed) -> Result<(), String> {
    let id = case["id"].as_str().unwrap();
    let go_replies: Vec<_> = case["go"]["replies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    if go_replies.len() != observed.replies.len() {
        return Err(format!("{id}: reply count"));
    }
    let root_text = observed.root.display().to_string();
    for (go, native) in go_replies.iter().zip(&observed.replies) {
        let go = expand(go, &observed.root).replace(&root_text, "<ROOT>");
        if go == *native {
            continue;
        }
        let Some((_, prefix)) = MESSAGE_RESIDUALS.iter().find(|(case_id, _)| *case_id == id) else {
            return Err(format!("{id}:\n go     {go}\n native {native}"));
        };
        let (go_error, go_code, go_text) = reply_parts(&go);
        let (error, code, text) = reply_parts(native);
        let shared = |text: &str| {
            text.find(prefix)
                .map(|at| text[..at + prefix.len()].to_owned())
        };
        if go_error != error
            || go_code != code
            || shared(&go_text).is_none()
            || shared(&go_text) != shared(&text)
        {
            return Err(format!(
                "{id} (documented residual) differs in class/prefix:\n go     {go}\n native {native}"
            ));
        }
    }
    if case["go"]["after"] != observed.after {
        let tree = |v: &Value| v["entries"].as_object().cloned().unwrap_or_default();
        let (go, native) = (tree(&case["go"]["after"]), tree(&observed.after));
        let mut diffs = Vec::new();
        for key in go
            .keys()
            .chain(native.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            if go.get(key) != native.get(key) {
                diffs.push(format!(
                    "  {key}: go={:?} native={:?}",
                    go.get(key),
                    native.get(key)
                ));
            }
        }
        return Err(format!(
            "{id}: side effects differ\n{}\n git go={} native={}",
            diffs.join("\n"),
            case["go"]["after"]["git"],
            observed.after["git"]
        ));
    }
    Ok(())
}

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).unwrap()
}

fn run_case(oracle: &Value, case: &Value) -> Result<(), String> {
    compare(case, &replay(oracle, case, &requests(case)))
}

#[test]
fn native_tools_match_every_recorded_go_reply_and_side_effect() {
    let oracle = fixture();
    assert!(
        oracle["go_version"]
            .as_str()
            .unwrap()
            .starts_with("go version go1.26.7")
    );
    let cases = oracle["cases"].as_array().unwrap();
    let failures: Vec<_> = cases
        .iter()
        .filter_map(|case| run_case(&oracle, case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
    // Anti-vacuity: the corpus must cover every tool, both outcomes, each error kind and real writes.
    assert!(cases.len() >= 190, "{}", cases.len());
    // Tool result texts (the inner JSON or error message), one string per recorded reply.
    let text = |case: &Value| -> String {
        case["go"]["replies"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| reply_parts(line.as_str().unwrap()).2)
            .collect::<Vec<_>>()
            .join("\n")
    };
    for needle in [
        "\"action\":\"restored\"",
        "\"action\":\"planned\"",
        "\"synced\":[{",
        "\"status\":\"unreadable\"",
        "\"status\":\"managed\"",
        "\"profiles\":[{",
        "\"history\":[{",
        "\"issues\":[{",
        "is not versioned",
        "uncommitted changes",
        "refusing restore",
        // Review of #764 slice b: absolute names re-root under the library, links may not escape.
        "is not versioned: no git repository at <ROOT>/data/symbrain/skills/library<ROOT>/victim",
        "escapes destination",
    ] {
        assert!(
            cases.iter().any(|case| text(case).contains(needle)),
            "no case for {needle}"
        );
    }
    let codes: std::collections::BTreeSet<_> = cases
        .iter()
        .flat_map(|case| case["go"]["replies"].as_array().unwrap().iter())
        .filter_map(|line| reply_parts(line.as_str().unwrap()).1)
        .collect();
    assert!(
        codes.contains("validation") && codes.contains("conflict"),
        "{codes:?}"
    );
    for tool in oracle["tools"].as_array().unwrap() {
        let name = tool.as_str().unwrap();
        assert!(
            cases.iter().any(|case| requests(case)
                .iter()
                .any(|r| r.contains(&format!("\"name\":\"{name}\"")))),
            "{name} unused"
        );
    }
    let writers = cases
        .iter()
        .filter(|case| text(case).contains("\"head\":\""))
        .count();
    assert!(writers >= 10, "{writers} restore cases that wrote state");
}

#[test]
fn tool_metadata_matches_go_registration_byte_for_byte() {
    let oracle = fixture();
    let case = &oracle["cases"][0];
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    materialize(&oracle, case, &root);
    write_profile(&root);
    let (native, _) = serve(&oracle, case, &root, &requests(case)[0]);
    let go: Value = serde_json::from_str(oracle["tools_list"].as_str().unwrap()).unwrap();
    let wanted: Vec<_> = oracle["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    let go_text = oracle["tools_list"].as_str().unwrap();
    for tool in go["result"]["tools"].as_array().unwrap() {
        let name = tool["name"].as_str().unwrap();
        if !wanted.contains(&name) {
            continue;
        }
        // Compare the exact serialized entry (key order, escaping), not just parsed equality.
        let entry = entry_text(go_text, name);
        assert!(
            native.contains(&entry),
            "{name} entry differs:\n go {entry}\n native {native}"
        );
    }
}

/// The serialized `{...}` object for the tool `name` inside Go's `tools/list` reply.
fn entry_text(listing: &str, name: &str) -> String {
    #[derive(serde::Deserialize)]
    struct Listing {
        result: Tools,
    }
    #[derive(serde::Deserialize)]
    struct Tools {
        tools: Vec<Box<serde_json::value::RawValue>>,
    }
    let parsed: Listing = serde_json::from_str(listing).unwrap();
    parsed
        .result
        .tools
        .iter()
        .map(|tool| tool.get().to_owned())
        .find(|tool| tool.contains(&format!("\"name\":\"{name}\"")))
        .unwrap()
}

#[test]
fn tool_order_among_skills_tools_follows_go_registration() {
    let oracle = fixture();
    let case = &oracle["cases"][0];
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    materialize(&oracle, case, &root);
    write_profile(&root);
    let (native, _) = serve(&oracle, case, &root, &requests(case)[0]);
    let names = |text: &str| -> Vec<String> {
        let value: Value = serde_json::from_str(text).unwrap();
        value["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .filter(|name| name.starts_with("skills_"))
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(
        names(&native),
        names(oracle["tools_list"].as_str().unwrap())
    );
}

// ----- negative controls: the replay must be able to fail -----

#[test]
fn control_a_wrong_argument_is_detected() {
    let oracle = fixture();
    let case = oracle["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "history-limit-1")
        .unwrap();
    let mut wrong = requests(case);
    wrong[0] = wrong[0].replace("\"limit\":1", "\"limit\":2");
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    materialize(&oracle, case, &root);
    write_profile(&root);
    let (_, reply) = serve(&oracle, case, &root, &wrong[0]);
    let reply = reply.replace(&root.display().to_string(), "<ROOT>");
    assert_ne!(
        reply,
        case["go"]["replies"][0].as_str().unwrap(),
        "limit 2 must not equal Go's limit 1"
    );
}

#[test]
fn control_b_missing_side_effect_is_detected() {
    // Dry run against the pre-state of a real restore: replies differ and the tree is unchanged.
    let oracle = fixture();
    let cases = oracle["cases"].as_array().unwrap();
    let real = cases
        .iter()
        .find(|c| c["id"] == "restore-real-head2")
        .unwrap();
    let dry = cases
        .iter()
        .find(|c| c["id"] == "restore-dry-run-explicit")
        .unwrap();
    let observed = replay(&oracle, dry, &requests(dry));
    assert_ne!(
        observed.after, real["go"]["after"],
        "a dry run must not look like a real restore"
    );
    let verdict = compare(
        &json!({"id": "control", "go": {"replies": real["go"]["replies"].as_array().unwrap()[..1].to_vec(), "after": real["go"]["after"]}}),
        &observed,
    );
    assert!(verdict.is_err(), "comparator must reject a missing write");
}

#[test]
fn control_c_tampered_expected_reply_is_rejected() {
    let oracle = fixture();
    let case = oracle["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "profile-list-populated")
        .unwrap();
    let observed = replay(&oracle, case, &requests(case));
    let mut tampered = case.clone();
    let original = tampered["go"]["replies"][0].as_str().unwrap().to_owned();
    tampered["go"]["replies"][0] = json!(original.replace("Work context", "Work contexT"));
    // The honest case passes, the tampered one must not.
    assert!(compare(case, &observed).is_ok());
    assert!(compare(&tampered, &observed).is_err());
}

#[test]
fn control_d_residual_markers_cannot_hide_class_changes() {
    let oracle = fixture();
    let case = oracle["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "profile-resolve-broken")
        .unwrap();
    let observed = replay(&oracle, case, &requests(case));
    assert!(compare(case, &observed).is_ok());
    let mut flipped = case.clone();
    let reply = flipped["go"]["replies"][0]
        .as_str()
        .unwrap()
        .replace("\"code\":\"validation\"", "\"code\":\"internal\"");
    flipped["go"]["replies"][0] = json!(reply);
    assert!(compare(&flipped, &observed).is_err());
}
