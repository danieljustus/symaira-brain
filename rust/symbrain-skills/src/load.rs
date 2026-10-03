//! Capability-rooted filesystem loading for skill bundles.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ambient_authority::ambient_authority;
#[cfg(unix)]
use cap_std::fs::OpenOptionsExt;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
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
/// no canonicalized pathname is reopened after validation.
///
/// # Errors
///
/// Returns [`SkillError`] when the root or a bundle input cannot be opened
/// safely, exceeds a configured limit, or fails parsing or validation.
pub fn load_bundle(root: &Path) -> Result<Bundle, SkillError> {
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

    let skill_bytes = read_skill_document(&root_cap, Path::new("SKILL.md"), "SKILL.md", None)?;
    let parsed = parse_skill_md(&skill_bytes)?;
    let mut manifest =
        match root_cap.symlink_metadata("symskills.toml") {
            Ok(_) => {
                let bytes = read_control(&root_cap, root, "symskills.toml")?;
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

    let resources = load_resources(&root_cap, root)?;
    let mut retained_budget = ReadBudget::new(MAX_TOTAL_RESOURCE_BYTES, "resource tree");
    let mut markdown = std::collections::BTreeMap::new();
    for resource in &resources {
        if !is_overlay_path(&resource.path) && is_markdown(&resource.path) {
            let bytes = read_limited_expected(
                &root_cap,
                Path::new(&resource.path),
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
    let block_overrides = load_overrides(&root_cap, root, &resources, &mut retained_budget)?;
    let bundle = Bundle {
        root: logical_root,
        root_cap,
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

fn open_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits().cast_signed());
    options
}

fn open_read_with(
    root: &Dir,
    relative: &Path,
    name: &str,
    nofollow: bool,
) -> Result<cap_std::fs::File, SkillError> {
    let mut options = open_options();
    if nofollow {
        options.follow(FollowSymlinks::No);
    }
    root.open_with(relative, &options)
        .map_err(|error| SkillError(format!("read {name}: {}", go_io_error(&error))))
}

fn open_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits().cast_signed());
    options
}

fn go_io_error(error: &std::io::Error) -> String {
    let message = error.to_string();
    #[cfg(windows)]
    if let Some(context) = message.strip_suffix(": no such file or directory") {
        return format!("{context}: The system cannot find the file specified.");
    }
    message
}

pub(crate) struct ReadBudget {
    remaining: u64,
    limit: u64,
    label: &'static str,
}

impl ReadBudget {
    pub(crate) fn new(limit: u64, label: &'static str) -> Self {
        Self {
            remaining: limit,
            limit,
            label,
        }
    }

    fn error(&self) -> SkillError {
        SkillError(format!(
            "{} exceeds maximum total size of {} bytes",
            self.label, self.limit
        ))
    }
}

fn read_control(root: &Dir, _anchor: &Path, name: &str) -> Result<Vec<u8>, SkillError> {
    let symlink = root
        .symlink_metadata(name)
        .is_ok_and(|metadata| metadata.file_type().is_symlink());
    read_limited(root, Path::new(name), &format!("read {name}"), MAX_INPUT_SIZE).map_err(
        |error| {
            if symlink {
                SkillError(format!("{name} escapes skill root or is not a regular file"))
            } else {
                error
            }
        },
    )
}

pub(crate) fn read_skill_document(
    root: &Dir,
    relative: &Path,
    name: &str,
    budget: Option<&mut ReadBudget>,
) -> Result<Vec<u8>, SkillError> {
    let symlink = root
        .symlink_metadata(relative)
        .is_ok_and(|metadata| metadata.file_type().is_symlink());
    read_limited_inner(
        root,
        relative,
        name,
        MAX_INPUT_SIZE,
        None,
        budget,
        Some(crate::model::MAX_FRONTMATTER_SIZE),
        false,
    )
    .map_err(|error| {
        if symlink {
            SkillError(format!("{name} escapes skill root or is not a regular file"))
        } else {
            error
        }
    })
}

fn read_limited(
    root: &Dir,
    relative: &Path,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, SkillError> {
    read_limited_inner(root, relative, name, limit, None, None, None, false)
}

fn read_limited_expected(
    root: &Dir,
    relative: &Path,
    name: &str,
    limit: u64,
    expected_size: u64,
    budget: Option<&mut ReadBudget>,
) -> Result<Vec<u8>, SkillError> {
    read_limited_inner(
        root,
        relative,
        name,
        limit,
        Some(expected_size),
        budget,
        None,
        false,
    )
}

pub(crate) fn read_limited_nofollow(
    root: &Dir,
    relative: &Path,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, SkillError> {
    read_limited_inner(root, relative, name, limit, None, None, None, true)
}

#[allow(clippy::too_many_arguments)]
fn read_limited_inner(
    root: &Dir,
    relative: &Path,
    name: &str,
    limit: u64,
    expected_size: Option<u64>,
    mut budget: Option<&mut ReadBudget>,
    frontmatter_limit: Option<usize>,
    nofollow: bool,
) -> Result<Vec<u8>, SkillError> {
    let mut file = open_read_with(root, relative, name, nofollow)?;
    let metadata = file
        .metadata()
        .map_err(|error| SkillError(format!("read {name}: {error}")))?;
    if !metadata.is_file() {
        return Err(SkillError(format!("{name} must be a regular file")));
    }
    if metadata.len() > limit {
        return Err(SkillError(format!(
            "{name} exceeds maximum input size of {limit} bytes"
        )));
    }
    if expected_size.is_some_and(|expected| metadata.len() > expected) {
        return Err(SkillError(format!("resource {name} changed since inventory")));
    }
    let budget_remaining = budget.as_ref().map(|budget| budget.remaining);
    if budget_remaining.is_some_and(|remaining| metadata.len() > remaining) {
        return Err(budget.as_deref().expect("budget exists").error());
    }
    let read_limit = expected_size.map_or(limit, |expected| limit.min(expected));
    let read_limit = budget_remaining.map_or(read_limit, |remaining| read_limit.min(remaining));
    let scan_capacity = frontmatter_limit.map(|size| size.saturating_mul(2).saturating_add(16));
    let initial_capacity = scan_capacity.map_or(read_limit, |scan| read_limit.min(scan as u64));
    let capacity = usize::try_from(initial_capacity)
        .map_err(|_| SkillError(format!("{name} input size cannot be represented")))?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut scan_frontmatter = frontmatter_limit.is_some();
    let mut chunk = [0_u8; 8192];

    loop {
        let remaining = read_limit.saturating_sub(bytes.len() as u64);
        if remaining == 0 {
            let mut extra = [0_u8; 1];
            match file.read(&mut extra) {
                Ok(0) => break,
                Ok(_) => {
                    if let Some(expected) = expected_size.filter(|expected| *expected <= read_limit) {
                        return Err(SkillError(format!(
                            "resource {name} changed since inventory (expected {expected} bytes)"
                        )));
                    }
                    if let Some(remaining) = budget_remaining.filter(|left| *left <= read_limit) {
                        if remaining < limit {
                            return Err(budget.as_deref().expect("budget exists").error());
                        }
                    }
                    return Err(SkillError(format!(
                        "{name} exceeds maximum input size of {limit} bytes"
                    )));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(SkillError(format!("read {name}: {error}"))),
            }
        }
        let mut request = usize::try_from(remaining.min(chunk.len() as u64))
            .map_err(|_| SkillError(format!("{name} input size cannot be represented")))?;
        if scan_frontmatter {
            let remaining_scan = scan_capacity
                .expect("frontmatter scan has a bounded capacity")
                .saturating_sub(bytes.len());
            if remaining_scan == 0 {
                return Err(SkillError(format!(
                    "SKILL.md frontmatter exceeds maximum size of {} bytes",
                    crate::model::MAX_FRONTMATTER_SIZE
                )));
            }
            request = request.min(remaining_scan);
        }
        let read = match file.read(&mut chunk[..request]) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(SkillError(format!("read {name}: {error}"))),
        };
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if scan_frontmatter {
            scan_frontmatter = !frontmatter_scan(&bytes)?;
            if scan_frontmatter && bytes.len() >= scan_capacity.expect("scan capacity exists") {
                return Err(SkillError(format!(
                    "SKILL.md frontmatter exceeds maximum size of {} bytes",
                    crate::model::MAX_FRONTMATTER_SIZE
                )));
            }
        }
    }
    if let Some(budget) = budget {
        budget.remaining = budget.remaining.saturating_sub(bytes.len() as u64);
    }
    Ok(bytes)
}

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
        collected.push(
            entry.map_err(|error| SkillError(format!("read {name} entry: {error}")))?,
        );
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
    relative: &Path,
    name: &str,
) -> Result<Option<Vec<cap_std::fs::DirEntry>>, SkillError> {
    if !optional_entry_exists(root, relative, name)? {
        return Ok(None);
    }
    read_dir(
        root,
        relative,
        name,
        MAX_RESOURCE_ENTRIES,
        &format!("resource tree exceeds maximum entry count of {MAX_RESOURCE_ENTRIES}"),
    )
    .map(Some)
}

fn load_resources(root: &Dir, _anchor: &Path) -> Result<Vec<Resource>, SkillError> {
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
        let mut entries = read_dir(
            root,
            directory,
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
            let metadata = match root.metadata(&relative) {
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
