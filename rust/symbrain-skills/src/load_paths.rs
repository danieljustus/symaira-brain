// Resource links are interpreted inside the retained root, never reopened via
// their ambient absolute pathname. Cap-std remains the authority for every stat,
// directory walk and final file open, including concurrent replacements.
fn resource_path(root: &Dir, anchors: &[PathBuf], relative: &Path) -> Result<PathBuf, SkillError> {
    use std::collections::VecDeque;
    use std::path::Component;

    let escape = || SkillError(format!("resource {} escapes skill root", slash(relative)));
    let mut pending: VecDeque<_> = relative
        .components()
        .map(|component| component.as_os_str().to_os_string())
        .collect();
    let mut resolved = PathBuf::new();
    let mut links = 0;
    while let Some(part) = pending.pop_front() {
        match Path::new(&part).components().next() {
            Some(Component::CurDir) | None => continue,
            Some(Component::ParentDir) => {
                if !resolved.pop() {
                    return Err(escape());
                }
                continue;
            }
            Some(Component::Normal(_)) => resolved.push(&part),
            Some(Component::RootDir | Component::Prefix(_)) => return Err(escape()),
        }
        if resolved.components().count() > MAX_RESOURCE_DEPTH + 1 {
            return Err(SkillError(format!(
                "resource {} exceeds maximum depth of {MAX_RESOURCE_DEPTH}",
                slash(relative)
            )));
        }
        let metadata = match root.symlink_metadata(&resolved) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(SkillError(format!(
                    "stat resource {}: {error}",
                    slash(relative)
                )));
            }
        };
        if !metadata.file_type().is_symlink() {
            continue;
        }
        links += 1;
        if links > MAX_RESOURCE_DEPTH {
            return Err(SkillError(format!(
                "resource {} exceeds maximum symlink resolution depth of {MAX_RESOURCE_DEPTH}",
                slash(relative)
            )));
        }
        let target = root.read_link_contents(&resolved).map_err(|error| {
            SkillError(format!("read resource link {}: {error}", slash(relative)))
        })?;
        let target = link_path_spelling(&target);
        resolved.pop();
        let target = if target.is_absolute() {
            let confined = anchors
                .iter()
                .find_map(|anchor| target.strip_prefix(link_path_spelling(anchor)).ok())
                .ok_or_else(escape)?;
            resolved.clear();
            confined
        } else {
            target.as_ref()
        };
        for component in target.components().rev() {
            pending.push_front(component.as_os_str().to_os_string());
        }
    }
    if resolved.as_os_str().is_empty() {
        resolved.push(".");
    }
    Ok(resolved)
}

// Capture only the explicitly trusted root's bounded link chain at bootstrap.
// Canonicalization can rewrite ancestor aliases, so retain its original link
// text too. Resource paths are never opened through any of these spellings.
fn trusted_root_spellings(root: &Path) -> Result<Vec<PathBuf>, SkillError> {
    let canonical_root = std::fs::canonicalize(root)
        .map_err(|error| SkillError(format!("resolve skill root spelling: {error}")))?;
    let mut spellings = vec![canonical_root, root.to_path_buf()];
    let mut spelling = root.to_path_buf();
    let mut links = 0;
    loop {
        // A trailing separator would make lstat follow the root link itself.
        spelling = spelling.components().collect();
        let metadata = std::fs::symlink_metadata(&spelling)
            .map_err(|error| SkillError(format!("stat trusted root spelling: {error}")))?;
        if !metadata.file_type().is_symlink() {
            return Ok(spellings);
        }
        if links == MAX_RESOURCE_DEPTH {
            return Err(SkillError(format!(
                "trusted root exceeds maximum symlink resolution depth of {MAX_RESOURCE_DEPTH}"
            )));
        }
        links += 1;
        let target = std::fs::read_link(&spelling)
            .map_err(|error| SkillError(format!("read trusted root link: {error}")))?;
        spelling = if target.is_absolute() {
            target
        } else {
            spelling
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(target)
        };
        spellings.push(spelling.clone());
    }
}

// Canonical Windows roots use extended drive/UNC prefixes, but link contents
// may use ordinary spellings. Compare only these equivalent volume prefixes;
// device namespaces stay distinct. This performs no I/O.
fn link_path_spelling(path: &Path) -> std::borrow::Cow<'_, Path> {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};

        let mut components = path.components();
        if let Some(Component::Prefix(prefix)) = components.next() {
            let mut spelling = match prefix.kind() {
                Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:", char::from(drive))),
                Prefix::VerbatimUNC(server, share) => {
                    let mut spelling = PathBuf::from(r"\\");
                    spelling.push(server);
                    spelling.push(share);
                    spelling
                }
                _ => return std::borrow::Cow::Borrowed(path),
            };
            spelling.extend(components);
            return std::borrow::Cow::Owned(spelling);
        }
    }
    std::borrow::Cow::Borrowed(path)
}

#[cfg(all(test, windows))]
mod path_spelling_tests {
    use super::link_path_spelling;
    use std::path::Path;

    #[test]
    fn only_extended_drive_and_unc_volume_prefixes_have_ordinary_aliases() {
        for (extended, ordinary) in [
            (r"\\?\C:\skill\real", r"C:\skill\real"),
            (
                r"\\?\UNC\server\share\skill\real",
                r"\\server\share\skill\real",
            ),
            (r"\\.\C:\skill\real", r"\\.\C:\skill\real"),
        ] {
            assert_eq!(
                link_path_spelling(Path::new(extended)).as_ref(),
                Path::new(ordinary)
            );
        }
        assert!(
            link_path_spelling(Path::new(r"\\?\C:\skill-other\real"))
                .strip_prefix(Path::new(r"C:\skill"))
                .is_err()
        );
    }
}
