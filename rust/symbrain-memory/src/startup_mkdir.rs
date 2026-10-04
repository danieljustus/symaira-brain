//! Go MkdirAll's recursive failure component, separate from database safe mkdir.
use crate::startup_fs::path_error;
use std::{
    fs,
    path::{Path, PathBuf},
};
use symbrain_core::{GoText, go_path};

pub(super) fn private(path: &Path) -> Result<(), GoText> {
    // Stat follows symlinks, as os.MkdirAll does for JWT/rotation directories.
    if let Ok(metadata) = fs::metadata(path) {
        if metadata.is_dir() {
            return Ok(());
        }
        let mut bytes = b"mkdir ".to_vec();
        bytes.extend(go_path::os_bytes(path.as_os_str()));
        bytes.extend(b": not a directory");
        return Err(bytes.into());
    }
    let bytes = go_path::os_bytes(path.as_os_str());
    let separator = |byte: u8| byte == b'/' || cfg!(windows) && byte == b'\\';
    // Retain Go's original lexical prefix; Path::parent normalizes dot and
    // repeated separators and can change the failing diagnostic's raw bytes.
    let mut end = bytes.len();
    while end > 0 && separator(bytes[end - 1]) {
        end -= 1;
    }
    while end > 0 && !separator(bytes[end - 1]) {
        end -= 1;
    }
    let parent_end = end.saturating_sub(1);
    if parent_end > go_path::volume_name_len(&bytes, cfg!(windows)) {
        private(&PathBuf::from(go_path::from_bytes(&bytes[..parent_end])))?;
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    if let Err(error) = builder.create(path) {
        if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()) {
            return Ok(());
        }
        return Err(path_error("mkdir", path, &error));
    }
    Ok(())
}
