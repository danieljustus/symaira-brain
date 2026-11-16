//! CFG-002: HOME and every XDG variable present, absent, empty or relative
//! resolve to the config/cache/state paths of Go `config.DefaultPaths` (a
//! `filepath.Join`-cleaned path; an empty variable counts as unset). The same
//! cases run as byte-level observations of the Go binary in
//! `symbrowse-cli/tests/fixtures/cli/cli_oracle.json` (`xdg-*`, `home-*`).
#![deny(unsafe_code)]

use std::{collections::HashMap, path::PathBuf};

use symbrowse_core::config::{FlagOverrides, LoadContext, go_clean, go_join, load};

fn context(home: &str, xdg: [Option<&str>; 3]) -> LoadContext {
    LoadContext {
        home: PathBuf::from(home),
        cwd: PathBuf::from("/work"),
        xdg_config_home: xdg[0].map(PathBuf::from),
        xdg_cache_home: xdg[1].map(PathBuf::from),
        xdg_state_home: xdg[2].map(PathBuf::from),
        env: HashMap::new(),
        flags: FlagOverrides::default(),
    }
}

fn dirs(context: &LoadContext) -> (String, String, String, String) {
    let result = load(context).expect("configuration loads");
    (
        result.config.config_dir,
        result.config.cache_dir,
        result.config.state_dir,
        result.config.daemon_log,
    )
}

#[test]
fn defaults_come_from_home() {
    assert_eq!(
        dirs(&context("/home/u", [None, None, None])),
        (
            "/home/u/.config/symbrowse".into(),
            "/home/u/.cache/symbrowse".into(),
            "/home/u/.local/state/symbrowse".into(),
            "/home/u/.local/state/symbrowse/daemon.log".into(),
        )
    );
}

#[test]
fn each_xdg_variable_moves_only_its_own_directory() {
    assert_eq!(
        dirs(&context("/home/u", [Some("/c"), None, None])),
        (
            "/c/symbrowse".into(),
            "/home/u/.cache/symbrowse".into(),
            "/home/u/.local/state/symbrowse".into(),
            "/home/u/.local/state/symbrowse/daemon.log".into(),
        )
    );
    assert_eq!(
        dirs(&context("/home/u", [None, Some("/ca"), None])).1,
        "/ca/symbrowse"
    );
    let state = dirs(&context("/home/u", [None, None, Some("/s")]));
    assert_eq!(
        (state.2.as_str(), state.3.as_str()),
        ("/s/symbrowse", "/s/symbrowse/daemon.log")
    );
}

#[test]
fn empty_xdg_variables_count_as_unset() {
    assert_eq!(
        dirs(&context("/home/u", [Some(""), Some(""), Some("")])),
        dirs(&context("/home/u", [None, None, None]))
    );
}

#[test]
fn xdg_and_home_values_are_cleaned_like_filepath_join() {
    for (xdg, want) in [
        ("/x/", "/x/symbrowse"),
        ("/x//y", "/x/y/symbrowse"),
        ("/a/../x/./y", "/x/y/symbrowse"),
        ("/", "/symbrowse"),
        ("rel", "rel/symbrowse"),
        ("./rel", "rel/symbrowse"),
        ("../rel", "../rel/symbrowse"),
        ("a/..", "symbrowse"),
        ("/..", "/symbrowse"),
    ] {
        assert_eq!(
            dirs(&context("/home/u", [Some(xdg), None, None])).0,
            want,
            "XDG_CONFIG_HOME={xdg:?}"
        );
    }
    assert_eq!(
        dirs(&context("/home//u/./x/..", [None, None, None])).0,
        "/home/u/.config/symbrowse"
    );
}

#[test]
fn go_clean_and_join_match_the_go_standard_library() {
    for (input, want) in [
        ("", "."),
        (".", "."),
        ("/", "/"),
        ("//", "/"),
        ("a/b/../c", "a/c"),
        ("a/../..", ".."),
        ("../../a", "../../a"),
        ("/../a", "/a"),
        ("a//b/./c/", "a/b/c"),
        ("abc/def/../../..", ".."),
    ] {
        assert_eq!(go_clean(input), want, "Clean({input:?})");
    }
    assert_eq!(go_join(&[]), "");
    assert_eq!(go_join(&["", ""]), "");
    assert_eq!(go_join(&["", "a", "", "b"]), "a/b");
    assert_eq!(go_join(&["a", "../b"]), "b");
    assert_eq!(go_join(&["/a", "..", ".."]), "/");
}
