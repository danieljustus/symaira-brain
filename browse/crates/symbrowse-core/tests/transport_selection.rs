#![deny(unsafe_code)]

use std::{
    collections::HashMap,
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

use serde::Deserialize;
use symbrowse_core::config::{
    FlagOverrides, LoadContext, TransportMode, explicit_selection, load, render_show_yaml,
    resolve_selection,
};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

#[derive(Deserialize)]
struct Fixture {
    schema_version: u8,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    mode: Option<String>,
    engine: Option<String>,
    expect: Option<String>,
    error: Option<String>,
}

fn fixture() -> Fixture {
    const CONTENT: &[u8] = include_bytes!("../../../testdata/port/core/transport-selection.json");
    serde_json::from_slice(CONTENT).expect("decode transport-selection fixture")
}

#[test]
fn every_transport_selection_fixture_case_matches_rust() {
    let fixture = fixture();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.cases.len(), 13);
    let mut seen = std::collections::BTreeSet::new();
    for case in fixture.cases {
        assert!(
            seen.insert(case.id.clone()),
            "duplicate fixture case {}",
            case.id
        );
        let result = resolve_selection(case.mode.as_deref(), case.engine.as_deref());
        if let Some(expected_error) = case.error {
            assert_eq!(
                result.unwrap_err().code,
                expected_error,
                "fixture case {}",
                case.id
            );
            continue;
        }
        let selected = result.unwrap_or_else(|error| {
            panic!("fixture case {} unexpectedly failed: {error}", case.id)
        });
        let expected = case
            .expect
            .expect("successful case declares expected selection");
        match expected.as_str() {
            "static" => assert_eq!(selected.mode, TransportMode::Static, "{}", case.id),
            "compat" => assert_eq!(selected.mode, TransportMode::Compat, "{}", case.id),
            engine => assert_eq!(
                selected
                    .engine
                    .map(|value| format!("{value:?}").to_ascii_lowercase()),
                Some(engine.to_owned()),
                "fixture case {}",
                case.id
            ),
        }
    }
}

#[test]
fn transport_selection_obeys_toml_environment_and_flag_precedence() {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "symbrowse-transport-selection-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let home = root.join("home");
    let cwd = root.join("project");
    let xdg_config = root.join("xdg-config");
    fs::create_dir_all(&home).expect("create fixture home");
    fs::create_dir_all(&cwd).expect("create fixture project");
    let global = xdg_config.join("symbrowse/config.toml");
    fs::create_dir_all(global.parent().expect("global config parent"))
        .expect("create global config directory");
    fs::write(&global, "mode = \"static\"\nengine = \"chrome\"\n").expect("write global config");
    fs::write(
        cwd.join(".symbrowse.toml"),
        "mode = \"browser\"\nengine = \"safari-attach\"\n",
    )
    .expect("write project config");

    let mut env = HashMap::new();
    env.insert("SYMBROWSE_MODE".to_owned(), "browser".to_owned());
    env.insert("SYMBROWSE_ENGINE".to_owned(), "firefox".to_owned());
    let context = LoadContext {
        home,
        cwd,
        xdg_config_home: Some(xdg_config),
        xdg_cache_home: None,
        xdg_state_home: None,
        env,
        flags: FlagOverrides {
            mode: Some("browser".to_owned()),
            engine: Some("safari-bidi".to_owned()),
            ..FlagOverrides::default()
        },
    };
    let result = load(&context).expect("load precedence configuration");
    assert_eq!(result.config.mode, "browser");
    assert_eq!(result.config.engine, "safari-bidi");
    assert_eq!(result.sources["mode"], "flag");
    assert_eq!(result.sources["engine"], "flag");
    let selected = explicit_selection(&result).expect("mode was explicitly selected");
    assert_eq!(selected.mode, "browser");
    assert_eq!(selected.engine.as_deref(), Some("safari-bidi"));
    let machine_output = render_show_yaml(&result);
    assert!(machine_output.contains("selection:"));
    assert!(machine_output.contains("engine: safari-bidi"));
    assert!(!machine_output.contains("profile"));
    fs::remove_dir_all(root).expect("remove unique config fixture root");
}

#[test]
fn engine_and_mode_precedence_covers_default_global_project_environment_and_flag() {
    let root = std::env::temp_dir().join(format!(
        "symbrowse-selection-precedence-{}-{}",
        std::process::id(),
        NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut context = LoadContext {
        home: root.join("home"),
        cwd: root.join("project"),
        xdg_config_home: Some(root.join("xdg-config")),
        xdg_cache_home: None,
        xdg_state_home: None,
        env: HashMap::new(),
        flags: FlagOverrides::default(),
    };
    fs::create_dir_all(&context.home).expect("create fixture home");
    fs::create_dir_all(&context.cwd).expect("create fixture project");

    let assert_source = |context: &LoadContext,
                         mode: &str,
                         engine: &str,
                         mode_source: &str,
                         engine_source: &str| {
        let loaded = load(context).expect("load precedence stage");
        assert_eq!(loaded.config.mode, mode);
        assert_eq!(loaded.config.engine, engine);
        assert_eq!(loaded.sources["mode"], mode_source);
        assert_eq!(loaded.sources["engine"], engine_source);
        let selection = explicit_selection(&loaded);
        if mode_source == "default" {
            assert!(selection.is_none(), "defaults are not explicit selections");
            return;
        }
        let selection = selection.expect("non-default mode is explicit");
        assert_eq!(selection.mode, mode);
        if mode == "browser" {
            assert_eq!(selection.engine.as_deref(), Some(engine));
        } else {
            assert_eq!(selection.engine, None);
        }
    };

    assert_source(&context, "browser", "chrome", "default", "default");
    let global = context
        .xdg_config_home
        .as_ref()
        .unwrap()
        .join("symbrowse/config.toml");
    fs::create_dir_all(global.parent().unwrap()).expect("create global config directory");
    fs::write(&global, "mode = \"static\"\nengine = \"\"\n").expect("write global config");
    assert_source(&context, "static", "", "global", "global");

    fs::write(
        context.cwd.join(".symbrowse.toml"),
        "mode = \"compat\"\nengine = \"\"\n",
    )
    .expect("write project config");
    assert_source(&context, "compat", "", "project", "project");

    context.env.insert("SYMBROWSE_MODE".into(), "static".into());
    assert_source(&context, "static", "", "env", "project");

    context.flags.mode = Some("browser".into());
    context.flags.engine = Some("safari-bidi".into());
    assert_source(&context, "browser", "safari-bidi", "flag", "flag");
    fs::remove_dir_all(root).expect("remove unique precedence fixture root");
}

#[test]
fn engine_precedence_selects_default_global_project_environment_and_flag() {
    let root = std::env::temp_dir().join(format!(
        "symbrowse-engine-precedence-{}-{}",
        std::process::id(),
        NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut context = LoadContext {
        home: root.join("home"),
        cwd: root.join("project"),
        xdg_config_home: Some(root.join("xdg-config")),
        xdg_cache_home: None,
        xdg_state_home: None,
        env: HashMap::new(),
        flags: FlagOverrides::default(),
    };
    fs::create_dir_all(&context.home).expect("create fixture home");
    fs::create_dir_all(&context.cwd).expect("create fixture project");
    let assert_engine = |context: &LoadContext, engine: &str, source: &str| {
        let loaded = load(context).expect("load engine precedence stage");
        assert_eq!(loaded.config.mode, "browser");
        assert_eq!(loaded.config.engine, engine);
        assert_eq!(loaded.sources["engine"], source);
        match explicit_selection(&loaded) {
            Some(selection) => assert_eq!(selection.engine.as_deref(), Some(engine)),
            None => assert_eq!(source, "default"),
        }
    };

    assert_engine(&context, "chrome", "default");
    let global = context
        .xdg_config_home
        .as_ref()
        .unwrap()
        .join("symbrowse/config.toml");
    fs::create_dir_all(global.parent().unwrap()).expect("create global config directory");
    fs::write(&global, "mode = \"browser\"\nengine = \"safari-bidi\"\n")
        .expect("write global config");
    assert_engine(&context, "safari-bidi", "global");
    fs::write(
        context.cwd.join(".symbrowse.toml"),
        "mode = \"browser\"\nengine = \"firefox\"\n",
    )
    .expect("write project config");
    assert_engine(&context, "firefox", "project");
    context
        .env
        .insert("SYMBROWSE_ENGINE".into(), "safari-attach".into());
    assert_engine(&context, "safari-attach", "env");
    context.flags.engine = Some("chrome".into());
    assert_engine(&context, "chrome", "flag");
    fs::remove_dir_all(root).expect("remove unique engine fixture root");
}
