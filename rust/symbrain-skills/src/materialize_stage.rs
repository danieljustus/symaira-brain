fn validate_component(value: &str, label: &str) -> Result<(), SkillError> {
    if value.is_empty() || value.len() > 255 || value == "." || value == ".." {
        return Err(SkillError(format!("{label} is not a safe path component")));
    }
    let mut components = Path::new(value).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(SkillError(format!("{label} is not a safe path component")));
    }
    Ok(())
}

fn materialize_stage(
    bundle: &Bundle,
    rendered: &Rendered,
    root: &Dir,
    stage: &Path,
    fault_operation: Option<&str>,
) -> Result<(), SkillError> {
    let mut resources = bundle.resources.clone();
    resources.sort_by(|left, right| left.path.cmp(&right.path));
    for resource in resources {
        if resource.path == "SKILL.md"
            || resource.path == ".symskills.json"
            || resource.path == "symskills.toml"
            || resource.path == "overlays"
            || resource.path.starts_with("overlays/")
        {
            continue;
        }
        if resource.size > MAX_RESOURCE_SIZE {
            return Err(SkillError(format!(
                "resource {} exceeds maximum size of {} bytes",
                resource.path, MAX_RESOURCE_SIZE
            )));
        }
        let bytes = if let Some(bytes) = rendered.files.get(&resource.path) {
            bytes.clone()
        } else {
            read_bundle_bytes(
                bundle,
                Path::new(&resource.path),
                &resource.path,
                MAX_RESOURCE_SIZE,
            )?
        };
        let mode = parse_mode(&resource.mode);
        write_file(
            root,
            &stage.join(&resource.path),
            &bytes,
            mode,
            fault_operation,
        )
        .map_err(|error| io_error(&error))?;
    }
    for path in rendered.files.keys() {
        if !bundle
            .resources
            .iter()
            .any(|resource| resource.path == *path)
        {
            return Err(SkillError(format!(
                "rendered resource {path:?} is not in the source bundle"
            )));
        }
    }
    write_file(
        root,
        &stage.join("SKILL.md"),
        &rendered.skill_md,
        0o644,
        fault_operation,
    )
    .map_err(|error| SkillError(format!("write SKILL.md: {error}")))?;
    if let Some((path, bytes)) = crate::target::generated_metadata(
        &rendered.target,
        &rendered.name,
        &rendered.frontmatter.description,
    ) {
        write_file(
            root,
            &stage.join(path),
            &bytes,
            0o644,
            fault_operation,
        )
        .map_err(|error| SkillError(format!("write {path}: {error}")))?;
    }
    Ok(())
}

fn source_hash(bundle: &Bundle, rendered: &Rendered) -> Result<String, SkillError> {
    let tree_hash = source_tree_hash(bundle)?;
    let mut hasher = Sha256::new();
    hasher.update(tree_hash.as_bytes());
    hasher.update([0]);
    hasher.update(&rendered.skill_md);
    hasher.update([0]);
    hasher.update(rendered.target.as_bytes());
    if let Some(files_hash) = variant_files_hash(&rendered.files) {
        hasher.update([0]);
        hasher.update(files_hash.as_bytes());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn source_tree_hash(bundle: &Bundle) -> Result<String, SkillError> {
    let mut resources = bundle.resources.clone();
    resources.sort_by(|left, right| left.path.cmp(&right.path));
    let mut hasher = Sha256::new();
    for resource in resources {
        if resource.path == ".symskills.json"
            || resource.path == "symskills.toml"
            || resource.path == "overlays"
            || resource.path.starts_with("overlays/")
        {
            continue;
        }
        let bytes = read_bundle_bytes(
            bundle,
            Path::new(&resource.path),
            &resource.path,
            MAX_RESOURCE_SIZE,
        )?;
        hasher.update(resource.path.as_bytes());
        hasher.update([0]);
        hasher.update(resource.mode.as_bytes());
        hasher.update([0]);
        hasher.update(&bytes);
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn variant_files_hash(files: &BTreeMap<String, Vec<u8>>) -> Option<String> {
    if files.is_empty() {
        return None;
    }
    let mut hasher = Sha256::new();
    for (path, bytes) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(bytes);
        hasher.update([0]);
    }
    Some(format!("{:x}", hasher.finalize()))
}
