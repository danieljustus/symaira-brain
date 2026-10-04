//! Independent frozen-Go type witnesses and stage/owner controls.
use super::{BrainConfig, Sources, default_path_with, load_with};
use crate::go_path;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

struct Source {
    global: Vec<u8>,
    project: Vec<u8>,
    env: BTreeMap<String, OsString>,
    calls: RefCell<Vec<String>>,
    cwd_error: bool,
    stat_error: Option<io::ErrorKind>,
    read_error: Option<io::ErrorKind>,
}
impl Source {
    fn new(global: &[u8], project: &[u8]) -> Self {
        Self {
            global: global.to_vec(),
            project: project.to_vec(),
            env: BTreeMap::from([(go_path::home_variable().into(), "owned".into())]),
            calls: RefCell::new(Vec::new()),
            cwd_error: false,
            stat_error: None,
            read_error: None,
        }
    }
}
impl Sources for Source {
    fn environment(&self, name: &str) -> Option<OsString> {
        self.calls.borrow_mut().push(format!("env:{name}"));
        self.env.get(name).cloned()
    }
    fn current_directory(&self) -> io::Result<PathBuf> {
        self.calls.borrow_mut().push("cwd".into());
        if self.cwd_error {
            Err(io::ErrorKind::NotFound.into())
        } else {
            Ok("project".into())
        }
    }
    fn metadata(&self, _path: &Path) -> io::Result<()> {
        self.calls.borrow_mut().push("stat".into());
        self.stat_error.map_or(Ok(()), |kind| Err(kind.into()))
    }
    fn read(&self, path: &Path) -> Result<Vec<u8>, (&'static str, io::Error)> {
        self.calls.borrow_mut().push("read".into());
        if let Some(kind) = self.read_error {
            return Err(("read", kind.into()));
        }
        Ok(
            if path.file_name() == Some(std::ffi::OsStr::new(".symbrain.toml")) {
                &self.project
            } else {
                &self.global
            }
            .clone(),
        )
    }
}
fn decode_base64(input: &str) -> Vec<u8> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut bits = 0_u32;
    let mut count = 0;
    for byte in input.bytes().take_while(|byte| *byte != b'=') {
        let value = alphabet
            .iter()
            .position(|v| *v == byte)
            .expect("fixture base64");
        bits = (bits << 6) | u32::try_from(value).unwrap();
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push(u8::try_from((bits >> count) & 0xff).unwrap());
        }
    }
    out
}

#[test]
fn sdk_marker_is_removed_once_before_utf8_and_ordered_type_admission() {
    for prefix in [&b"\xff\xfe"[..], &b"\xfe\xff"[..], &b"\xef\xbb\xbf"[..]] {
        for project in [false, true] {
            let bytes = [prefix, b"audit.enabled=false\n"].concat();
            let source = if project {
                Source::new(b"", &bytes)
            } else {
                Source::new(&bytes, b"")
            };
            assert!(!load_with(&source).unwrap().audit.enabled);
        }
        let mut source = Source::new(&[prefix, b"audit.enabled=7\n"].concat(), b"");
        source
            .env
            .insert("SYMBRAIN_MODULES_SCOPE".into(), "bad".into());
        let error = load_with(&source).unwrap_err();
        assert!(
            error
                .text()
                .as_ref()
                .ends_with(b"field \"audit\": field \"enabled\": cannot convert int64 to bool")
        );
        assert!(!source.calls.borrow().iter().any(|call| call == "cwd"));
        assert!(
            !source
                .calls
                .borrow()
                .iter()
                .any(|call| call == "env:SYMBRAIN_MODULES_SCOPE")
        );
        for rest in [&b"[bad config"[..], &b"#\xff\n"[..]] {
            let source = Source::new(&[prefix, rest].concat(), b"");
            let error = load_with(&source).unwrap_err();
            assert!(
                error
                    .text()
                    .as_ref()
                    .windows(b"failed to parse".len())
                    .any(|part| part == b"failed to parse")
            );
            assert!(!source.calls.borrow().iter().any(|call| call == "cwd"));
        }
        // A second marker is content, not another prefix to decode as UTF16.
        if prefix.len() == 2 {
            assert!(
                load_with(&Source::new(
                    &[prefix, prefix, b"audit.enabled=false\n"].concat(),
                    b""
                ))
                .is_err()
            );
        }
    }
}
#[test]
fn all_thirteen_actual_frozen_go_conversion_failures_have_exact_field_chains() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/brain13-types-go58.json"
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 13);
    for case in cases {
        let source = Source::new(&decode_base64(case["global_b64"].as_str().unwrap()), b"");
        let error = load_with(&source).unwrap_err();
        let mut expected = b"config: failed to load ".to_vec();
        let path = go_path::os_bytes(default_path_with(&source).as_os_str());
        expected.extend_from_slice(&path);
        expected.extend_from_slice(b": global config error: failed to apply ");
        expected.extend_from_slice(&path);
        expected.extend_from_slice(b": ");
        expected.extend_from_slice(&decode_base64(
            case["expected_conversion"].as_str().unwrap(),
        ));
        assert_eq!(error.text().as_ref(), expected, "{}", case["id"]);
    }
}
#[test]
fn full_positive_value_uses_project_then_environment_and_keeps_release_mapping() {
    let mut source = Source::new(
        br#"
default_profile="global"
[audit]
enabled=false
verbose=true
[gateway]
identity_injection=false
[updatecheck]
enabled=false
[servers.vault]
binary_path="vault"
[servers.operate]
binary_path="operate"
[servers.scope]
binary_path="scope"
[patterns]
enabled=false
promotion_threshold=4.9
[modules]
browse=true
operate=true
scope=true
"#,
        b"default_profile=\"project\"\n[modules]\nbrowse=false\n",
    );
    source
        .env
        .insert("SYMBRAIN_DEFAULT_PROFILE".into(), "env".into());
    source
        .env
        .insert("SYMBRAIN_MODULES_SCOPE".into(), "false".into());
    source
        .env
        .insert("SYMBRAIN_AUDIT_VERBOSE".into(), "false".into());
    let config = load_with(&source).unwrap();
    assert_eq!(config.default_profile.as_ref(), b"env");
    assert!(
        !config.audit.enabled
            && !config.audit.verbose
            && !config.gateway.identity_injection
            && !config.updatecheck.enabled
    );
    assert_eq!(config.servers.vault.as_ref(), b"vault");
    assert_eq!(config.servers.operate.as_ref(), b"operate");
    assert_eq!(config.servers.scope.as_ref(), b"scope");
    assert!(!config.patterns.enabled);
    assert_eq!(config.patterns.promotion_threshold, 4);
    assert!(config.modules.browse && config.modules.operate && !config.modules.scope);
    assert_eq!(
        config.modules.enabled_cores(),
        BTreeMap::from([("symbrowse".into(), true)])
    );
    assert_eq!(config.modules.enabled_modules().len(), 3);
}
#[test]
fn zero_plain_values_do_not_clear_prior_values_but_pointer_false_does() {
    let config = load_with(&Source::new(
        b"default_profile=\"global\"\n[audit]\nverbose=true\n[patterns]\npromotion_threshold=5\n[modules]\nbrowse=true\n",
        b"default_profile=\"\"\n[audit]\nenabled=false\nverbose=false\n[patterns]\npromotion_threshold=0\n[modules]\nbrowse=false\n",
    )).unwrap();
    assert_eq!(config.default_profile.as_ref(), b"global");
    assert!(!config.audit.enabled && config.audit.verbose && config.modules.browse);
    assert_eq!(config.patterns.promotion_threshold, 5);
}
#[test]
fn scalar_struct_values_and_unknown_keys_are_ignored_while_empty_env_is_ignored() {
    let mut source = Source::new(b"audit=\"ignored\"\nmodules=[]\nunknown=true\n", b"");
    source
        .env
        .insert("SYMBRAIN_AUDIT_ENABLED".into(), OsString::new());
    let config = load_with(&source).unwrap();
    assert!(config.audit.enabled);
    assert!(!config.modules.browse);
}
#[test]
fn declaration_and_source_error_priority_prevents_later_admission() {
    let mut source = Source::new(
        b"[modules]\nscope=\"late\"\n[audit]\nenabled=\"early\"\n",
        b"[audit]\nenabled=\"project\"\n",
    );
    source
        .env
        .insert("SYMBRAIN_AUDIT_ENABLED".into(), "env".into());
    let error = load_with(&source).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("field \"audit\": field \"enabled\": cannot parse \"early\"")
    );
    assert!(!source.calls.borrow().iter().any(|v| v == "cwd"));
}
#[test]
fn non_notfound_stat_error_defers_to_read_but_notfound_skips_the_file() {
    let mut source = Source::new(b"", b"");
    source.stat_error = Some(io::ErrorKind::PermissionDenied);
    source.read_error = Some(io::ErrorKind::IsADirectory);
    let error = load_with(&source).unwrap_err();
    assert!(error.to_string().contains(": read "));
    assert_eq!(
        source
            .calls
            .borrow()
            .iter()
            .filter(|v| *v == "read")
            .count(),
        1
    );
    source.stat_error = Some(io::ErrorKind::NotFound);
    source.calls.borrow_mut().clear();
    load_with(&source).unwrap();
    assert!(!source.calls.borrow().iter().any(|v| v == "read"));
}
#[test]
fn failed_getwd_skips_project_and_does_not_skip_environment() {
    let mut source = Source::new(b"", b"[audit]\nenabled=\"bad\"\n");
    source.cwd_error = true;
    source
        .env
        .insert("SYMBRAIN_AUDIT_ENABLED".into(), "false".into());
    assert!(!load_with(&source).unwrap().audit.enabled);
}
#[test]
fn home_is_required_before_absolute_xdg_file_admission() {
    let mut source = Source::new(b"", b"");
    source.env.clear();
    source.env.insert(
        "XDG_CONFIG_HOME".into(),
        if cfg!(windows) { "C:\\owned" } else { "/owned" }.into(),
    );
    let error = load_with(&source).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cannot determine home directory")
    );
    assert!(
        !source
            .calls
            .borrow()
            .iter()
            .any(|v| v == "stat" || v == "read")
    );
}
#[test]
#[cfg(unix)]
fn raw_environment_string_is_kept_while_bool_error_quotes_each_original_byte() {
    use crate::config::format_go_quoted_bytes;
    use std::os::unix::ffi::OsStringExt;
    let raw = b"owned\xff\xe2\x82";
    let mut source = Source::new(b"", b"");
    source.env.insert(
        "SYMBRAIN_DEFAULT_PROFILE".into(),
        OsString::from_vec(raw.to_vec()),
    );
    assert_eq!(load_with(&source).unwrap().default_profile.as_ref(), raw);
    source.env.insert(
        "SYMBRAIN_AUDIT_ENABLED".into(),
        OsString::from_vec(raw.to_vec()),
    );
    let error = load_with(&source).unwrap_err();
    assert!(error.to_string().contains(&format_go_quoted_bytes(raw)));
}
#[test]
fn default_value_is_complete_and_threshold_nonpositive_resolves_after_all_layers() {
    let config = BrainConfig::default();
    assert!(
        config.audit.enabled
            && config.gateway.identity_injection
            && config.patterns.enabled
            && config.updatecheck.enabled
    );
    assert_eq!(
        load_with(&Source::new(b"[patterns]\npromotion_threshold=-2\n", b""))
            .unwrap()
            .patterns
            .promotion_threshold,
        3
    );
}
