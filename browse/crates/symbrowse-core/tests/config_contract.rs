#![deny(unsafe_code)]

use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::Deserialize;
use symbrowse_core::config::{Field, FlagOverrides, LoadContext, load, show_fields};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

#[derive(Deserialize)]
struct Fixture {
    schema_version: u8,
    oracle: Oracle,
    cases: Vec<Case>,
    invalid: Vec<InvalidCase>,
}

#[derive(Deserialize)]
struct Oracle {
    commit: String,
    release: String,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    fields: BTreeMap<String, Field>,
}

#[derive(Deserialize)]
struct InvalidCase {
    name: String,
    error: String,
}

fn fixture() -> Fixture {
    const CONTENT: &[u8] = include_bytes!("../../../testdata/port/core/config-contract.json");
    serde_json::from_slice(CONTENT).expect("decode Go-generated config fixture")
}

#[test]
fn config_precedence_matches_go() {
    let fixture = fixture();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(
        fixture.oracle.commit,
        "652453d1595fc302bd69c328e7da8a21dbee28b9"
    );
    assert_eq!(fixture.oracle.release, "v0.8.0");
    for expected in fixture.cases {
        let root = test_root(&expected.name);
        let context = match expected.name.as_str() {
            "defaults" => context(&root),
            "precedence" => precedence_context(&root),
            other => panic!("unknown config case {other}"),
        };
        let result = load(&context).expect("load Rust configuration");
        assert_eq!(
            normalize(show_fields(&result), &root),
            expected.fields,
            "{}",
            expected.name
        );
        fs::remove_dir_all(root).expect("remove config fixture root");
    }
}

#[test]
fn config_validation_errors_match_go() {
    for expected in fixture().invalid {
        let root = test_root(&expected.name);
        let mut context = precedence_context(&root);
        match expected.name.as_str() {
            "engine" => {
                context
                    .env
                    .insert("SYMBROWSE_ENGINE".to_owned(), "wat".to_owned());
            }
            "autosave" => {
                context
                    .env
                    .insert("SYMBROWSE_ENGINE".to_owned(), "chrome".to_owned());
                context
                    .env
                    .insert("SYMBROWSE_AUTOSAVE".to_owned(), "sometimes".to_owned());
            }
            "timeout" => {
                context
                    .env
                    .insert("SYMBROWSE_ENGINE".to_owned(), "chrome".to_owned());
                context
                    .env
                    .insert("SYMBROWSE_AUTOSAVE".to_owned(), "auto".to_owned());
                context
                    .env
                    .insert("SYMBROWSE_READ_TIMEOUT".to_owned(), "0".to_owned());
            }
            other => panic!("unknown invalid config case {other}"),
        }
        let actual = load(&context).expect_err("invalid configuration must fail");
        assert_eq!(actual.to_string(), expected.error, "{}", expected.name);
        fs::remove_dir_all(root).expect("remove config fixture root");
    }
}

#[test]
fn config_show_omits_encryption_key_material() {
    const MARKER: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let root = test_root("redaction");
    let mut context = context(&root);
    context
        .env
        .insert("SYMBROWSE_ENCRYPTION_KEY".to_owned(), MARKER.to_owned());
    let result = load(&context).expect("load configuration with encryption key environment");
    let json = serde_json::to_string(&show_fields(&result)).expect("serialize show fields");
    assert!(
        !json.contains(MARKER),
        "config show exposed encryption key material"
    );
    fs::remove_dir_all(root).expect("remove config fixture root");
}

#[test]
fn retired_compat_is_a_typed_config_error_from_toml_env_and_flag() {
    const EXPECTED: &str = "invalid configuration: transport_retired: the compat transport (Go/AzureTLS sidecar) has been retired and is no longer available; set mode to static or browser";
    for source in ["global-toml", "project-toml", "env", "flag"] {
        let root = test_root(&format!("retired-{source}"));
        let mut context = context(&root);
        match source {
            "global-toml" => {
                let global = context
                    .xdg_config_home
                    .as_ref()
                    .unwrap()
                    .join("symbrowse/config.toml");
                fs::create_dir_all(global.parent().unwrap()).expect("create config directory");
                fs::write(global, "mode = \"compat\"\n").expect("write global config");
            }
            "project-toml" => {
                fs::write(context.cwd.join(".symbrowse.toml"), "mode = \"compat\"\n")
                    .expect("write project config");
            }
            "env" => {
                context
                    .env
                    .insert("SYMBROWSE_MODE".to_owned(), "compat".to_owned());
            }
            _ => context.flags.mode = Some("compat".to_owned()),
        }
        let error = load(&context).expect_err(source).to_string();
        assert_eq!(error, EXPECTED, "{source}");
        fs::remove_dir_all(root).expect("remove config fixture root");
    }
}

#[test]
fn retired_compat_is_never_absorbed_by_the_static_engine() {
    let root = test_root("retired-static-engine");
    let mut context = context(&root);
    context
        .env
        .insert("SYMBROWSE_MODE".to_owned(), "compat".to_owned());
    context
        .env
        .insert("SYMBROWSE_ENGINE".to_owned(), "static".to_owned());
    let error = load(&context)
        .expect_err("compat with static engine")
        .to_string();
    assert!(error.contains("transport_retired"), "{error}");
    fs::remove_dir_all(root).expect("remove config fixture root");
}

#[test]
fn static_and_browser_modes_still_load_after_compat_retirement() {
    for (mode, engine) in [
        ("static", "chrome"),
        ("browser", "chrome"),
        ("browser", "firefox"),
    ] {
        let root = test_root("retired-unaffected");
        let mut context = context(&root);
        context
            .env
            .insert("SYMBROWSE_MODE".to_owned(), mode.to_owned());
        context
            .env
            .insert("SYMBROWSE_ENGINE".to_owned(), engine.to_owned());
        let result = load(&context);
        // static mode + browser engine is the pre-existing engine_not_allowed
        // contract; only the retired transport changed.
        match (mode, result) {
            ("browser", Ok(result)) => assert_eq!(result.config.mode, "browser"),
            ("static", Err(error)) => {
                assert!(error.to_string().contains("engine_not_allowed"), "{error}");
            }
            (mode, other) => panic!("unexpected {mode}: {other:?}"),
        }
        fs::remove_dir_all(root).expect("remove config fixture root");
    }
    let root = test_root("retired-unaffected-static-engine");
    let mut context = context(&root);
    context
        .env
        .insert("SYMBROWSE_ENGINE".to_owned(), "static".to_owned());
    context
        .env
        .insert("SYMBROWSE_MODE".to_owned(), "static".to_owned());
    assert_eq!(load(&context).expect("static engine").config.mode, "static");
    fs::remove_dir_all(root).expect("remove config fixture root");
}

#[derive(Deserialize)]
struct SelectionCase {
    id: String,
    mode: String,
    engine: Option<String>,
    expect: Option<String>,
    error: Option<String>,
}

fn selection_cases() -> Vec<SelectionCase> {
    const CONTENT: &[u8] = include_bytes!("../../../testdata/port/core/transport-selection.json");
    #[derive(Deserialize)]
    struct File {
        schema_version: u8,
        cases: Vec<SelectionCase>,
    }
    let file: File = serde_json::from_slice(CONTENT).expect("decode transport selection fixture");
    assert_eq!(file.schema_version, 1);
    file.cases
}

/// CFG-006: every fixture case resolves to exactly the expected selection or
/// the expected typed error code, with no fallback to another mode or engine.
#[test]
fn transport_selection_fixture_resolves_exactly_or_fails_typed() {
    use symbrowse_core::config::resolve_selection;
    let cases = selection_cases();
    assert_eq!(cases.len(), 9);
    for case in cases {
        let resolved = resolve_selection(Some(&case.mode), case.engine.as_deref());
        match (&case.expect, &case.error) {
            (Some(expect), None) => {
                let selection = resolved.unwrap_or_else(|error| panic!("{}: {error}", case.id));
                let selected = selection.engine.map_or("static".to_owned(), |engine| {
                    format!("{engine:?}").to_lowercase()
                });
                assert_eq!(&selected, expect, "{}", case.id);
            }
            (None, Some(code)) => {
                let error = resolved.expect_err(&case.id);
                assert_eq!(error.code, code, "{}", case.id);
            }
            _ => panic!(
                "{}: fixture case needs exactly one of expect/error",
                case.id
            ),
        }
    }
}

/// CFG-006: TOML, environment and flag each select an explicit mode and engine;
/// unknown, conflicting and retired selections fail with their typed code from
/// every source, and nothing is substituted.
#[test]
fn transport_selection_is_explicit_and_typed_from_every_source() {
    for case in selection_cases()
        .iter()
        // Browser mode without an engine is a resolver-level error: the
        // configuration default engine is `chrome`, never inferred silently.
        .filter(|case| case.id != "browser-missing-engine")
    {
        // The configuration spelling of the static transport is engine "static".
        let engine = match (&case.engine, case.mode.as_str()) {
            (Some(engine), _) => Some(engine.clone()),
            (None, "static") => Some("static".to_owned()),
            (None, _) => None,
        };
        for source in ["global-toml", "project-toml", "env", "flag"] {
            let root = test_root(&format!("selection-{}-{source}", case.id));
            let mut context = context(&root);
            let toml = format!(
                "mode = \"{}\"\n{}",
                case.mode,
                engine
                    .as_ref()
                    .map_or(String::new(), |engine| format!("engine = \"{engine}\"\n"))
            );
            match source {
                "global-toml" => {
                    let global = context
                        .xdg_config_home
                        .as_ref()
                        .unwrap()
                        .join("symbrowse/config.toml");
                    fs::create_dir_all(global.parent().unwrap()).expect("config directory");
                    fs::write(global, toml).expect("write global config");
                }
                "project-toml" => {
                    fs::write(context.cwd.join(".symbrowse.toml"), toml)
                        .expect("write project config");
                }
                "env" => {
                    context
                        .env
                        .insert("SYMBROWSE_MODE".to_owned(), case.mode.clone());
                    if let Some(engine) = &engine {
                        context
                            .env
                            .insert("SYMBROWSE_ENGINE".to_owned(), engine.clone());
                    }
                }
                _ => {
                    context.flags.mode = Some(case.mode.clone());
                    context.flags.engine = engine.clone();
                }
            }
            let loaded = load(&context);
            match (&case.expect, &case.error) {
                (Some(expect), None) => {
                    let config = loaded
                        .unwrap_or_else(|error| panic!("{} via {source}: {error}", case.id))
                        .config;
                    assert_eq!(&config.engine, expect, "{} via {source}", case.id);
                    assert_eq!(config.mode, case.mode, "{} via {source}", case.id);
                }
                (None, Some(code)) => {
                    let error = loaded
                        .err()
                        .unwrap_or_else(|| panic!("{} via {source} must fail", case.id))
                        .to_string();
                    // The Go-oracle engine check runs first and carries no code.
                    let expected = if code == "invalid_browser_engine" {
                        "invalid engine"
                    } else {
                        code.as_str()
                    };
                    assert!(
                        error.contains(expected),
                        "{} via {source}: {error}",
                        case.id
                    );
                }
                _ => panic!(
                    "{}: fixture case needs exactly one of expect/error",
                    case.id
                ),
            }
            fs::remove_dir_all(root).expect("remove config fixture root");
        }
    }
}

/// CFG-006: flag beats environment beats project TOML beats global TOML for
/// both the mode and the engine; the winning engine is never overridden by a
/// lower-precedence source.
#[test]
fn transport_selection_precedence_is_flag_env_project_global() {
    let root = test_root("selection-precedence");
    let mut context = context(&root);
    let global = context
        .xdg_config_home
        .as_ref()
        .unwrap()
        .join("symbrowse/config.toml");
    fs::create_dir_all(global.parent().unwrap()).expect("config directory");
    fs::write(global, "mode = \"browser\"\nengine = \"chrome\"\n").expect("global");
    let load_engine = |context: &LoadContext| {
        let result = load(context).expect("selection loads");
        (result.config.engine, result.sources["engine"].clone())
    };
    assert_eq!(
        load_engine(&context),
        ("chrome".to_owned(), "global".to_owned())
    );
    fs::write(
        context.cwd.join(".symbrowse.toml"),
        "mode = \"browser\"\nengine = \"firefox\"\n",
    )
    .expect("project");
    assert_eq!(
        load_engine(&context),
        ("firefox".to_owned(), "project".to_owned())
    );
    context
        .env
        .insert("SYMBROWSE_ENGINE".to_owned(), "safari".to_owned());
    assert_eq!(
        load_engine(&context),
        ("safari".to_owned(), "env".to_owned())
    );
    context.flags.engine = Some("chrome".to_owned());
    assert_eq!(
        load_engine(&context),
        ("chrome".to_owned(), "flag".to_owned())
    );
    fs::remove_dir_all(root).expect("remove config fixture root");
}

fn context(root: &Path) -> LoadContext {
    let home = root.join("home");
    let cwd = root.join("workspace");
    for directory in [&home, &cwd] {
        fs::create_dir_all(directory).expect("create fixture directory");
    }
    LoadContext {
        home,
        cwd,
        xdg_config_home: Some(root.join("xdg-config")),
        xdg_cache_home: Some(root.join("xdg-cache")),
        xdg_state_home: Some(root.join("xdg-state")),
        env: HashMap::new(),
        flags: FlagOverrides::default(),
    }
}

fn precedence_context(root: &Path) -> LoadContext {
    let mut context = context(root);
    let global = context
        .xdg_config_home
        .as_ref()
        .unwrap()
        .join("symbrowse/config.toml");
    fs::create_dir_all(global.parent().unwrap()).expect("create global config directory");
    fs::write(
        global,
        "log_level = \"info\"\nstate_dir = \"global-state\"\nread_timeout = 77\nallowed_domains = [\"global.example\"]\n",
    )
    .expect("write global config");
    fs::write(
        context.cwd.join(".symbrowse.toml"),
        "log_level = \"debug\"\nstate_dir = \"project-state\"\noperation_timeout = 44\nengine = \"static\"\n",
    )
    .expect("write project config");
    context.env.extend([
        ("SYMBROWSE_LOG_LEVEL".to_owned(), "error".to_owned()),
        ("SYMBROWSE_READ_TIMEOUT".to_owned(), "90".to_owned()),
        (
            "SYMBROWSE_ALLOWED_DOMAINS".to_owned(),
            "env.example, *.env.example".to_owned(),
        ),
        ("SYMBROWSE_HEADLESS".to_owned(), "true".to_owned()),
    ]);
    context.flags = FlagOverrides {
        log_level: Some("trace".to_owned()),
        state_dir: Some("flag-state".to_owned()),
        cache_dir: Some("flag-cache".to_owned()),
        ..FlagOverrides::default()
    };
    context
}

fn normalize(mut fields: BTreeMap<String, Field>, root: &Path) -> BTreeMap<String, Field> {
    let raw = root.to_string_lossy();
    let canonical = root
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(root))
        .to_string_lossy()
        .into_owned();
    for field in fields.values_mut() {
        field.value = field.value.replace(&canonical, "<ROOT>");
        field.value = field.value.replace(raw.as_ref(), "<ROOT>");
        field.value = field.value.replace('\\', "/");
    }
    fields
}

fn test_root(name: &str) -> PathBuf {
    let id = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "symbrowse-rust-config-{name}-{}-{id}",
        std::process::id()
    ));
    if root.exists() {
        fs::remove_dir_all(&root).expect("clear stale fixture root");
    }
    fs::create_dir_all(&root).expect("create fixture root");
    root
}
