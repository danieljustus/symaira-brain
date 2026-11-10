//! Race-tolerant filesystem snapshot walking for oracle tests.
//!
//! A tree walk lists a directory and then stats each name. A lock or staging
//! file that a finished process removes in between makes the stat fail with
//! `NotFound`; that entry is simply gone and must not fail the snapshot. Every
//! other error stays fatal so real filesystem problems are still caught.
use std::io;
use std::path::Path;

/// Unwraps `result`, or returns `None` when the entry vanished (`NotFound`).
///
/// # Panics
/// Panics, naming `path`, on any error other than `NotFound`.
pub fn skip_vanished<T>(result: io::Result<T>, path: &Path) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => panic!("{}: {error}", path.display()),
    }
}
