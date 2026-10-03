//! Preserve the proven directory-open diagnostic without reopening audit paths.
use std::{io, path::Path};

pub(super) fn render(error: &io::Error, path: &Path) -> String {
    #[cfg(unix)]
    if error.raw_os_error() == Some(21) {
        return format!(
            "decide: open audit log: open {}: is a directory",
            path.display()
        );
    }
    // Keep capability/no-follow/short-write failures honest. Windows stage and
    // syscall spelling still require native proof rather than an ambient stat.
    #[cfg(not(unix))]
    let _ = path;
    format!("decide: write audit log: {error}")
}
