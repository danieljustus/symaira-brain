//! Search order for child binaries.
//!
//! The built-in workers ship in the same release bundle as `symbrain`, so they
//! are looked up next to the running executable first (after resolving
//! symlinks, which makes a Homebrew `bin/symbrain` link land in the Cellar).
//! Every child, workers included, then falls back to the managed directory
//! (`~/.symaira/bin`) and finally `PATH`.

use std::path::{Path, PathBuf};

use crate::client::find_in_dir;

/// Workers that every Brain release bundles next to `symbrain`.
const BUNDLED_WORKERS: [&str; 3] = ["symbrowse", "symoperate", "symscope"];

/// The directories a lookup may consult, in lookup priority apart from the
/// executable directory, which only applies to bundled workers.
pub(crate) struct Search {
    pub(crate) executable_dir: Option<PathBuf>,
    pub(crate) managed_dir: Option<PathBuf>,
    pub(crate) path_dirs: Vec<PathBuf>,
}

impl Search {
    pub(crate) fn from_environment() -> Self {
        Self {
            executable_dir: std::env::current_exe()
                .ok()
                .and_then(|executable| executable_dir_of(&executable)),
            managed_dir: std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".symaira").join("bin")),
            path_dirs: std::env::var_os("PATH")
                .map(|paths| std::env::split_paths(&paths).collect())
                .unwrap_or_default(),
        }
    }

    /// First usable executable named `binary`, searching in the documented order.
    pub(crate) fn find(&self, binary: &str) -> Option<PathBuf> {
        let bundled = BUNDLED_WORKERS.contains(&binary);
        bundled
            .then_some(self.executable_dir.as_ref())
            .flatten()
            .into_iter()
            .chain(self.managed_dir.as_ref())
            .chain(self.path_dirs.iter())
            .find_map(|directory| find_in_dir(directory, binary))
    }
}

/// Directory that holds `executable` after resolving symlinks.
pub(crate) fn executable_dir_of(executable: &Path) -> Option<PathBuf> {
    executable
        .canonicalize()
        .ok()
        .and_then(|resolved| resolved.parent().map(Path::to_path_buf))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;

    fn install(directory: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(directory).expect("create directory");
        let path = directory.join(name);
        fs::write(&path, "#!/bin/sh\n").expect("write binary");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    fn search(root: &Path) -> Search {
        Search {
            executable_dir: Some(root.join("exe")),
            managed_dir: Some(root.join("managed")),
            path_dirs: vec![root.join("path-a"), root.join("path-b")],
        }
    }

    #[test]
    fn bundled_workers_prefer_the_executable_directory_over_managed_and_path() {
        for worker in BUNDLED_WORKERS {
            let root = tempfile::tempdir().expect("temp dir");
            let search = search(root.path());
            let on_path = install(&root.path().join("path-b"), worker);
            assert_eq!(search.find(worker), Some(on_path), "{worker}: PATH only");
            let managed = install(&root.path().join("managed"), worker);
            assert_eq!(
                search.find(worker),
                Some(managed),
                "{worker}: managed beats PATH"
            );
            let beside = install(&root.path().join("exe"), worker);
            assert_eq!(search.find(worker), Some(beside), "{worker}: exe dir first");
        }
    }

    #[test]
    fn falls_back_to_managed_then_path_when_nothing_sits_next_to_the_executable() {
        let root = tempfile::tempdir().expect("temp dir");
        let search = search(root.path());
        install(&root.path().join("exe"), "unrelated");
        let on_path = install(&root.path().join("path-b"), "symscope");
        assert_eq!(search.find("symscope"), Some(on_path));
        let managed = install(&root.path().join("managed"), "symscope");
        assert_eq!(search.find("symscope"), Some(managed));
        assert_eq!(search.find("symoperate"), None);
    }

    #[test]
    fn path_entries_keep_their_order() {
        let root = tempfile::tempdir().expect("temp dir");
        let search = search(root.path());
        let first = install(&root.path().join("path-a"), "symbrowse");
        install(&root.path().join("path-b"), "symbrowse");
        assert_eq!(search.find("symbrowse"), Some(first));
    }

    #[test]
    fn other_children_ignore_the_executable_directory() {
        let root = tempfile::tempdir().expect("temp dir");
        let search = search(root.path());
        install(&root.path().join("exe"), "symvault");
        install(&root.path().join("exe"), "foreign-server");
        assert_eq!(search.find("symvault"), None);
        assert_eq!(search.find("foreign-server"), None);
        let on_path = install(&root.path().join("path-a"), "symvault");
        assert_eq!(search.find("symvault"), Some(on_path));
    }

    #[test]
    fn non_executable_files_next_to_the_executable_are_skipped() {
        let root = tempfile::tempdir().expect("temp dir");
        let search = search(root.path());
        let plain = root.path().join("exe").join("symbrowse");
        fs::create_dir_all(plain.parent().expect("parent")).expect("create");
        fs::write(&plain, "not executable").expect("write");
        let on_path = install(&root.path().join("path-a"), "symbrowse");
        assert_eq!(search.find("symbrowse"), Some(on_path));
    }

    #[test]
    fn a_symlinked_executable_resolves_into_its_real_directory() {
        let root = tempfile::tempdir().expect("temp dir");
        let cellar = root.path().join("Cellar").join("symbrain").join("bin");
        let real = install(&cellar, "symbrain");
        let sibling = install(&cellar, "symbrowse");
        let homebrew_bin = root.path().join("homebrew").join("bin");
        fs::create_dir_all(&homebrew_bin).expect("create bin");
        let link = homebrew_bin.join("symbrain");
        symlink(&real, &link).expect("symlink");

        let resolved = executable_dir_of(&link).expect("resolved directory");
        assert_eq!(resolved, cellar.canonicalize().expect("canonical cellar"));
        let search = Search {
            executable_dir: Some(resolved),
            managed_dir: None,
            path_dirs: vec![homebrew_bin],
        };
        assert_eq!(
            search
                .find("symbrowse")
                .map(|path| path.canonicalize().expect("canonical")),
            Some(sibling.canonicalize().expect("canonical sibling"))
        );
    }

    #[test]
    fn missing_executable_has_no_directory() {
        assert_eq!(executable_dir_of(Path::new("/nonexistent/symbrain")), None);
    }
}
