//! Go-compatible optional-core selection for release setup.
use std::collections::BTreeMap;

pub(crate) fn enabled_cores()
-> Result<BTreeMap<String, bool>, symbrain_core::config::resolved::ConfigError> {
    symbrain_core::config::resolved::load().map(|config| config.modules.enabled_cores())
}

pub(crate) fn parse_go_bool(value: &str) -> Result<bool, ()> {
    symbrain_core::config::resolved::parse_bool(value.as_bytes()).ok_or(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct OwnedSources {
        global: Vec<u8>,
        project: Vec<u8>,
    }
    impl symbrain_core::config::resolved::Sources for OwnedSources {
        fn environment(&self, name: &str) -> Option<std::ffi::OsString> {
            (name == symbrain_core::go_path::home_variable()).then(|| "owned".into())
        }
        fn current_directory(&self) -> std::io::Result<std::path::PathBuf> {
            Ok("project".into())
        }
        fn metadata(&self, _path: &std::path::Path) -> std::io::Result<()> {
            Ok(())
        }
        fn read(&self, path: &std::path::Path) -> Result<Vec<u8>, (&'static str, std::io::Error)> {
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
    fn parse_enabled_cores(
        bytes: &[u8],
        _path: &std::path::Path,
    ) -> Result<BTreeMap<String, bool>, symbrain_core::config::resolved::ConfigError> {
        let source = OwnedSources {
            global: bytes.to_vec(),
            project: Vec::new(),
        };
        symbrain_core::config::resolved::load_with(&source)
            .map(|config| config.modules.enabled_cores())
    }

    #[test]
    fn optional_browse_selection_matches_go_default_and_explicit_values() {
        let path = std::path::Path::new("config.toml");
        let empty = parse_enabled_cores(b"", path).unwrap();
        assert!(!empty.get("symbrowse").copied().unwrap_or(false));
        assert!(
            parse_enabled_cores(b"[modules]\nbrowse = false\n", path)
                .unwrap()
                .get("symbrowse")
                .is_none_or(|value| !value)
        );
        assert!(
            parse_enabled_cores(b"[modules]\nbrowse = true\n", path)
                .unwrap()
                .get("symbrowse")
                .is_some_and(|value| *value)
        );
        assert!(parse_enabled_cores(b"[modules]\nbrowse = 1\n", path).is_err());
    }

    #[test]
    fn go_parse_bool_spellings_and_empty_env_contract() {
        for value in ["1", "t", "T", "TRUE", "True", "true"] {
            assert_eq!(parse_go_bool(value), Ok(true), "{value}");
        }
        for value in ["0", "f", "F", "FALSE", "False", "false"] {
            assert_eq!(parse_go_bool(value), Ok(false), "{value}");
        }
        for value in ["", "yes", "on", "2"] {
            assert_eq!(parse_go_bool(value), Err(()), "{value}");
        }
    }

    #[test]
    fn toml_false_does_not_clear_prior_true_like_configkit() {
        let source = OwnedSources {
            global: b"[modules]\nbrowse = true\n".to_vec(),
            project: b"[modules]\nbrowse = false\n".to_vec(),
        };
        let config = symbrain_core::config::resolved::load_with(&source).unwrap();
        assert!(config.modules.browse);
    }
}
