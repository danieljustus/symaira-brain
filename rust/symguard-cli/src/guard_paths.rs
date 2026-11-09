//! Guard-only lexical paths, shared by scan discovery and doctor configuration.
use std::path::Component;
use std::path::{Path, PathBuf};

/// Join generated paths without interpreting symlinks or creating new volumes.
pub(crate) fn join_native_path(base: &Path, parts: &[&str]) -> PathBuf {
    {
        clean_native_path(
            &parts
                .iter()
                .fold(base.to_path_buf(), |path, part| path.join(part)),
        )
    }
}

/// Clean a path like Go `filepath.Clean`, including slash-form Windows input.
pub(crate) fn clean_native_path(path: &Path) -> PathBuf {
    clean_unix_path(path)
}

fn clean_unix_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                result.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if result.file_name().is_some_and(|name| name != "..") {
                    result.pop();
                } else if !result.has_root() {
                    result.push(component.as_os_str());
                }
            }
        }
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}
