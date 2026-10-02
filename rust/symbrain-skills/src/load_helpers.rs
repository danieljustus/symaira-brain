pub(crate) fn read_bundle_bytes(
    root: &Bundle,
    relative: &Path,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, SkillError> {
    let resource = root
        .resources
        .iter()
        .find(|resource| Path::new(&resource.path) == relative)
        .ok_or_else(|| SkillError(format!("resource {name} changed since inventory")))?;
    let resolved = resource_path(
        &root.root_cap,
        &[root.canonical_root.as_path(), root.root.as_path()],
        relative,
    )?;
    let mut budget = root
        .read_budget
        .lock()
        .map_err(|_| SkillError("skill input budget lock poisoned".into()))?;
    read_limited_expected(
        &root.root_cap,
        &resolved,
        name,
        limit,
        resource.size,
        Some(&mut budget),
    )
}

pub(crate) fn read_bundle_optional_bytes(
    root: &Bundle,
    relative: &Path,
    name: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>, SkillError> {
    let resolved = resource_path(
        &root.root_cap,
        &[root.canonical_root.as_path(), root.root.as_path()],
        relative,
    )?;
    if !optional_entry_exists(&root.root_cap, &resolved, name)? {
        return Ok(None);
    }
    read_bundle_bytes(root, relative, name, limit).map(Some)
}

fn load_overrides(
    root: &Dir,
    anchors: &[&Path],
    resources: &[Resource],
    budget: &mut ReadBudget,
) -> Result<
    std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    SkillError,
> {
    let Some(mut targets) = read_optional_dir(root, anchors, Path::new("overlays"), "overlays")?
    else {
        return Ok(std::collections::BTreeMap::new());
    };
    targets.sort_by_key(cap_std::fs::DirEntry::file_name);
    let mut result = std::collections::BTreeMap::new();
    for target in targets {
        let target_name = target.file_name().to_string_lossy().to_string();
        let target_type = target
            .file_type()
            .map_err(|error| SkillError(format!("stat overlay target: {error}")))?;
        if !target_type.is_dir() {
            continue;
        }
        let blocks = PathBuf::from("overlays")
            .join(&target_name)
            .join(variant::BLOCKS_DIR);
        let Some(mut files) = read_optional_dir(root, anchors, &blocks, "overlay blocks")? else {
            continue;
        };
        files.sort_by_key(cap_std::fs::DirEntry::file_name);
        let mut values = std::collections::BTreeMap::new();
        for file in files {
            let name = file.file_name().to_string_lossy().to_string();
            if !is_markdown(&name) {
                continue;
            }
            let relative = blocks.join(&name);
            let file_type = file
                .file_type()
                .map_err(|error| SkillError(format!("stat overlay block: {error}")))?;
            if file_type.is_dir() {
                continue;
            }
            let path = slash(&relative);
            let resource_size = resources
                .iter()
                .find(|resource| resource.path == path)
                .map(|resource| resource.size)
                .ok_or_else(|| SkillError(format!("resource {path} changed since inventory")))?;
            let resolved = resource_path(root, anchors, &relative)?;
            let bytes = read_limited_expected(
                root,
                &resolved,
                &path,
                MAX_INPUT_SIZE,
                resource_size,
                Some(budget),
            )?;
            let text = String::from_utf8(bytes)
                .map_err(|_| SkillError(format!("invalid_utf8_overlay: {}", relative.display())))?;
            let id = name
                .strip_suffix(".markdown")
                .or_else(|| name.strip_suffix(".md"))
                .unwrap_or(&name);
            values.insert(id.to_string(), text);
        }
        if !values.is_empty() {
            result.insert(target_name, values);
        }
    }
    Ok(result)
}
