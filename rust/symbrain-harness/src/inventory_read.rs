use std::path::{Path, PathBuf};

use crate::{AtomicFile, ConfigInventory, Harness, parse};

pub(super) fn inspect(
    harness: &Harness,
    path: Option<PathBuf>,
    capability: Option<(PathBuf, PathBuf)>,
) -> ConfigInventory {
    let Some(path) = path else {
        return ConfigInventory {
            path: String::new(),
            exists: false,
            parsed: false,
            error: Some("resolve config path".into()),
            servers: Vec::new(),
        };
    };
    let path_string = path.to_string_lossy().into_owned();
    let Some((trusted_root, relative_path)) = capability else {
        return ConfigInventory {
            path: path_string,
            exists: false,
            parsed: false,
            error: Some("resolve config capability".into()),
            servers: Vec::new(),
        };
    };
    let capability = match AtomicFile::open(&trusted_root, &relative_path, path, false) {
        Ok(capability) => capability,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ConfigInventory {
                path: path_string,
                exists: false,
                parsed: false,
                error: None,
                servers: Vec::new(),
            };
        }
        Err(error) => {
            return ConfigInventory {
                path: path_string,
                exists: false,
                parsed: false,
                error: Some(error.to_string()),
                servers: Vec::new(),
            };
        }
    };
    let snapshot = match capability.read_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ConfigInventory {
                path: path_string,
                exists: false,
                parsed: false,
                error: None,
                servers: Vec::new(),
            };
        }
        Err(error) => {
            return ConfigInventory {
                path: path_string,
                exists: true,
                parsed: false,
                error: Some(error.to_string()),
                servers: Vec::new(),
            };
        }
    };
    let document = match parse(harness, &snapshot.bytes) {
        Ok(document) => document,
        Err(error) => {
            return ConfigInventory {
                path: path_string,
                exists: true,
                parsed: false,
                error: Some(error.to_string()),
                servers: Vec::new(),
            };
        }
    };
    let servers = document
        .server_names()
        .into_iter()
        .filter_map(|name| document.server_info(&name))
        .collect();
    ConfigInventory {
        path: path_string,
        exists: true,
        parsed: true,
        error: None,
        servers,
    }
}

pub(super) fn clean_path(path: &Path) -> PathBuf {
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
}
