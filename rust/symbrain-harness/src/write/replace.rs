use cap_std::fs::Dir;
use std::io;
use std::path::Path;

pub(super) fn replace_existing(parent: &Dir, temporary: &Path, target: &Path) -> io::Result<()> {
    parent.rename(temporary, parent, target)
}
