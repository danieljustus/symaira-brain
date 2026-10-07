//! Guard-only lexical paths, shared by scan discovery and doctor configuration.
#[cfg(not(windows))]
use std::path::Component;
use std::path::{Path, PathBuf};
#[cfg(any(windows, test))]
#[path = "guard_path_windows.rs"]
mod windows_paths;

/// Join generated paths without interpreting symlinks or creating new volumes.
pub(crate) fn join_native_path(base: &Path, parts: &[&str]) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let units: Vec<u16> = base.as_os_str().encode_wide().collect();
        let joined = std::ffi::OsString::from_wide(&windows_paths::join_units(&units, parts));
        clean_native_path(Path::new(&joined))
    }
    #[cfg(not(windows))]
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
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        // Go 1.26 preserves lone surrogates through WTF8; keep native UTF16 units.
        let units: Vec<u16> = path.as_os_str().encode_wide().collect();
        PathBuf::from(std::ffi::OsString::from_wide(&windows_paths::clean_units(
            &units,
        )))
    }
    #[cfg(not(windows))]
    clean_unix_path(path)
}

#[cfg(not(windows))]
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
