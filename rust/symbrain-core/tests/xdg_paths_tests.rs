use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use symbrain_core::xdg::{
    resolve_audit_dir, resolve_cache_dir, resolve_config_dir, resolve_config_path,
    resolve_data_dir, resolve_managed_bin_dir, resolve_patterns_dir, resolve_profiles_dir,
};

#[derive(Debug, Deserialize)]
struct Suite {
    cases: Vec<TestCase>,
}

#[derive(Debug, Deserialize)]
struct TestCase {
    id: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
    expect: BTreeMap<String, String>,
}

fn get_env(env: &BTreeMap<String, String>, key: &str) -> Option<PathBuf> {
    env.get(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn load_fixture() -> Vec<u8> {
    match std::env::var_os("SYMBRAIN_XDG_ORACLE_FIXTURE") {
        Some(path) => fs::read(PathBuf::from(path)).expect("read platform Go oracle fixture"),
        None if cfg!(windows) => {
            panic!("native Windows tests require SYMBRAIN_XDG_ORACLE_FIXTURE from the Go oracle")
        }
        None => include_bytes!("fixtures/oracle_expectations.json").to_vec(),
    }
}

#[test]
fn xdg_paths_match_go_oracle() {
    let fixture = load_fixture();
    let suite: Suite = serde_json::from_slice(&fixture).expect("parse xdg oracle fixture");
    assert_eq!(
        suite.cases.len(),
        256,
        "the full 4x4x4x4 XDG matrix must run"
    );

    #[cfg(windows)]
    for case in &suite.cases {
        for (dimension, key) in [
            ("home=abs", "HOME"),
            ("xdg-config=abs", "XDG_CONFIG_HOME"),
            ("xdg-data=abs", "XDG_DATA_HOME"),
            ("xdg-cache=abs", "XDG_CACHE_HOME"),
        ] {
            if case.id.split(',').any(|part| part == dimension) {
                let value = case.env.get(key).expect("absolute case has a path");
                assert!(
                    PathBuf::from(value).is_absolute(),
                    "case {} must use native absolute {key}: {value:?}",
                    case.id
                );
            }
        }
    }

    for case in suite.cases {
        let home = get_env(&case.env, "HOME");
        let xdg_config = get_env(&case.env, "XDG_CONFIG_HOME");
        let xdg_data = get_env(&case.env, "XDG_DATA_HOME");
        let xdg_cache = get_env(&case.env, "XDG_CACHE_HOME");

        let config_path = resolve_config_path(xdg_config.as_deref(), home.as_deref());
        let config_dir = resolve_config_dir(xdg_config.as_deref(), home.as_deref());
        let profiles_dir = resolve_profiles_dir(xdg_config.as_deref(), home.as_deref());
        let data_dir = resolve_data_dir(xdg_data.as_deref(), home.as_deref());
        let audit_dir = resolve_audit_dir(xdg_data.as_deref(), home.as_deref());
        let patterns_dir = resolve_patterns_dir(xdg_data.as_deref(), home.as_deref());
        let cache_dir = resolve_cache_dir(xdg_cache.as_deref(), home.as_deref());
        let managed_bin_dir = resolve_managed_bin_dir(home.as_deref());

        let mut actual = BTreeMap::new();
        actual.insert(
            "config_path".to_string(),
            config_path.to_string_lossy().into_owned(),
        );
        actual.insert(
            "config_dir".to_string(),
            config_dir.to_string_lossy().into_owned(),
        );
        actual.insert(
            "profiles_dir".to_string(),
            profiles_dir.to_string_lossy().into_owned(),
        );
        if let Some(d) = data_dir {
            actual.insert("data_dir".to_string(), d.to_string_lossy().into_owned());
        } else {
            actual.insert("data_dir".to_string(), "(error)".to_string());
        }
        if let Some(d) = audit_dir {
            actual.insert("audit_dir".to_string(), d.to_string_lossy().into_owned());
        } else {
            actual.insert("audit_dir".to_string(), "(error)".to_string());
        }
        if let Some(d) = patterns_dir {
            actual.insert("patterns_dir".to_string(), d.to_string_lossy().into_owned());
        } else {
            actual.insert("patterns_dir".to_string(), "(error)".to_string());
        }
        if let Some(d) = cache_dir {
            actual.insert("cache_dir".to_string(), d.to_string_lossy().into_owned());
        } else {
            actual.insert("cache_dir".to_string(), "(error)".to_string());
        }
        if let Some(d) = managed_bin_dir {
            actual.insert(
                "managed_bin_dir".to_string(),
                d.to_string_lossy().into_owned(),
            );
        } else {
            actual.insert("managed_bin_dir".to_string(), "(error)".to_string());
        }

        let normalize_separators = |paths: &BTreeMap<String, String>| {
            paths
                .iter()
                .map(|(key, value)| (key.clone(), value.replace('\\', "/")))
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(
            normalize_separators(&actual),
            normalize_separators(&case.expect),
            "case {} differs from Go oracle",
            case.id
        );
    }
}
