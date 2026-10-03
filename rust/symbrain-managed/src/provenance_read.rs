//! Read-only file handles and Go-compatible open/read error ownership.
use std::path::Path;

#[cfg(not(windows))]
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

#[cfg(windows)]
pub(super) fn read(path: &Path) -> Result<Vec<u8>, (&'static str, std::io::Error)> {
    use std::io::Read;
    use std::os::windows::fs::OpenOptionsExt;

    // Go1.26.7 syscall.Open permits read-only directory handles and shares
    // reads/writes, but not deletes. Preserve actual open versus read failures;
    // a metadata precheck or synthesized error could hide access/race failures.
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_SHARE_READ_WRITE: u32 = 3;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|error| ("open", error))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| ("read", error))?;
    Ok(bytes)
}
