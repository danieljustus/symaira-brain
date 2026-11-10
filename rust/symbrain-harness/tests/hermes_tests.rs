//! Hermes config discovery: `HERMES_HOME`, `~/.hermes` and profile configs.

use std::ffi::OsString;
use std::fs;

use symbrain_harness::locations_from;

fn os(value: &std::path::Path) -> OsString {
    value.as_os_str().to_owned()
}

#[test]
fn defaults_to_home_dot_hermes_without_profiles() {
    let home = tempfile::tempdir().unwrap();
    let found = locations_from(None, Some(os(home.path()))).unwrap();
    assert_eq!(found.root, home.path().join(".hermes"));
    assert_eq!(found.global, home.path().join(".hermes/config.yaml"));
    assert!(found.profiles.is_empty());
}

#[test]
fn hermes_home_wins_over_home_and_empty_values_are_ignored() {
    let home = tempfile::tempdir().unwrap();
    let alt = tempfile::tempdir().unwrap();
    let found = locations_from(Some(os(alt.path())), Some(os(home.path()))).unwrap();
    assert_eq!(found.global, alt.path().join("config.yaml"));
    let empty = locations_from(Some(OsString::new()), Some(os(home.path()))).unwrap();
    assert_eq!(empty.root, home.path().join(".hermes"));
    assert!(locations_from(None, None).is_none());
    assert!(locations_from(Some(OsString::new()), Some(OsString::new())).is_none());
}

#[test]
fn profiles_need_a_config_file_and_are_sorted_without_hidden_entries() {
    let alt = tempfile::tempdir().unwrap();
    for name in ["zeta", "alpha", ".hidden", "no-config"] {
        fs::create_dir_all(alt.path().join("profiles").join(name)).unwrap();
    }
    for name in ["zeta", "alpha", ".hidden"] {
        fs::write(
            alt.path().join("profiles").join(name).join("config.yaml"),
            "a: 1\n",
        )
        .unwrap();
    }
    fs::write(alt.path().join("profiles/stray-file"), "x").unwrap();
    let found = locations_from(Some(os(alt.path())), None).unwrap();
    let names: Vec<_> = found.profiles.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["alpha", "zeta"]);
    assert_eq!(
        found.profiles[0].config,
        alt.path().join("profiles/alpha/config.yaml")
    );
}
