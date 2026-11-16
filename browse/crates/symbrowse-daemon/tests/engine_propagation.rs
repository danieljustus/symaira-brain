//! CFG-005: the engine chosen by flag, environment, TOML or default reaches the
//! daemon's `SessionSpec` unchanged, and an unknown engine never gets that far.
//! Go reference (`runDaemon`, `resolveMCPEngine`): precedence is `--engine`
//! (only when given), then `SYMBROWSE_ENGINE`, then `config.toml`, then
//! `chrome`; `ValidateEngine` rejects anything outside
//! chrome/static/safari-attach/safari-bidi. The byte-level CLI observations of
//! the Go binary are replayed by `symbrowse-cli/tests/cli_oracle.rs`
//! (`engine-*` and `cfgm-engine-*`).
#![deny(unsafe_code)]

use std::{collections::HashMap, fs, path::PathBuf};

use symbrowse_core::config::{FlagOverrides, LoadContext, load};
use symbrowse_daemon::SessionSpec;

fn context(name: &str) -> (LoadContext, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "symbrowse-engine-propagation-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let cwd = root.join("cwd");
    let home = root.join("home");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&home).unwrap();
    (
        LoadContext {
            home,
            cwd,
            xdg_config_home: Some(root.join("xdg")),
            xdg_cache_home: None,
            xdg_state_home: None,
            env: HashMap::new(),
            flags: FlagOverrides::default(),
        },
        root,
    )
}

fn spec_engine(context: &LoadContext) -> (String, String, String) {
    let result = load(context).expect("configuration loads");
    let spec = SessionSpec::from_config(&result.config, "propagation");
    (spec.engine, spec.mode, result.sources["engine"].clone())
}

#[test]
fn engine_reaches_the_session_spec_from_every_source() {
    for engine in ["chrome", "static", "safari-attach", "safari-bidi"] {
        let expected_mode = if engine == "static" {
            "static"
        } else {
            "browser"
        };

        let (mut ctx, root) = context(&format!("env-{engine}"));
        ctx.env.insert("SYMBROWSE_ENGINE".into(), engine.into());
        assert_eq!(
            spec_engine(&ctx),
            (engine.into(), expected_mode.into(), "env".into()),
            "env {engine}"
        );
        fs::remove_dir_all(root).unwrap();

        let (ctx, root) = context(&format!("project-{engine}"));
        fs::write(
            ctx.cwd.join(".symbrowse.toml"),
            format!("engine = \"{engine}\"\n"),
        )
        .unwrap();
        assert_eq!(
            spec_engine(&ctx),
            (engine.into(), expected_mode.into(), "project".into()),
            "project {engine}"
        );
        fs::remove_dir_all(root).unwrap();

        let (ctx, root) = context(&format!("global-{engine}"));
        let global = ctx.xdg_config_home.clone().unwrap().join("symbrowse");
        fs::create_dir_all(&global).unwrap();
        fs::write(
            global.join("config.toml"),
            format!("engine = \"{engine}\"\n"),
        )
        .unwrap();
        assert_eq!(
            spec_engine(&ctx),
            (engine.into(), expected_mode.into(), "global".into()),
            "global {engine}"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn default_engine_is_chrome_in_browser_mode() {
    let (ctx, root) = context("default");
    assert_eq!(
        spec_engine(&ctx),
        ("chrome".into(), "browser".into(), "default".into())
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn environment_beats_project_beats_global() {
    let (mut ctx, root) = context("precedence");
    let global = ctx.xdg_config_home.clone().unwrap().join("symbrowse");
    fs::create_dir_all(&global).unwrap();
    fs::write(global.join("config.toml"), "engine = \"static\"\n").unwrap();
    assert_eq!(spec_engine(&ctx).2, "global");
    fs::write(
        ctx.cwd.join(".symbrowse.toml"),
        "engine = \"safari-attach\"\n",
    )
    .unwrap();
    assert_eq!(
        spec_engine(&ctx),
        ("safari-attach".into(), "browser".into(), "project".into())
    );
    ctx.env
        .insert("SYMBROWSE_ENGINE".into(), "safari-bidi".into());
    assert_eq!(
        spec_engine(&ctx),
        ("safari-bidi".into(), "browser".into(), "env".into())
    );
    // An empty environment value is ignored like an unset one.
    ctx.env.insert("SYMBROWSE_ENGINE".into(), String::new());
    assert_eq!(spec_engine(&ctx).2, "project");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unknown_engine_is_rejected_by_every_source_with_the_go_message() {
    let message = "invalid configuration: invalid engine \"wat\": use one of chrome, static, safari-attach, safari-bidi";

    let (mut ctx, root) = context("bad-env");
    ctx.env.insert("SYMBROWSE_ENGINE".into(), "wat".into());
    assert_eq!(load(&ctx).unwrap_err().to_string(), message);
    fs::remove_dir_all(root).unwrap();

    let (ctx, root) = context("bad-toml");
    fs::write(ctx.cwd.join(".symbrowse.toml"), "engine = \"wat\"\n").unwrap();
    assert_eq!(load(&ctx).unwrap_err().to_string(), message);
    fs::remove_dir_all(root).unwrap();

    // Validation happens on the winning value only: a bad TOML value is
    // shadowed by a valid environment value.
    let (mut ctx, root) = context("shadowed");
    fs::write(ctx.cwd.join(".symbrowse.toml"), "engine = \"wat\"\n").unwrap();
    ctx.env.insert("SYMBROWSE_ENGINE".into(), "static".into());
    assert_eq!(spec_engine(&ctx).0, "static");
    fs::remove_dir_all(root).unwrap();
}
