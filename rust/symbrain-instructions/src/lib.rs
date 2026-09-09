//! Byte-preserving canonical instruction source and managed-block renderer.
#![deny(missing_docs)]
#![deny(unsafe_code)]

mod block;
mod paths;
mod source;

/// The opening delimiter of a managed instruction block.
pub const BEGIN_MARKER: &[u8] = b"<!-- symbrain:begin -->";
/// The closing delimiter of a managed instruction block.
pub const END_MARKER: &[u8] = b"<!-- symbrain:end -->";
/// The distinct prefix for escaped reserved tokens in managed content.
pub const ESCAPE_SENTINEL: &[u8] = b"<!-- symbrain:escape -->";
/// The on-disk spelling of a literal escape sentinel.
pub const ESCAPED_ESCAPE_SENTINEL: &[u8] = b"<!-- symbrain:escape -->e";
/// The on-disk spelling for a begin delimiter in managed content.
pub const ESCAPED_BEGIN_MARKER: &[u8] = b"<!-- symbrain:escape -->b";
/// The on-disk spelling for an end delimiter in managed content.
pub const ESCAPED_END_MARKER: &[u8] = b"<!-- symbrain:escape -->d";
/// The global instruction file name.
pub const GLOBAL_FILE_NAME: &str = "instructions.md";
/// The project-local instruction directory name.
pub const PROJECT_DIR_NAME: &str = ".symbrain";
/// The project-local instruction file name.
pub const PROJECT_FILE_NAME: &str = "instructions.md";
/// The application directory under an XDG config home.
pub const APP_NAME: &str = "symbrain";
/// The maximum size of one canonical source file.
pub const MAX_SOURCE_FILE_BYTES: u64 = 1 << 20;
/// The maximum size of the merged global and project sources.
pub const MAX_SOURCE_TOTAL_BYTES: u64 = 3 << 19;

pub use block::render;
pub use paths::open_trusted_root;
pub use source::{Source, resolve_global_path};
