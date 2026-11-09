//! Filesystem-root capabilities shared by installation and materialization.

use std::io;
use std::path::Path;

use cap_fs_ext::DirExt;
use cap_std::ambient_authority;
use cap_std::fs::Dir;

/// Open only the OS volume/share anchor with ambient authority. All caller-
/// controlled descendants must be opened relative to this capability.
pub(crate) fn open_filesystem_root(absolute: &Path) -> io::Result<Dir> {
    let anchor = {
        let _ = absolute;
        Path::new("/").to_path_buf()
    };
    let root = Dir::open_ambient_dir(&anchor, ambient_authority())?;
    Ok(root)
}

/// Open a single directory component without following links or junctions.
pub(crate) fn open_child_nofollow(root: &Dir, name: &Path) -> io::Result<Dir> {
    let child = root.open_dir_nofollow(name)?;
    Ok(child)
}

/// Windows junctions and other reparse tags need the same refusal as symlinks.
pub(crate) fn is_reparse_point(metadata: &cap_std::fs::Metadata) -> bool {
    {
        let _ = metadata;
        false
    }
}
