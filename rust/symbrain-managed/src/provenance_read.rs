//! Read-only file handles and Go-compatible open/read error ownership.
use std::path::Path;

pub(super) fn read(path: &Path) -> Result<Vec<u8>, (&'static str, std::io::Error)> {
    std::fs::read(path).map_err(|error| {
        let operation = if error.kind() == std::io::ErrorKind::IsADirectory {
            "read"
        } else {
            "open"
        };
        (operation, error)
    })
}
