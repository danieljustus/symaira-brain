//! Preserve the proven directory-open diagnostic without reopening audit paths.
use std::{io, path::Path};
use symbrain_core::GoText;

pub(super) fn render(error: &io::Error, path: &Path) -> GoText {
    #[cfg(unix)]
    if error.raw_os_error() == Some(21) {
        return GoText::path("decide: open audit log: open ", path, ": is a directory");
    }
    // Keep capability/no-follow/short-write failures honest. Windows stage and
    // syscall spelling still require native proof rather than an ambient stat.
    #[cfg(not(unix))]
    let _ = path;
    format!("decide: write audit log: {error}").into()
}
