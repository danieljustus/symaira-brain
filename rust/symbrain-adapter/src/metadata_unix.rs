use cap_fs_ext::{FollowSymlinks, OpenOptionsExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};
use rustix::buffer::spare_capacity;
use rustix::fs::{self, XattrFlags};
use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

const INITIAL_XATTR_BUFFER: usize = 4096;
const MAX_XATTR_BUFFER: usize = 16 * 1024 * 1024;

pub(super) fn copy(parent: &Dir, name: &Path, destination: &File) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    options.custom_flags(libc::O_NONBLOCK);
    let source = parent.open_with(name, &options)?;
    let stat = fs::fstat(&source).map_err(io::Error::from)?;
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "metadata source is not a regular file",
        ));
    }

    // fchmod preserves the complete Unix mode word, including set-id and
    // sticky bits. The source is opened read-only and is never chmod'ed.
    fs::fchmod(destination, (stat.st_mode & 0o7777).into()).map_err(io::Error::from)?;
    for name in xattr_names(&source)? {
        #[cfg(target_os = "macos")]
        if name == b"com.apple.provenance" {
            // macOS owns this attribute and rejects copying it to a new inode.
            // It is outside the user xattr/ACL preservation contract.
            continue;
        }
        let value = xattr_value(&source, &name)?;
        fs::fsetxattr(
            destination,
            OsStr::from_bytes(&name),
            &value,
            XattrFlags::empty(),
        )
        .map_err(io::Error::from)?;
    }
    Ok(())
}

fn xattr_names(source: &File) -> io::Result<Vec<Vec<u8>>> {
    let mut capacity = INITIAL_XATTR_BUFFER;
    loop {
        let mut buffer = vec![0_u8; capacity];
        match fs::flistxattr(source, &mut buffer) {
            Ok(length) => {
                return Ok(buffer[..length]
                    .split(|byte| *byte == 0)
                    .filter(|name| !name.is_empty())
                    .map(ToOwned::to_owned)
                    .collect());
            }
            Err(error) if error == rustix::io::Errno::RANGE && capacity < MAX_XATTR_BUFFER => {
                capacity *= 2;
            }
            Err(error) => return Err(io::Error::from(error)),
        }
    }
}

fn xattr_value(source: &File, name: &[u8]) -> io::Result<Vec<u8>> {
    let mut value = Vec::with_capacity(INITIAL_XATTR_BUFFER);
    loop {
        let length =
            match fs::fgetxattr(source, OsStr::from_bytes(name), spare_capacity(&mut value)) {
                Ok(length) => length,
                Err(error)
                    if error == rustix::io::Errno::RANGE && value.capacity() < MAX_XATTR_BUFFER =>
                {
                    value.reserve(value.capacity());
                    continue;
                }
                Err(error) => return Err(io::Error::from(error)),
            };
        value.truncate(length);
        return Ok(value);
    }
}
