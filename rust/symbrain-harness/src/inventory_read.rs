use std::path::{Path, PathBuf};

use crate::go_read::ReadFailure;
use crate::{ConfigInventory, Format, Harness, ServerInfo};

pub(super) fn inspect(
    harness: &Harness,
    path: Option<PathBuf>,
    capability: Option<(PathBuf, PathBuf)>,
) -> ConfigInventory {
    let Some(path) = path else {
        return ConfigInventory {
            path: String::new(),
            raw_path: PathBuf::new(),
            exists: false,
            parsed: false,
            error: Some("resolve config path".into()),
            servers: Vec::new(),
        };
    };
    let path_string = path_json_text(&path);
    let bytes = match crate::go_read::read_config(&path, capability) {
        Ok(bytes) => bytes,
        Err(ReadFailure::NotFound) => {
            return ConfigInventory {
                path: path_string,
                raw_path: path,
                exists: false,
                parsed: false,
                error: None,
                servers: Vec::new(),
            };
        }
        Err(ReadFailure::Message(message)) => {
            // Go sets `Exists` only after a successful read.
            return ConfigInventory {
                path: path_string,
                raw_path: path,
                exists: false,
                parsed: false,
                error: Some(message),
                servers: Vec::new(),
            };
        }
    };
    let servers = match decode_servers(harness, &bytes) {
        Ok(servers) => servers,
        Err(detail) => {
            return ConfigInventory {
                path: path_string,
                raw_path: path,
                exists: true,
                parsed: false,
                error: Some(crate::inventory::parse_error_message(harness, &detail)),
                servers: Vec::new(),
            };
        }
    };
    ConfigInventory {
        path: path_string,
        raw_path: path,
        exists: true,
        parsed: true,
        error: None,
        servers,
    }
}

/// Go's JSON encoder replaces each invalid byte, not each invalid sequence.
fn path_json_text(path: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut bytes = path.as_os_str().as_bytes();
        let mut text = String::new();
        while !bytes.is_empty() {
            match std::str::from_utf8(bytes) {
                Ok(valid) => {
                    text.push_str(valid);
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    text.push_str(&String::from_utf8_lossy(&bytes[..valid]));
                    text.push('\u{fffd}');
                    bytes = &bytes[valid + 1..];
                }
            }
        }
        text
    }
    #[cfg(not(unix))]
    path.to_string_lossy().into_owned()
}

/// Decodes a config exactly like Go's `Parse` and lists its servers.
fn decode_servers(harness: &Harness, bytes: &[u8]) -> Result<Vec<ServerInfo>, String> {
    let key = harness.server_key().unwrap_or_default();
    match harness.format {
        Format::Json => {
            let root = crate::go_json::decode_object(bytes)?;
            let Some(servers) = crate::json::server_map(&root, key) else {
                return Ok(Vec::new());
            };
            let mut names = servers.keys.clone();
            names.sort();
            Ok(names
                .iter()
                .filter_map(|name| {
                    servers
                        .get(name)
                        .and_then(|value| crate::json::server_info(name, value))
                })
                .collect())
        }
        Format::Toml => Ok(crate::go_toml::server_infos(
            &crate::go_toml::decode_object(bytes)?,
            key,
        )),
        Format::Unsupported => Err("harness does not support MCP configuration".to_owned()),
    }
}

/// Lexical equivalent of Go's `filepath.Clean`: no filesystem access, `.` and
/// empty components dropped, `..` folded into a preceding real component,
/// leading `..` kept, `..` at a root dropped, and an empty result spelled `.`.
pub(crate) fn clean_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => match result.components().next_back() {
                Some(std::path::Component::Normal(_)) => {
                    result.pop();
                }
                Some(std::path::Component::ParentDir) | None => result.push(".."),
                _ => {}
            },
            component => result.push(component.as_os_str()),
        }
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

pub(super) fn project_root_error(path: &Path) -> Option<String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => None,
        Ok(_) => Some("project root is not a real directory".to_owned()),
        Err(error) => Some(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::clean_path;
    use std::path::Path;

    #[test]
    fn clean_path_preserves_leading_parent_components() {
        assert_eq!(
            clean_path(Path::new("../../sibling")),
            Path::new("../../sibling")
        );
        assert_eq!(
            clean_path(Path::new("a/../../sibling")),
            Path::new("../sibling")
        );
        assert_eq!(
            clean_path(Path::new("./project/../sibling")),
            Path::new("sibling")
        );
    }

    #[test]
    fn clean_path_matches_go_filepath_clean() {
        // Expected values are Go's `filepath.Clean` results.
        for (input, want) in [
            ("", "."),
            (".", "."),
            ("./", "."),
            ("./.", "."),
            ("./x/../", "."),
            ("a//b", "a/b"),
            ("a/b/", "a/b"),
            ("a/./b/.", "a/b"),
            ("a/b/../..", "."),
            ("..", ".."),
            ("../", ".."),
            ("../..", "../.."),
            ("/", "/"),
            ("//", "/"),
            ("/..", "/"),
            ("/../x", "/x"),
            ("/a/../..", "/"),
            ("/a//b/", "/a/b"),
        ] {
            // Path equality ignores repeated and trailing separators, so
            // compare the spelling.
            let want = if cfg!(windows) {
                want.replace('/', "\\")
            } else {
                want.to_owned()
            };
            assert_eq!(
                clean_path(Path::new(input)).to_str().unwrap(),
                want,
                "{input:?}"
            );
        }
    }
}
