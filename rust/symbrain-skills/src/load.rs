//! Capability-rooted filesystem loading for skill bundles.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ambient_authority::ambient_authority;
#[cfg(unix)]
use cap_std::fs::OpenOptionsExt;
use cap_std::fs::{Dir, OpenOptions};

use crate::model::{
    Bundle, MAX_INPUT_SIZE, MAX_RESOURCE_DEPTH, MAX_RESOURCE_ENTRIES, MAX_TOTAL_RESOURCE_BYTES,
    Resource, SkillError, parse_manifest, parse_skill_md,
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

    let skill_bytes = read_control(&root_cap, root, "SKILL.md")?;
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
    let mut markdown = std::collections::BTreeMap::new();
    for resource in &resources {
        if !is_overlay_path(&resource.path) && is_markdown(&resource.path) {
            let bytes = read_limited(
                &root_cap,
                Path::new(&resource.path),
                &format!("resource {}", resource.path),
                MAX_INPUT_SIZE,
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
    let block_overrides = load_overrides(&root_cap, root)?;
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

fn open_read(root: &Dir, relative: &Path, name: &str) -> Result<cap_std::fs::File, SkillError> {
    root.open_with(relative, &open_options())
        .map_err(|error| SkillError(format!("read {name}: {error}")))
}

fn read_control(root: &Dir, _anchor: &Path, name: &str) -> Result<Vec<u8>, SkillError> {
    let symlink = root
        .symlink_metadata(name)
        .is_ok_and(|metadata| metadata.file_type().is_symlink());
    read_limited(
        root,
        Path::new(name),
        &format!("read {name}"),
        MAX_INPUT_SIZE,
    )
    .map_err(|error| {
        if symlink {
            SkillError(format!(
                "{name} escapes skill root or is not a regular file"
            ))
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
    let file = open_read(root, relative, name)?;
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
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| SkillError(format!("read {name}: {error}")))?;
    if bytes.len() as u64 > limit {
        return Err(SkillError(format!(
            "{name} exceeds maximum input size of {limit} bytes"
        )));
    }
    Ok(bytes)
}

fn read_dir(
    root: &Dir,
    relative: &Path,
    name: &str,
) -> Result<Vec<cap_std::fs::DirEntry>, SkillError> {
    root.read_dir(relative)
        .map_err(|error| SkillError(format!("read {name}: {error}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| SkillError(format!("read {name} entry: {error}")))
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
        let mut entries = read_dir(root, directory, "bundle directory")?;
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
                continue;
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
    #[cfg(not(unix))]
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
