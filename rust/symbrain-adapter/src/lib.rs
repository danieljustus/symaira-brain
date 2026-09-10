//! Existing-file-aware instruction adapters derived from the harness registry.
#![deny(missing_docs)]
#![deny(unsafe_code)]

mod atomic;
#[cfg(unix)]
mod metadata_unix;
#[cfg(windows)]
mod metadata_windows;
mod path;
mod target;

pub use atomic::{AtomicFile, write_atomic};
pub use path::validate_relative_target_path;
pub use target::{
    AdapterError, CLAUDE_POINTER, CURSOR_HEADER, Rendered, Target, adapter_for_harness, adapters,
    all_targets, render, render_for_harness, target_for_harness,
};
