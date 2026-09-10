mod atomic;
mod core;
mod paths;
mod replace;

/// Maximum configuration size accepted by the install/uninstall path.
pub const MAX_CONFIG_BYTES: usize = 8 * 1024 * 1024;

const MAX_TEMP_ATTEMPTS: u32 = 1000;
#[cfg(windows)]
const MAX_WINDOWS_REPLACE_ATTEMPTS: u32 = 8;

pub use atomic::{atomic_write, backup_path, create_private_dir_all};
pub use core::{AtomicFile, FileSnapshot};
