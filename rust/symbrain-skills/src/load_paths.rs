// Resource links are interpreted inside the retained root, never reopened via
// their ambient absolute pathname. Cap-std remains the authority for every stat,
// directory walk and final file open, including concurrent replacements.
fn resource_path(root: &Dir, anchors: &[&Path], relative: &Path) -> Result<PathBuf, SkillError> {
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
        resolved.pop();
        let target = if target.is_absolute() {
            let confined = anchors
                .iter()
                .find_map(|anchor| target.strip_prefix(anchor).ok())
                .ok_or_else(escape)?;
            resolved.clear();
            confined
        } else {
            target.as_path()
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
