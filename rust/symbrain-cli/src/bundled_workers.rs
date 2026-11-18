//! Built-in workers that ship in the same release bundle as `symbrain`.
//!
//! Browse, Operate and Scope no longer need selecting or installing: when the
//! worker binary sits next to the running executable, the optional-module
//! settings (`modules.*`, `setup --modules`) have nothing left to decide.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Directory of the running executable after resolving symlinks, so a
/// Homebrew `bin/symbrain` link lands in the Cellar next to its workers.
fn executable_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .canonicalize()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

fn is_executable_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// True when `binary` is an executable file in `directory`.
pub(crate) fn exists_in(directory: &Path, binary: &str) -> bool {
    is_executable_file(&directory.join(binary))
}

/// Splits the optional-module selection into what still needs downloading and
/// the binaries already bundled in `directory` (kept sorted for stable notes).
pub(crate) fn without_bundled(
    enabled: BTreeMap<String, bool>,
    directory: Option<&Path>,
) -> (BTreeMap<String, bool>, Vec<String>) {
    let mut bundled = Vec::new();
    let remaining = enabled
        .into_iter()
        .filter(|(binary, _)| {
            let present = directory.is_some_and(|dir| exists_in(dir, binary));
            if present {
                bundled.push(binary.clone());
            }
            !present
        })
        .collect();
    (remaining, bundled)
}

/// `enabled` minus the cores that ship beside the running executable.
pub(crate) fn installable(
    enabled: BTreeMap<String, bool>,
) -> (BTreeMap<String, bool>, Vec<String>) {
    without_bundled(enabled, executable_dir().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(directory: &Path, name: &str, mode: u32) {
        let path = directory.join(name);
        std::fs::write(&path, "#!/bin/sh\n").expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");
    }

    fn enabled(names: &[&str]) -> BTreeMap<String, bool> {
        names
            .iter()
            .map(|name| ((*name).to_string(), true))
            .collect()
    }

    #[test]
    fn bundled_cores_are_dropped_from_the_download_selection() {
        let dir = tempfile::tempdir().expect("temp dir");
        install(dir.path(), "symbrowse", 0o755);
        let (remaining, bundled) =
            without_bundled(enabled(&["symbrowse", "symoperate"]), Some(dir.path()));
        assert_eq!(bundled, ["symbrowse"]);
        assert_eq!(remaining, enabled(&["symoperate"]));
    }

    #[test]
    fn nothing_is_dropped_without_a_usable_sibling() {
        let dir = tempfile::tempdir().expect("temp dir");
        install(dir.path(), "symbrowse", 0o644);
        let selection = enabled(&["symbrowse"]);
        assert_eq!(
            without_bundled(selection.clone(), Some(dir.path())),
            (selection.clone(), Vec::new()),
            "a non-executable file is no worker"
        );
        assert_eq!(
            without_bundled(selection.clone(), None),
            (selection, Vec::new())
        );
    }
}
