use std::path::Path;

use cap_std::fs::Dir;
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
    let mode = permissions.mode() & 0o777;
    write_bytes(root, path, bytes, mode, fault)
}
