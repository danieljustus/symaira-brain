use super::*;
use std::collections::BTreeMap;

fn doc(text: &str) -> DocumentMut {
    text.parse().expect("valid TOML")
}

fn resolved(global: &str, project: &str, env: &[(&str, &str)]) -> Option<PathBuf> {
    let env: BTreeMap<String, OsString> = env
        .iter()
        .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
        .collect();
    resolve(
        &doc(global),
        &doc(project),
        "SYMBRAIN_SERVERS_VAULT_BINARY_PATH",
        |name| env.get(name).cloned(),
    )
}

type Case<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)], Option<&'a str>);

const PATH: &str = "[servers.vault]\nbinary_path = \"/g/symvault\"\n";

#[test]
fn zero_values_are_converted_not_skipped() {
    // CoreKit v0.18.2: these were accepted by the v0.17 model and are now
    // rejected, which disables every override.
    for bad in [
        "[modules]\nbrowse = 0\n",
        "[modules]\nbrowse = \"\"\n",
        "[audit]\nenabled = 0\n",
        "[audit]\nverbose = 0.0\n",
        "[patterns]\npromotion_threshold = false\n",
        "default_profile = 0\n",
        "default_profile = false\n",
        "[servers.operate]\nbinary_path = 0\n",
    ] {
        assert_eq!(resolved(&format!("{bad}{PATH}"), "", &[]), None, "{bad}");
        assert_eq!(resolved("", &format!("{bad}{PATH}"), &[]), None, "{bad}");
    }
}

#[test]
fn valid_typed_values_keep_the_override() {
    for good in [
        "[modules]\nbrowse = false\n",
        "[modules]\nbrowse = \"T\"\n",
        "[patterns]\npromotion_threshold = 0\n",
        "[patterns]\npromotion_threshold = 1.5\n",
        "[patterns]\npromotion_threshold = \"-3\"\n",
        "default_profile = \"\"\n",
    ] {
        assert_eq!(
            resolved(&format!("{good}{PATH}"), "", &[]),
            Some(PathBuf::from("/g/symvault")),
            "{good}"
        );
    }
    assert_eq!(
        resolved(
            &format!("{PATH}[patterns]\npromotion_threshold = \"1_0\"\n"),
            "",
            &[]
        ),
        None
    );
}

#[test]
fn present_empty_environment_is_converted_like_lookup_env() {
    for (name, value, expected) in [
        ("SYMBRAIN_MODULES_BROWSE", "", None),
        ("SYMBRAIN_AUDIT_ENABLED", "", None),
        ("SYMBRAIN_PATTERNS_PROMOTION_THRESHOLD", "", None),
        ("SYMBRAIN_PATTERNS_PROMOTION_THRESHOLD", " 3", None),
        (
            "SYMBRAIN_PATTERNS_PROMOTION_THRESHOLD",
            "3",
            Some("/g/symvault"),
        ),
        ("SYMBRAIN_MODULES_SCOPE", "maybe", None),
        ("SYMBRAIN_MODULES_SCOPE", "False", Some("/g/symvault")),
        ("SYMBRAIN_DEFAULT_PROFILE", "", Some("/g/symvault")),
    ] {
        assert_eq!(
            resolved(PATH, "", &[(name, value)]),
            expected.map(PathBuf::from),
            "{name}={value:?}"
        );
    }
}

#[test]
fn binary_path_layers_replace_earlier_ones_even_when_empty() {
    let project = "[servers.vault]\nbinary_path = \"/p/symvault\"\n";
    let empty = "[servers.vault]\nbinary_path = \"\"\n";
    let cases: [Case<'_>; 6] = [
        (PATH, "", &[], Some("/g/symvault")),
        (PATH, project, &[], Some("/p/symvault")),
        (PATH, empty, &[], None),
        ("", project, &[], Some("/p/symvault")),
        (
            PATH,
            project,
            &[("SYMBRAIN_SERVERS_VAULT_BINARY_PATH", "/e/symvault")],
            Some("/e/symvault"),
        ),
        (
            PATH,
            project,
            &[("SYMBRAIN_SERVERS_VAULT_BINARY_PATH", "")],
            None,
        ),
    ];
    for (global, project, env, expected) in cases {
        assert_eq!(
            resolved(global, project, env),
            expected.map(PathBuf::from),
            "{global}|{project}|{env:?}"
        );
    }
}
