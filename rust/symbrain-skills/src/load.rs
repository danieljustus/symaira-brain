//! Capability-rooted filesystem loading for skill bundles.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ambient_authority::ambient_authority;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
#[cfg(unix)]
use cap_std::fs::OpenOptionsExt;
use cap_std::fs::{Dir, OpenOptions};

use crate::model::{
    Bundle, MAX_INPUT_SIZE, MAX_RESOURCE_DEPTH, MAX_RESOURCE_ENTRIES, MAX_TOTAL_RESOURCE_BYTES,
    Resource, SkillError, frontmatter_scan, parse_manifest, parse_skill_md,
};
use crate::variant;

/// Reads all bundle inputs through one capability rooted at the trusted root.
///
/// The root is opened once and retained by the bundle. Every file read is then
/// resolved by the capability API and consumed from that already-open handle;
/// no resource is reopened through an ambient pathname after validation.
///
/// # Errors
///
/// Returns [`SkillError`] when the root or a bundle input cannot be opened
/// safely, exceeds a configured limit, or fails parsing or validation.
pub fn load_bundle(root: &Path) -> Result<Bundle, SkillError> {
    BundleLoader::default().load(root)
}

/// One actual-read budget shared by an operation's bundles and their later reads.
#[derive(Debug, Clone)]
pub struct BundleLoader {
    budget: Arc<Mutex<ReadBudget>>,
}

impl Default for BundleLoader {
    fn default() -> Self {
        Self {
            budget: Arc::new(Mutex::new(ReadBudget::new(
                MAX_TOTAL_RESOURCE_BYTES,
                "skill operation inputs",
            ))),
        }
    }
}

impl BundleLoader {
    /// Loads a bundle without resetting this operation's actual-read budget.
    ///
    /// # Errors
    ///
    /// Returns a loader error or rejects inputs that exceed the shared budget.
    pub fn load(&self, root: &Path) -> Result<Bundle, SkillError> {
        load_bundle_with_budget(root, Arc::clone(&self.budget))
    }
}

fn load_bundle_with_budget(
    root: &Path,
    read_budget: Arc<Mutex<ReadBudget>>,
) -> Result<Bundle, SkillError> {
    let logical_root = std::path::absolute(root)
        .map_err(|error| SkillError(format!("resolve skill root: {error}")))?;
    let root_cap = Arc::new(
        Dir::open_ambient_dir(root, ambient_authority())
            .map_err(|error| SkillError(format!("open skill root: {error}")))?,
    );
    if !root_cap
        .dir_metadata()
        .map_err(|error| SkillError(format!("stat skill root: {error}")))?
        .is_dir()
    {
        return Err(SkillError("skill root is not a directory".into()));
    }

    let root_spellings = trusted_root_spellings(&logical_root)?;
    let anchors = root_spellings.as_slice();

    let mut retained_budget = read_budget
        .lock()
        .map_err(|_| SkillError("skill input budget lock poisoned".into()))?;
    let skill_bytes = read_skill_document(
        &root_cap,
        Path::new("SKILL.md"),
        "SKILL.md",
        Some(&mut retained_budget),
    )?;
    let parsed = parse_skill_md(&skill_bytes)?;
    let mut manifest =
        match root_cap.symlink_metadata("symskills.toml") {
            Ok(_) => {
                let bytes = read_control(&root_cap, "symskills.toml", &mut retained_budget)?;
                parse_manifest(&String::from_utf8(bytes).map_err(|error| {
                    SkillError(format!("read symskills.toml as UTF-8: {error}"))
                })?)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => crate::model::Manifest {
                targets: std::collections::BTreeMap::new(),
                ..Default::default()
            },
            Err(error) => {
                return Err(SkillError(format!("stat symskills.toml: {error}")));
            }
        };
    if manifest.skill.name.is_empty() {
        manifest.skill.name.clone_from(&parsed.frontmatter.name);
    }
    if manifest.skill.version.is_empty() {
        manifest
            .skill
            .version
            .clone_from(&parsed.frontmatter.version);
    }

    let resources = load_resources(&root_cap, anchors)?;
    let mut markdown = std::collections::BTreeMap::new();
    for resource in &resources {
        if !is_overlay_path(&resource.path) && is_markdown(&resource.path) {
            let resolved = resource_path(&root_cap, anchors, Path::new(&resource.path))?;
            let bytes = read_limited_expected(
                &root_cap,
                &resolved,
                &format!("resource {}", resource.path),
                MAX_INPUT_SIZE,
                resource.size,
                Some(&mut retained_budget),
            )?;
            if contains_variant_syntax(&bytes) && std::str::from_utf8(&bytes).is_err() {
                return Err(SkillError(format!(
                    "invalid_utf8_variant_markdown: resource {}",
                    resource.path
                )));
            }
            markdown.insert(resource.path.clone(), bytes);
        }
    }
    let block_overrides = load_overrides(&root_cap, anchors, &resources, &mut retained_budget)?;
    drop(retained_budget);
    let bundle = Bundle {
        root: logical_root,
        root_cap,
        root_spellings,
        read_budget,
        frontmatter: parsed.frontmatter,
        manifest,
        body: parsed.body,
        resources,
        markdown,
        block_overrides,
        body_line_offset: parsed.body_line_offset,
    };
    Ok(bundle)
}

fn contains_variant_syntax(bytes: &[u8]) -> bool {
    bytes.windows(7).any(|window| window == b"{{term:")
        || bytes.windows(14).any(|window| window == b"symskills:")
}

fn is_markdown(path: &str) -> bool {
    let path = Path::new(path);
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
    })
}
fn is_overlay_path(path: &str) -> bool {
    path == "overlays" || path.starts_with("overlays/")
}
fn slash(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

include!("load_read.rs");
include!("load_paths.rs");

fn collect_bounded_entries<T>(
    entries: impl IntoIterator<Item = std::io::Result<T>>,
    limit: usize,
    name: &str,
    overflow_message: &str,
) -> Result<Vec<T>, SkillError> {
    let mut collected = Vec::new();
    for entry in entries {
        if collected.len() >= limit {
            return Err(SkillError(overflow_message.to_owned()));
        }
        collected.push(entry.map_err(|error| SkillError(format!("read {name} entry: {error}")))?);
    }
    Ok(collected)
}

fn read_dir(
    root: &Dir,
    relative: &Path,
    name: &str,
    limit: usize,
    overflow_message: &str,
) -> Result<Vec<cap_std::fs::DirEntry>, SkillError> {
    let entries = root
        .read_dir(relative)
        .map_err(|error| SkillError(format!("read {name}: {error}")))?;
    collect_bounded_entries(entries, limit, name, overflow_message)
}

fn optional_entry_exists(root: &Dir, relative: &Path, name: &str) -> Result<bool, SkillError> {
    // Check the entry itself: a dangling link must not masquerade as an absent tree.
    match root.symlink_metadata(relative) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(SkillError(format!("stat {name}: {error}"))),
    }
}
fn read_optional_dir(
    root: &Dir,
    anchors: &[PathBuf],
    relative: &Path,
    name: &str,
) -> Result<Option<Vec<cap_std::fs::DirEntry>>, SkillError> {
    let resolved = resource_path(root, anchors, relative)?;
    if !optional_entry_exists(root, &resolved, name)? {
        return Ok(None);
    }
    read_dir(
        root,
        &resolved,
        name,
        MAX_RESOURCE_ENTRIES,
        &format!("resource tree exceeds maximum entry count of {MAX_RESOURCE_ENTRIES}"),
    )
    .map(Some)
}

fn load_resources(root: &Dir, anchors: &[PathBuf]) -> Result<Vec<Resource>, SkillError> {
    let mut resources = Vec::new();
    let mut total_bytes = 0_u64;
    let mut entries_seen = 0;
    let mut pending = vec![(PathBuf::new(), 0_usize)];
    while let Some((current, depth)) = pending.pop() {
        if depth > MAX_RESOURCE_DEPTH {
            return Err(SkillError(format!(
                "resource tree exceeds maximum depth of {MAX_RESOURCE_DEPTH}"
            )));
        }
        let directory = if current.as_os_str().is_empty() {
            Path::new(".")
        } else {
            current.as_path()
        };
        let overflow =
            format!("resource tree exceeds maximum entry count of {MAX_RESOURCE_ENTRIES}");
        let remaining_entries = MAX_RESOURCE_ENTRIES.saturating_sub(entries_seen);
        let resolved_directory = resource_path(root, anchors, directory)?;
        let mut entries = read_dir(
            root,
            &resolved_directory,
            "bundle directory",
            remaining_entries,
            &overflow,
        )?;
        entries.sort_by_key(cap_std::fs::DirEntry::file_name);
        for entry in entries.into_iter().rev() {
            entries_seen += 1;
            if entries_seen > MAX_RESOURCE_ENTRIES {
                return Err(SkillError(format!(
                    "resource tree exceeds maximum entry count of {MAX_RESOURCE_ENTRIES}"
                )));
            }
            let name = entry.file_name();
            let relative = current.join(&name);
            let file_type = entry
                .file_type()
                .map_err(|error| SkillError(format!("stat bundle entry: {error}")))?;
            if file_type.is_dir() {
                if name != ".git" {
                    pending.push((relative, depth + 1));
                }
                continue;
            }
            if relative == Path::new("SKILL.md") {
                continue;
            }
            let resolved = resource_path(root, anchors, &relative)?;
            let metadata = match root.metadata(&resolved) {
                Ok(metadata) => metadata,
                Err(error) if file_type.is_symlink() => {
                    return Err(SkillError(format!(
                        "resource {} escapes skill root: {error}",
                        slash(&relative)
                    )));
                }
                Err(error) => {
                    return Err(SkillError(format!(
                        "stat resource {}: {error}",
                        slash(&relative)
                    )));
                }
            };
            if metadata.is_dir() {
                if name != ".git" {
                    pending.push((relative, depth + 1));
                }
                continue;
            }
            if !metadata.is_file() {
                return Err(SkillError(format!(
                    "resource {} must be a regular file",
                    slash(&relative)
                )));
            }
            append_resource(&mut resources, &mut total_bytes, &relative, &metadata)?;
        }
    }
    resources.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(resources)
}

fn resource_mode(metadata: &cap_std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        cap_std::fs::PermissionsExt::mode(&metadata.permissions()) & 0o777
    }
    #[cfg(windows)]
    {
        let _ = metadata;
        0o666
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
        0o644
    }
}

fn append_resource(
    resources: &mut Vec<Resource>,
    total_bytes: &mut u64,
    relative: &Path,
    metadata: &cap_std::fs::Metadata,
) -> Result<(), SkillError> {
    if metadata.len() > crate::model::MAX_RESOURCE_SIZE {
        return Err(SkillError(format!(
            "resource {} exceeds maximum size of {} bytes (actual: {})",
            slash(relative),
            crate::model::MAX_RESOURCE_SIZE,
            metadata.len()
        )));
    }
    *total_bytes = total_bytes
        .checked_add(metadata.len())
        .ok_or_else(|| SkillError("resource byte count overflow".into()))?;
    if *total_bytes > MAX_TOTAL_RESOURCE_BYTES {
        return Err(SkillError(format!(
            "resource tree exceeds maximum total size of {MAX_TOTAL_RESOURCE_BYTES} bytes"
        )));
    }
    let mode = resource_mode(metadata);
    resources.push(Resource {
        path: slash(relative),
        size: metadata.len(),
        mode: format!("{mode:04o}"),
        executable: mode & 0o111 != 0,
    });
    Ok(())
}

include!("load_helpers.rs");

#[cfg(test)]
#[path = "load_tests.rs"]
mod tests;
