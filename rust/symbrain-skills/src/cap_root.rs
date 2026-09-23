//! Filesystem-root capabilities shared by installation and materialization.

use std::io;
use std::path::Path;

use cap_fs_ext::DirExt;
use cap_std::ambient_authority;
use cap_std::fs::Dir;

/// Open only the OS volume/share anchor with ambient authority. All caller-
/// controlled descendants must be opened relative to this capability.
pub(crate) fn open_filesystem_root(absolute: &Path) -> io::Result<Dir> {
    #[cfg(windows)]
    let anchor = windows_volume_root(absolute)?;
    #[cfg(not(windows))]
    let anchor = {
        let _ = absolute;
        Path::new("/").to_path_buf()
    };
    let root = Dir::open_ambient_dir(&anchor, ambient_authority())?;
    #[cfg(windows)]
    reject_reparse_point(&root)?;
    Ok(root)
}

/// Open a single directory component without following links or junctions.
pub(crate) fn open_child_nofollow(root: &Dir, name: &Path) -> io::Result<Dir> {
    let child = root.open_dir_nofollow(name)?;
    #[cfg(windows)]
    reject_reparse_point(&child)?;
    Ok(child)
}

/// Windows junctions and other reparse tags need the same refusal as symlinks.
pub(crate) fn is_reparse_point(metadata: &cap_std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

#[cfg(windows)]
fn windows_volume_root(absolute: &Path) -> io::Result<std::path::PathBuf> {
    use std::path::{Component, PathBuf};

    let mut components = absolute.components();
    let (Some(Component::Prefix(prefix)), Some(Component::RootDir)) =
        (components.next(), components.next())
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "absolute Windows path needs a drive or UNC share root",
        ));
    };
    let mut anchor = PathBuf::from(prefix.as_os_str());
    anchor.push(Path::new(r"\"));
    Ok(anchor)
}

#[cfg(windows)]
fn reject_reparse_point(directory: &Dir) -> io::Result<()> {
    // FILE_ATTRIBUTE_REPARSE_POINT: checking the opened handle, not an
    // ambient path, also rejects junctions and non-symlink reparse tags.
    if is_reparse_point(&directory.dir_metadata()?) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "directory is a reparse point",
        ));
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn drive_and_unc_roots_preserve_their_volume() {
        for (path, root) in [
            (r"C:\Users\demo", r"C:\"),
            (r"\\server\share\folder", r"\\server\share\"),
            (r"\\?\C:\Users\demo", r"\\?\C:\"),
        ] {
            assert_eq!(
                windows_volume_root(Path::new(path)).unwrap(),
                Path::new(root)
            );
        }
    }
}
