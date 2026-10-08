// Comparison caches use Go's sourceFingerprint, not the installation tree hash.
// Included in materialize.rs so both paths share confinement, locks and rollback.
pub(crate) fn cached_comparison(
    bundle: &Bundle,
    rendered: &Rendered,
    cache: &Path,
) -> Result<Materialized, SkillError> {
    #[derive(Serialize)]
    struct Hint<'a> {
        fingerprint: &'a str,
        target: &'a str,
        name: &'a str,
    }
    let fingerprint = comparison_fingerprint(bundle)?;
    let mut root = PathBuf::new();
    for part in std::path::absolute(&bundle.root)
        .map_err(|error| SkillError(format!("comparison cache root: {error}")))?
        .components()
    {
        if part == Component::ParentDir {
            root.pop();
        } else {
            root.push(part.as_os_str());
        }
    }
    let mut key = Sha256::new();
    key.update(b"status-render-v1\0");
    key.update(root.as_os_str().as_encoded_bytes());
    key.update([0]);
    key.update(rendered.target.as_bytes());
    let relative = PathBuf::from("status-render").join(format!("{:x}", key.finalize()));
    let mut hash = Sha256::new();
    hash.update(fingerprint.as_bytes());
    hash.update([0]);
    hash.update(&rendered.skill_md);
    hash.update([0]);
    hash.update(rendered.target.as_bytes());
    if let Some((path, bytes)) = crate::target::generated_metadata(
        &rendered.target,
        &rendered.name,
        &rendered.frontmatter.description,
    ) {
        hash.update([0]);
        hash.update(path.as_bytes());
        hash.update([0]);
        hash.update(bytes);
    }
    if let Some(variants) = variant_files_hash(&rendered.files) {
        hash.update([0]);
        hash.update(variants.as_bytes());
    }
    let hash = format!("{:x}", hash.finalize());
    let hint = serde_json::to_string(&Hint {
        fingerprint: &fingerprint,
        target: &rendered.target,
        name: &rendered.name,
    })
    .map_err(|error| SkillError(format!("encode comparison cache hint: {error}")))?;
    let hint = format!("{}\n", comparison_json_escape(&hint));
    materialize_inner(
        bundle,
        rendered,
        cache,
        None,
        Some((&relative, &hash, hint.as_bytes())),
    )
}

fn comparison_fingerprint(bundle: &Bundle) -> Result<String, SkillError> {
    let mut paths = BTreeMap::new();
    paths.insert("SKILL.md", "");
    for resource in &bundle.resources {
        paths.insert(resource.path.as_str(), resource.mode.as_str());
    }
    let mut hash = Sha256::new();
    for (path, mode) in paths {
        let limit = if path == "SKILL.md" || path == "symskills.toml" {
            MAX_INPUT_SIZE
        } else {
            MAX_RESOURCE_SIZE
        };
        let bytes = if path == "SKILL.md" {
            let mut budget = bundle
                .read_budget
                .lock()
                .map_err(|_| SkillError("skill input budget lock poisoned".into()))?;
            crate::load::read_skill_document(
                &bundle.root_cap,
                Path::new(path),
                path,
                Some(&mut budget),
            )?
        } else {
            read_bundle_bytes(bundle, Path::new(path), path, limit)?
        };
        hash.update(path.as_bytes());
        hash.update([0]);
        hash.update(mode.as_bytes());
        hash.update([0]);
        hash.update(bytes);
        hash.update([0]);
    }
    // The native built-in target registry has no external metadata templates.
    Ok(format!("{:x}", hash.finalize()))
}

fn comparison_json_escape(json: &str) -> String {
    json.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn write_comparison_sidecar(root: &Dir, path: &Path, bytes: &[u8]) -> Result<(), SkillError> {
    match root.symlink_metadata(path) {
        Ok(metadata)
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || is_reparse_point(&metadata) =>
        {
            return Err(SkillError(
                "comparison cache sidecar is not a regular file".into(),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(&error)),
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stage =
        make_sibling_dir(root, parent, ".symskills-sidecar-").map_err(|error| io_error(&error))?;
    // Atomic replacement avoids truncating a caller-provided hard link.
    let staged = stage.join("hint.json");
    let result = (|| {
        write_file(root, &staged, bytes, 0o644, None)?;
        root.rename(&staged, root, path)?;
        sync_dir(root, parent, None)
    })();
    let cleanup = root.remove_dir_all(&stage);
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(io_error(&error)),
        (Err(error), Err(cleanup)) => Err(SkillError(format!(
            "comparison sidecar: {error}; cleanup: {cleanup}"
        ))),
    }
}
