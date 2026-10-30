//! Replays `tests/fixtures/config_toml_oracle.json` — captured from the real Go
//! `config.Load`, `config.LoadTargets` and `config.LoadCapabilities` by
//! `scripts/skills-config-toml-oracle` — against `symbrain_skills::config_toml`.
//!
//! The contract per case:
//!
//! * a native answer (`Loaded` / `Error`) must equal Go's byte for byte;
//! * a Go parse or typed-decode failure must never be answered natively (its
//!   message embeds source positions, so Go owns it);
//! * every Go success or `failed to apply` error must be decided natively,
//!   except the pinned `GO_OWNED_*` lists.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};
use symbrain_skills::config_toml::{
    ConfigFile, CustomTarget, LoadOutcome, SkillsConfig, TargetsOutcome, load, load_targets,
};

/// Set to a scratch fixture written by `go run ./scripts/skills-config-toml-oracle
/// -out PATH -mutations N -seed S` to soak the decoder with a larger or
/// differently seeded corpus. The pinned `GO_OWNED_*` lists and the corpus
/// size floor only describe the committed fixture and are skipped then; every
/// native-answer check still applies.
const SOAK_ENV: &str = "SYMSKILLS_CONFIG_TOML_ORACLE_FIXTURE";

fn fixture() -> Value {
    let path = std::env::var_os(SOAK_ENV).map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/config_toml_oracle.json"),
        PathBuf::from,
    );
    let raw = std::fs::read_to_string(path).expect("fixture must exist");
    serde_json::from_str(&raw).expect("fixture must be JSON")
}

fn input_bytes(input: &Value) -> Option<Vec<u8>> {
    if input.is_null() {
        return None;
    }
    if let Some(text) = input.get("utf8").and_then(Value::as_str) {
        return Some(text.as_bytes().to_vec());
    }
    if let Some(encoded) = input.get("b64").and_then(Value::as_str) {
        return Some(base64_decode(encoded));
    }
    let repeat = &input["repeat"];
    let count = usize::try_from(repeat["count"].as_u64().expect("count")).expect("count fits");
    Some(
        format!(
            "{}{}{}",
            repeat["prefix"].as_str().expect("prefix"),
            repeat["unit"].as_str().expect("unit").repeat(count),
            repeat["suffix"].as_str().expect("suffix"),
        )
        .into_bytes(),
    )
}

fn base64_decode(encoded: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for byte in encoded.bytes().filter(|byte| *byte != b'=') {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            other => panic!("invalid base64 byte {other}"),
        };
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).expect("byte"));
        }
    }
    out
}

/// The fixture stores `Load` results as the fields that differ from the
/// recorded defaults.
fn delta_json(defaults: &SkillsConfig, config: &SkillsConfig) -> Value {
    let mut out = serde_json::Map::new();
    for (key, default, got) in [
        ("library_dir", &defaults.library_dir, &config.library_dir),
        ("render_dir", &defaults.render_dir, &config.render_dir),
        ("cache_dir", &defaults.cache_dir, &config.cache_dir),
        ("profiles_dir", &defaults.profiles_dir, &config.profiles_dir),
        ("base_dir", &defaults.base_dir, &config.base_dir),
    ] {
        if default != got {
            out.insert(key.into(), json!(got));
        }
    }
    if defaults.vcs_enabled != config.vcs_enabled {
        out.insert("vcs_enabled".into(), json!(config.vcs_enabled));
    }
    Value::Object(out)
}

fn target_json(target: &CustomTarget) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("name".into(), json!(target.name));
    for (key, value) in [
        ("display_name", &target.display_name),
        ("binary_name", &target.binary_name),
    ] {
        if !value.is_empty() {
            out.insert(key.into(), json!(value));
        }
    }
    out.insert("skill_root_user".into(), json!(target.skill_root_user));
    for (key, value) in [
        ("skill_root_project", &target.skill_root_project),
        ("metadata_file", &target.metadata_file),
        ("metadata_template", &target.metadata_template),
        ("overlay_dir", &target.overlay_dir),
    ] {
        if !value.is_empty() {
            out.insert(key.into(), json!(value));
        }
    }
    if !target.capabilities.is_empty() {
        out.insert("capabilities".into(), json!(target.capabilities));
    }
    Value::Object(out)
}

/// `config.Load` cases Go accepts (or rejects while applying) that the module
/// hands back to Go, as `id reason`. Each is a documented Go quirk: `BurntSushi`
/// accepts a few documents the TOML spec forbids (a value redefining a dotted
/// table, a table extending an inline table), its datetime offsets are wider
/// than `toml_edit`'s, `toml_edit` caps nesting depth, and multi-line inline
/// tables are left to Go. Pinned so a regression towards "always ask Go" fails
/// the replay and a decoder that gains coverage must shrink the list.
const GO_OWNED_LOAD: &[&str] = &[
    "duplicate/library_dir-dotted-then-value Syntax",
    "duplicate/render_dir-dotted-then-value Syntax",
    "duplicate/cache_dir-dotted-then-value Syntax",
    "duplicate/profiles_dir-dotted-then-value Syntax",
    "duplicate/base_dir-dotted-then-value Syntax",
    "structure/table-after-dotted Syntax",
    "structure/inline-table-extend Syntax",
    "structure/inline-table-extend-key Syntax",
    "structure/inline-table-newline Syntax",
    "structure/inline-table-newline-start Syntax",
    "structure/inline-table-comment Syntax",
    "syntax-values/1979-05-27T07:32:00+24:00 Syntax",
    "syntax-values/1979-05-27T07:32:00+00:60 Syntax",
    "syntax-misc/\"[a]\\nb.c = 1\\n[a.b]\\n\" Syntax",
    "syntax-misc/\"a.b = 1\\na = 2\\n\" Syntax",
    "syntax-misc/\"a = {}\\na.b = 1\\n\" Syntax",
    "toml11/inline-newline Syntax",
    "toml11/inline-comment-in-table Syntax",
    "huge/array-depth-100 Syntax",
    "huge/array-depth-1000 Syntax",
    "huge/array-depth-20000 Syntax",
    "huge/inline-depth-200 Syntax",
    "huge/inline-depth-5000 Syntax",
    "huge/dotted-key-depth-500 Syntax",
    "huge/header-depth-2000 Syntax",
    "mutation/2457 Syntax",
    "mutation/2601 Unquotable",
    "mutation/3316 Syntax",
];

/// The same for the typed `LoadTargets`/`LoadCapabilities` decode.
const GO_OWNED_TARGETS: &[&str] = &[
    "duplicate/library_dir-dotted-then-value Syntax",
    "duplicate/render_dir-dotted-then-value Syntax",
    "duplicate/cache_dir-dotted-then-value Syntax",
    "duplicate/profiles_dir-dotted-then-value Syntax",
    "duplicate/base_dir-dotted-then-value Syntax",
    "structure/table-after-dotted Syntax",
    "structure/inline-table-extend Syntax",
    "structure/inline-table-extend-key Syntax",
    "structure/inline-table-newline Syntax",
    "structure/inline-table-newline-start Syntax",
    "structure/inline-table-comment Syntax",
    "syntax-values/1979-05-27T07:32:00+24:00 Syntax",
    "syntax-values/1979-05-27T07:32:00+00:60 Syntax",
    "syntax-misc/\"[a]\\nb.c = 1\\n[a.b]\\n\" Syntax",
    "syntax-misc/\"a.b = 1\\na = 2\\n\" Syntax",
    "syntax-misc/\"a = {}\\na.b = 1\\n\" Syntax",
    "toml11/inline-newline Syntax",
    "toml11/inline-comment-in-table Syntax",
    "huge/array-depth-100 Syntax",
    "huge/array-depth-1000 Syntax",
    "huge/array-depth-20000 Syntax",
    "huge/inline-depth-200 Syntax",
    "huge/inline-depth-5000 Syntax",
    "huge/dotted-key-depth-500 Syntax",
    "huge/header-depth-2000 Syntax",
    "mutation/2457 Syntax",
    "mutation/3316 Syntax",
];

fn pinned(list: &[&str]) -> Vec<String> {
    list.iter().map(|entry| (*entry).to_owned()).collect()
}

/// Counters and findings of one corpus replay.
#[derive(Default)]
struct Replay {
    native_value: usize,
    native_error: usize,
    go_syntax: usize,
    rejected: usize,
    targets_value: usize,
    targets_go_error: usize,
    targets_nondeterministic: usize,
    failures: Vec<String>,
    owned_load: Vec<String>,
    owned_targets: Vec<String>,
}

impl Replay {
    /// `config.Load`.
    fn check_load(
        &mut self,
        id: &str,
        expected: &Value,
        defaults: &SkillsConfig,
        outcome: LoadOutcome,
    ) {
        let go_error = expected.get("error").and_then(Value::as_str);
        let go_parse_error = go_error.is_some_and(|text| text.contains(": failed to parse "));
        match outcome {
            LoadOutcome::Loaded(config) => {
                let delta = delta_json(defaults, &config);
                let same = expected.get("config").unwrap_or(&json!({})) == &delta;
                if go_error.is_some() || !same {
                    self.failures
                        .push(format!("{id}: Loaded {config:?} but Go said {expected}"));
                } else {
                    self.native_value += 1;
                }
            }
            LoadOutcome::Error(message) => {
                if go_error == Some(message.as_str()) && !go_parse_error {
                    self.native_error += 1;
                } else {
                    self.failures
                        .push(format!("{id}: Error {message:?} but Go said {expected}"));
                }
            }
            LoadOutcome::Rejected => {
                // Only a Go parse failure may be answered without its message.
                if go_parse_error {
                    self.rejected += 1;
                } else {
                    self.failures
                        .push(format!("{id}: Rejected but Go said {expected}"));
                }
            }
            LoadOutcome::NeedsGo(reason) => {
                if go_parse_error {
                    self.go_syntax += 1;
                } else {
                    self.owned_load.push(format!("{id} {reason:?}"));
                }
            }
        }
    }

    /// `config.LoadTargets` + `config.LoadCapabilities`.
    fn check_targets(&mut self, id: &str, expected: &Value, outcome: TargetsOutcome) {
        let go_failed = expected.get("error").is_some();
        if expected.get("nondeterministic") == Some(&Value::Bool(true)) {
            // Go decides by random map order: only "ask Go" is correct.
            match outcome {
                TargetsOutcome::NeedsGo(_) => self.targets_nondeterministic += 1,
                TargetsOutcome::Loaded(config) => self.failures.push(format!(
                    "{id}: targets Loaded {config:?} but Go's answer depends on map order"
                )),
            }
            return;
        }
        match outcome {
            TargetsOutcome::Loaded(config) => {
                let targets = if config.targets.is_empty() {
                    Value::Null
                } else {
                    Value::Array(config.targets.iter().map(target_json).collect())
                };
                let capabilities = config
                    .capabilities
                    .as_ref()
                    .map_or(Value::Null, |caps| json!(caps));
                if !go_failed
                    && expected["targets"] == targets
                    && expected["capabilities"] == capabilities
                {
                    self.targets_value += 1;
                } else {
                    self.failures.push(format!(
                        "{id}: targets {targets} / {capabilities} but Go said {expected}"
                    ));
                }
            }
            TargetsOutcome::NeedsGo(reason) => {
                if go_failed {
                    self.targets_go_error += 1;
                } else {
                    self.owned_targets.push(format!("{id} {reason:?}"));
                }
            }
        }
    }
}

fn defaults_from(fx: &Value) -> SkillsConfig {
    let defaults = &fx["defaults"];
    let text = |key: &str| defaults[key].as_str().expect("default path").to_owned();
    SkillsConfig {
        library_dir: text("library_dir"),
        render_dir: text("render_dir"),
        cache_dir: text("cache_dir"),
        profiles_dir: text("profiles_dir"),
        base_dir: text("base_dir"),
        vcs_enabled: defaults["vcs_enabled"].as_bool().expect("default vcs"),
    }
}

#[test]
fn replays_go_config_toml_corpus() {
    let fx = fixture();
    let defaults = defaults_from(&fx);
    let global_path = fx["global_path"].as_str().unwrap();
    let project_path = fx["project_path"].as_str().unwrap();
    let cases = fx["cases"].as_array().expect("cases");
    let soak = std::env::var_os(SOAK_ENV).is_some();
    assert!(
        soak || cases.len() >= 6000,
        "corpus shrank to {} cases",
        cases.len()
    );

    let mut replay = Replay::default();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let global_bytes = input_bytes(&case["global"]);
        let project_bytes = input_bytes(&case["project"]);
        let global = global_bytes.as_deref().map(|content| ConfigFile {
            path: global_path,
            content,
        });
        let project = project_bytes.as_deref().map(|content| ConfigFile {
            path: project_path,
            content,
        });
        replay.check_load(
            id,
            &case["load"],
            &defaults,
            load(&defaults, global, project),
        );
        replay.check_targets(id, &case["targets_decode"], load_targets(global, project));
    }
    eprintln!(
        "config_toml oracle: {} cases; Load native value {} / apply error {} / Go parse error left to Go {} / Go parse error answered as rejected {} / \
         accepted but Go-owned {}; targets native {} / Go error left to Go {} / map-order dependent left to Go {} / \
         accepted but Go-owned {}",
        cases.len(),
        replay.native_value,
        replay.native_error,
        replay.go_syntax,
        replay.rejected,
        replay.owned_load.len(),
        replay.targets_value,
        replay.targets_go_error,
        replay.targets_nondeterministic,
        replay.owned_targets.len(),
    );
    assert!(
        replay.failures.is_empty(),
        "{} mismatches:\n{}",
        replay.failures.len(),
        replay.failures.join("\n")
    );
    assert!(
        replay.rejected > 0,
        "no invalid-escape case in the corpus was rejected"
    );
    if soak {
        eprintln!(
            "soak: Go-owned Load {:#?}\nGo-owned targets {:#?}",
            replay.owned_load, replay.owned_targets
        );
        return;
    }
    assert_eq!(
        replay.owned_load,
        pinned(GO_OWNED_LOAD),
        "Load Go-owned set drifted"
    );
    assert_eq!(
        replay.owned_targets,
        pinned(GO_OWNED_TARGETS),
        "targets Go-owned set drifted"
    );
}

/// A mutation of the decoder must be caught: the replay above is only evidence
/// if wrong answers fail it. These are the cheap in-process controls.
#[test]
fn native_answers_are_not_vacuous() {
    let defaults = SkillsConfig {
        library_dir: "L".into(),
        render_dir: "R".into(),
        cache_dir: "C".into(),
        profiles_dir: "P".into(),
        base_dir: "B".into(),
        vcs_enabled: true,
    };
    let file = |content: &'static str| ConfigFile {
        path: "/cfg.toml",
        content: content.as_bytes(),
    };
    let LoadOutcome::Loaded(config) = load(
        &defaults,
        Some(file("library_dir = \"/x\"\n[vcs]\nenabled = false\n")),
        None,
    ) else {
        panic!("simple config must decode natively");
    };
    assert_eq!(config.library_dir, "/x");
    assert!(!config.vcs_enabled);
    assert_eq!(
        load(&defaults, Some(file("render_dir = 1\n")), None),
        LoadOutcome::Error(
            "global config error: failed to apply /cfg.toml: field \"render_dir\": cannot convert int64 to string"
                .into()
        )
    );
    assert!(matches!(
        load(&defaults, Some(file("render_dir = \n")), None),
        LoadOutcome::NeedsGo(_)
    ));
    let mut by_target: BTreeMap<String, bool> = BTreeMap::new();
    by_target.insert("mcp".into(), true);
    let TargetsOutcome::Loaded(targets) = load_targets(
        Some(file(
            "[[targets]]\nname = \"a\"\nskill_root_user = \"/u\"\n[targets.capabilities]\nmcp = true\n",
        )),
        None,
    ) else {
        panic!("targets must decode natively");
    };
    assert_eq!(targets.targets[0].capabilities, by_target);
}
