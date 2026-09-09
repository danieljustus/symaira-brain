pub(crate) fn read_bundle_bytes(
    root: &Bundle,
    relative: &Path,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, SkillError> {
    read_limited(&root.root_cap, relative, name, limit)
}

pub(crate) fn read_bundle_text(
    root: &Bundle,
    relative: &Path,
    name: &str,
    limit: u64,
) -> Result<String, SkillError> {
    let bytes = read_bundle_bytes(root, relative, name, limit)?;
    String::from_utf8(bytes).map_err(|_| SkillError(format!("invalid_utf8_overlay: {name}")))
}

fn is_not_found(error: &SkillError) -> bool {
    error.0.contains("No such file or directory") || error.0.contains("not found")
}

fn load_overrides(
    root: &Dir,
    _anchor: &Path,
) -> Result<
    std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    SkillError,
> {
    let mut targets = match read_dir(root, Path::new("overlays"), "overlays") {
        Ok(entries) => entries,
        Err(error) if is_not_found(&error) => {
            return Ok(std::collections::BTreeMap::new());
        }
        Err(error) => return Err(error),
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
        let mut files = match read_dir(root, &blocks, "overlay blocks") {
            Ok(entries) => entries,
            Err(error) if is_not_found(&error) => continue,
            Err(error) => return Err(error),
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
            let bytes = read_limited(root, &relative, &slash(&relative), MAX_INPUT_SIZE)?;
            let metadata = open_read(root, &relative, &slash(&relative))?
                .metadata()
                .map_err(|error| SkillError(format!("stat overlay block: {error}")))?;
            if !metadata.is_file() {
                continue;
            }
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
