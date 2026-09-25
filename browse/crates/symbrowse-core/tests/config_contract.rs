#![deny(unsafe_code)]

use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::Deserialize;
use symbrowse_core::config::{
    Field, FlagOverrides, LoadContext, load, render_show_text, render_show_yaml, show_fields,
};

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
fn remaining_environment_settings_override_global_toml_like_go() {
    let root = test_root("environment-settings");
    let mut context = context(&root);
    let global = context
        .xdg_config_home
        .as_ref()
        .unwrap()
        .join("symbrowse/config.toml");
    fs::create_dir_all(global.parent().unwrap()).expect("create global config directory");
    fs::write(
        global,
        "cache_ttl_hours = 9\nfetch_robots = false\nfetch_user_agent = \"toml-agent/1\"\nfetch_no_cache = true\nidle_timeout = 60\noperation_timeout = 35\nread_timeout = 70\nstate_expire_days = 28\nautosave = \"auto\"\nautosave_interval = 12\nautosave_key = \"global-key\"\nupload_dirs = [\"/global/uploads\"]\ndaemon_log = \"global-daemon.log\"\napproval_timeout = 45\n",
    )
    .expect("write lower-precedence config");
    context.env.extend([
        ("SYMBROWSE_CACHE_TTL_HOURS".into(), "7".into()),
        ("SYMBROWSE_FETCH_ROBOTS".into(), "".into()),
        ("SYMBROWSE_FETCH_USER_AGENT".into(), "env-agent/2".into()),
        ("SYMBROWSE_FETCH_NO_CACHE".into(), "".into()),
        ("SYMBROWSE_IDLE_TIMEOUT".into(), "120".into()),
        ("SYMBROWSE_OPERATION_TIMEOUT".into(), "44".into()),
        ("SYMBROWSE_READ_TIMEOUT".into(), "90".into()),
        ("SYMBROWSE_STATE_EXPIRE_DAYS".into(), "14".into()),
        ("SYMBROWSE_AUTOSAVE".into(), "always".into()),
        ("SYMBROWSE_AUTOSAVE_INTERVAL".into(), "7".into()),
        ("SYMBROWSE_AUTOSAVE_KEY".into(), "env-key".into()),
        (
            "SYMBROWSE_UPLOAD_DIRS".into(),
            "/tmp/uploads, /tmp/second".into(),
        ),
        (
            "SYMBROWSE_DAEMON_LOG".into(),
            root.join("env-daemon.log").display().to_string(),
        ),
        ("SYMBROWSE_APPROVAL_TIMEOUT".into(), "12".into()),
    ]);

    let result = load(&context).expect("load environment-overridden config");
    let fields = show_fields(&result);
    for (name, value) in [
        ("cache_ttl_hours", "7"),
        ("fetch_robots", "false"),
        ("fetch_user_agent", "env-agent/2"),
        ("fetch_no_cache", "true"),
        ("idle_timeout", "120"),
        ("operation_timeout", "44"),
        ("read_timeout", "90"),
        ("state_expire_days", "14"),
        ("autosave", "always"),
        ("autosave_interval", "7"),
        ("autosave_key", "env-key"),
        ("upload_dirs", "/tmp/uploads,/tmp/second"),
        ("daemon_log", root.join("env-daemon.log").to_str().unwrap()),
        ("approval_timeout", "12"),
    ] {
        assert_eq!(fields[name].value, value, "value for {name}");
        let source = if matches!(name, "fetch_robots" | "fetch_no_cache") {
            "global"
        } else {
            "env"
        };
        assert_eq!(fields[name].source, source, "source for {name}");
    }
    fs::remove_dir_all(root).expect("remove environment settings fixture root");
}

#[test]
fn supported_flags_override_environment_project_and_global_config() {
    let root = test_root("flag-precedence");
    let mut context = context(&root);
    let global = context
        .xdg_config_home
        .as_ref()
        .unwrap()
        .join("symbrowse/config.toml");
    fs::create_dir_all(global.parent().unwrap()).expect("create global config directory");
    fs::write(
        global,
        "log_level = \"info\"\nlog_format = \"text\"\nconfig_dir = \"global-config\"\ncache_dir = \"global-cache\"\nstate_dir = \"global-state\"\nexecutable_path = \"global-browser\"\n",
    )
    .expect("write global config");
    fs::write(
        context.cwd.join(".symbrowse.toml"),
        "log_level = \"debug\"\nlog_format = \"json\"\nconfig_dir = \"project-config\"\ncache_dir = \"project-cache\"\nstate_dir = \"project-state\"\nexecutable_path = \"project-browser\"\n",
    )
    .expect("write project config");
    context.env.extend([
        ("SYMBROWSE_LOG_LEVEL".into(), "warn".into()),
        ("SYMBROWSE_LOG_FORMAT".into(), "text".into()),
        ("SYMBROWSE_CONFIG_DIR".into(), "env-config".into()),
        ("SYMBROWSE_CACHE_DIR".into(), "env-cache".into()),
        ("SYMBROWSE_STATE_DIR".into(), "env-state".into()),
        ("SYMBROWSE_EXECUTABLE_PATH".into(), "env-browser".into()),
    ]);
    context.flags = FlagOverrides {
        log_level: Some("trace".into()),
        log_format: Some("json".into()),
        config_dir: Some("flag-config".into()),
        cache_dir: Some("flag-cache".into()),
        state_dir: Some("flag-state".into()),
        executable_path: Some("flag-browser".into()),
        ..FlagOverrides::default()
    };

    let result = load(&context).expect("load config with all supported flags");
    let fields = show_fields(&result);
    for (name, value) in [
        ("log_level", "trace"),
        ("log_format", "json"),
        ("config_dir", "flag-config"),
        ("cache_dir", "flag-cache"),
        ("state_dir", "flag-state"),
        ("executable_path", "flag-browser"),
    ] {
        assert_eq!(fields[name].value, value, "value for {name}");
        assert_eq!(fields[name].source, "flag", "source for {name}");
    }
    fs::remove_dir_all(root).expect("remove flag precedence fixture root");
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
fn xdg_home_fallbacks_and_relative_values_match_go_path_joining() {
    let root = test_root("xdg-paths");
    let home = root.join("home");
    let mut context = context(&root);
    context.xdg_config_home = None;
    context.xdg_cache_home = None;
    context.xdg_state_home = None;
    let result = load(&context).expect("load with XDG values unset");
    assert_eq!(
        PathBuf::from(result.config.config_dir),
        home.join(".config/symbrowse")
    );
    assert_eq!(
        PathBuf::from(result.config.cache_dir),
        home.join(".cache/symbrowse")
    );
    assert_eq!(
        PathBuf::from(result.config.state_dir),
        home.join(".local/state/symbrowse")
    );

    context.xdg_config_home = Some(PathBuf::from("relative/config"));
    context.xdg_cache_home = Some(PathBuf::from("relative/cache"));
    context.xdg_state_home = Some(PathBuf::from("relative/state"));
    let result = load(&context).expect("load with relative XDG values");
    assert_eq!(
        PathBuf::from(result.config.config_dir),
        PathBuf::from("relative/config/symbrowse")
    );
    assert_eq!(
        PathBuf::from(result.config.cache_dir),
        PathBuf::from("relative/cache/symbrowse")
    );
    assert_eq!(
        PathBuf::from(result.config.state_dir),
        PathBuf::from("relative/state/symbrowse")
    );
    assert_eq!(
        PathBuf::from(result.config.daemon_log),
        PathBuf::from("relative/state/symbrowse/daemon.log")
    );
    fs::remove_dir_all(root).expect("remove config fixture root");
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
fn config_show_redacts_endpoint_credentials_and_secret_environment_values() {
    const CORPUS: &[u8] = include_bytes!("../../../testdata/port/security/redaction-corpus.json");
    let corpus: serde_json::Value =
        serde_json::from_slice(CORPUS).expect("decode redaction corpus");
    let endpoint = corpus["endpoint"].as_str().expect("corpus endpoint");
    let secrets = corpus["secret_values"]
        .as_array()
        .expect("corpus secret markers");
    let root = test_root("redaction-endpoint");
    let mut context = context(&root);
    context
        .env
        .insert("SYMBROWSE_CDP_ENDPOINT".to_owned(), endpoint.to_owned());
    context.env.insert(
        "SYMBROWSE_ENCRYPTION_KEY".to_owned(),
        secrets[5].as_str().unwrap().to_owned(),
    );
    let result = load(&context).expect("load synthetic credential endpoint");
    let fields = show_fields(&result);
    let output = format!("{}{}", render_show_text(&result), render_show_yaml(&result));
    let endpoint_value = &fields["cdp_endpoint"].value;
    assert!(endpoint_value.contains("127.0.0.1:9222"));
    assert!(endpoint_value.contains("mode=active"));
    for secret in secrets {
        let secret = secret.as_str().unwrap();
        assert!(!output.contains(secret), "config show leaked {secret}");
        assert!(
            !endpoint_value.contains(secret),
            "endpoint output leaked {secret}"
        );
    }
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
