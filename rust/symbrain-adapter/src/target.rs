use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use symbrain_harness::{HarnessError, HarnessName, InstructionAdapter, all, lookup};
use symbrain_instructions::render as render_block;

use super::atomic::AtomicFile;
use super::path::validate_relative_target_path;

/// The fixed pointer prelude used by a fresh Claude instruction file.
pub const CLAUDE_POINTER: &[u8] = b"<!-- symbrain:managed-pointer -->\nThis file is managed by `symbrain sync`. Canonical instructions live in AGENTS.md.\nEdit AGENTS.md for global changes; use the managed block below for project-specific additions.\n\n";

/// The fixed YAML frontmatter used by a fresh Cursor rule file.
pub const CURSOR_HEADER: &[u8] = b"---\ndescription: Symaira brain managed instructions\nglobs: **/*\nalwaysApply: true\n---\n\n";

/// An error returned while selecting or rendering an instruction adapter.
#[derive(Debug)]
pub enum AdapterError {
    /// The harness registry rejected the requested name.
    Harness(HarnessError),
    /// Filesystem access or durability synchronization failed.
    Io(io::Error),
    /// A target path was absolute, empty, or escaped its project root.
    InvalidTargetPath(String),
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Harness(error) => error.fmt(f),
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::InvalidTargetPath(path) => {
                write!(f, "invalid relative adapter target path {path:?}")
            }
        }
    }
}

impl std::error::Error for AdapterError {}

impl From<HarnessError> for AdapterError {
    fn from(error: HarnessError) -> Self {
        Self::Harness(error)
    }
}

impl From<io::Error> for AdapterError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// One fixed instruction target selected by a registered harness capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    harness: HarnessName,
    capability: InstructionAdapter,
    relative_path: &'static str,
}

impl Target {
    /// Returns the harness that owns this target.
    #[must_use]
    pub const fn harness(self) -> HarnessName {
        self.harness
    }

    /// Returns the registry capability that selected this target.
    #[must_use]
    pub const fn capability(self) -> InstructionAdapter {
        self.capability
    }

    /// Returns the fixed project-relative output path.
    #[must_use]
    pub const fn relative_path(self) -> &'static str {
        self.relative_path
    }

    /// Resolves the output path under a project directory after validation.
    ///
    /// # Errors
    /// Returns [`AdapterError::InvalidTargetPath`] if the fixed path is not a
    /// safe relative file path.
    pub fn path(self, project_dir: &Path) -> Result<PathBuf, AdapterError> {
        validate_relative_target_path(Path::new(self.relative_path))?;
        Ok(project_dir.join(self.relative_path))
    }

    /// Renders this target while preserving bytes outside its managed block.
    ///
    /// Fresh Claude and Cursor files receive their fixed prelude. Existing
    /// files are passed directly to the byte-preserving instruction renderer,
    /// so arbitrary user bytes, CRLF sequences, and invalid UTF-8 survive.
    ///
    /// # Errors
    /// Returns [`AdapterError::InvalidTargetPath`] if the target path is unsafe.
    pub fn render(
        self,
        existing: &[u8],
        canonical_content: &[u8],
        project_dir: &Path,
    ) -> Result<Rendered, AdapterError> {
        let path = self.path(project_dir)?;
        let output = match self.capability {
            InstructionAdapter::Claude if existing.is_empty() => {
                let mut output =
                    Vec::with_capacity(CLAUDE_POINTER.len() + canonical_content.len() + 64);
                output.extend_from_slice(CLAUDE_POINTER);
                output.extend_from_slice(&render_block(&[], canonical_content));
                output.push(b'\n');
                output
            }
            InstructionAdapter::Cursor if existing.is_empty() => {
                let mut output =
                    Vec::with_capacity(CURSOR_HEADER.len() + canonical_content.len() + 64);
                output.extend_from_slice(CURSOR_HEADER);
                output.extend_from_slice(&render_block(&[], canonical_content));
                output
            }
            _ => render_block(existing, canonical_content),
        };
        Ok(Rendered {
            harness: self.harness,
            relative_path: self.relative_path,
            path,
            output,
        })
    }
}

/// The result of rendering one adapter target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// Registered harness owning the output.
    pub harness: HarnessName,
    /// Fixed project-relative target path.
    pub relative_path: &'static str,
    /// Resolved output path under the supplied project directory.
    pub path: PathBuf,
    /// Exact bytes to write.
    pub output: Vec<u8>,
}
/// Renders and atomically writes this target.
///
/// # Errors
/// Returns a rendering or filesystem error.
impl Target {
    /// Renders and atomically writes this target.
    ///
    /// # Errors
    /// Returns a rendering or filesystem error.
    pub fn write(
        self,
        existing: &[u8],
        canonical_content: &[u8],
        project_dir: &Path,
    ) -> Result<Rendered, AdapterError> {
        let atomic = AtomicFile::open(project_dir, Path::new(self.relative_path), true)
            .map_err(AdapterError::Io)?;
        let actual = match atomic.read_bounded() {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => existing.to_vec(),
            Err(error) => return Err(AdapterError::Io(error)),
        };
        let rendered = self.render(&actual, canonical_content, project_dir)?;
        if rendered.output != actual {
            atomic.write(&rendered.output).map_err(AdapterError::Io)?;
        }
        Ok(rendered)
    }
}

/// Returns the adapter target selected by a harness name.
///
/// Harness selection is delegated to the nine-entry `symbrain-harness`
/// registry. A successful `None` means the registered harness intentionally
/// has no instruction adapter (five current harnesses have this result).
///
/// # Errors
/// Returns the registry's unknown-harness error for an unregistered name.
pub fn target_for_harness(name: &str) -> Result<Option<Target>, AdapterError> {
    let harness = lookup(name)?;
    Ok(target_for_registered_harness(
        harness.name,
        harness.instruction_adapter,
    ))
}

/// Alias for [`target_for_harness`] using adapter terminology.
///
/// # Errors
/// Returns the same registry-selection errors as [`target_for_harness`].
pub fn adapter_for_harness(name: &str) -> Result<Option<Target>, AdapterError> {
    target_for_harness(name)
}

/// Returns all four adapter targets in canonical harness registry order.
///
/// The list is derived from [`symbrain_harness::all`], not from a second
/// harness-name table.
#[must_use]
pub fn all_targets() -> Vec<Target> {
    all()
        .iter()
        .filter_map(|harness| {
            target_for_registered_harness(harness.name, harness.instruction_adapter)
        })
        .collect()
}

/// Alias for [`all_targets`].
#[must_use]
pub fn adapters() -> Vec<Target> {
    all_targets()
}

/// Renders a previously selected target.
///
/// # Errors
/// Returns an invalid target path error.
pub fn render(
    target: Target,
    existing: &[u8],
    canonical_content: &[u8],
    project_dir: &Path,
) -> Result<Rendered, AdapterError> {
    target.render(existing, canonical_content, project_dir)
}

/// Renders a target selected by harness name.
///
/// # Errors
/// Returns an unknown-harness error, or an invalid target path error.
pub fn render_for_harness(
    name: &str,
    existing: &[u8],
    canonical_content: &[u8],
    project_dir: &Path,
) -> Result<Option<Rendered>, AdapterError> {
    let Some(target) = target_for_harness(name)? else {
        return Ok(None);
    };
    target
        .render(existing, canonical_content, project_dir)
        .map(Some)
}

fn target_for_registered_harness(
    harness: HarnessName,
    capability: InstructionAdapter,
) -> Option<Target> {
    let relative_path = match capability {
        InstructionAdapter::Agents => "AGENTS.md",
        InstructionAdapter::Claude => "CLAUDE.md",
        InstructionAdapter::Cursor => ".cursor/rules/symbrain.mdc",
        InstructionAdapter::Antigravity => "GEMINI.md",
        InstructionAdapter::None => return None,
    };
    Some(Target {
        harness,
        capability,
        relative_path,
    })
}
