use std::path::Path;

use cap_std::fs::Dir;
#[cfg(unix)]
use cap_std::fs::PermissionsExt;

use super::replace::{FaultPoint, write_bytes};
use crate::model::SkillError;

pub(crate) fn write_bounded(
    root: &Dir,
    path: &Path,
    bytes: &[u8],
    permissions: &cap_std::fs::Permissions,
    _mode: u32,
    fault: Option<FaultPoint>,
) -> Result<(), SkillError> {
    #[cfg(unix)]
    let mode = permissions.mode() & 0o777;
    #[cfg(not(unix))]
    let mode = 0o644;
    #[cfg(not(unix))]
    let _ = permissions;
    write_bytes(root, path, bytes, mode, fault)
}
