fn marker_bytes(
    hash: &str,
    mut existing: Option<BTreeMap<String, serde_json::Value>>,
    manifest: &[OutputEntry],
    digest: &str,
) -> Result<Vec<u8>, SkillError> {
    let marker = existing.get_or_insert_with(BTreeMap::new);
    marker.insert(
        "source_hash".into(),
        serde_json::Value::String(hash.to_owned()),
    );
    marker.insert(
        "output_digest".into(),
        serde_json::Value::String(digest.to_owned()),
    );
    marker.insert(
        "output_manifest".into(),
        serde_json::Value::Null,
    );
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"{\n");
    let entries = marker.iter().collect::<Vec<_>>();
    for (index, (key, value)) in entries.iter().enumerate() {
        let key = serde_json::to_string(key)
            .map_err(|error| SkillError(format!("encode marker key: {error}")))?;
        let value = if key == "\"output_manifest\"" {
            serde_json::to_string_pretty(manifest)
                .map_err(|error| SkillError(format!("encode manifest: {error}")))?
        } else {
            serde_json::to_string_pretty(value)
                .map_err(|error| SkillError(format!("encode marker value: {error}")))?
        };
        bytes.extend_from_slice(format!("  {key}: ").as_bytes());
        for (line, content) in value.split('\n').enumerate() {
            if line > 0 {
                bytes.extend_from_slice(b"  ");
            }
            bytes.extend_from_slice(content.as_bytes());
            if line + 1 < value.lines().count() {
                bytes.push(b'\n');
            }
        }
        if index + 1 < entries.len() {
            bytes.push(b',');
        }
        bytes.push(b'\n');
    }
    bytes.push(b'}');
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_INPUT_SIZE {
        return Err(SkillError(format!(
            "render marker exceeds maximum input size of {MAX_INPUT_SIZE} bytes"
        )));
    }
    Ok(bytes)
}

fn read_marker(
    root: &Dir,
    path: &Path,
) -> Result<Option<BTreeMap<String, serde_json::Value>>, SkillError> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let Ok(file) = root.open_with(path, &options) else {
        return Ok(None);
    };
    let metadata = file
        .metadata()
        .map_err(|error| SkillError(format!("read marker metadata: {error}")))?;
    if !metadata.is_file() {
        return Ok(None);
    }
    if metadata.len() > MAX_INPUT_SIZE {
        return Err(SkillError(format!(
            "render marker exceeds maximum input size of {MAX_INPUT_SIZE} bytes"
        )));
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_SIZE.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| SkillError(format!("read marker: {error}")))?;
    if bytes.len() as u64 > MAX_INPUT_SIZE {
        return Err(SkillError(format!(
            "render marker exceeds maximum input size of {MAX_INPUT_SIZE} bytes"
        )));
    }
    Ok(serde_json::from_slice(&bytes).ok())
}

fn marker_matches(
    marker: Option<&BTreeMap<String, serde_json::Value>>,
    hash: &str,
    expected: &[OutputEntry],
    digest: &str,
) -> bool {
    let Some(marker) = marker else { return false };
    if marker
        .get("source_hash")
        .and_then(serde_json::Value::as_str)
        != Some(hash)
        || marker
            .get("output_digest")
            .and_then(serde_json::Value::as_str)
            != Some(digest)
    {
        return false;
    }
    marker
        .get("output_manifest")
        .and_then(|value| serde_json::from_value::<Vec<OutputEntry>>(value.clone()).ok())
        .is_some_and(|manifest| manifest == expected)
}

fn bound_output_with_marker(manifest: &[OutputEntry], marker: &[u8]) -> Result<(), SkillError> {
    let total = manifest
        .iter()
        .filter(|entry| entry.kind == "file")
        .try_fold(marker.len() as u64, |total, entry| {
            total
                .checked_add(entry.size)
                .ok_or_else(|| SkillError("rendered output byte count overflow".into()))
        })?;
    if total > MAX_OUTPUT_BYTES {
        return Err(SkillError(format!(
            "rendered output including marker exceeds maximum size of {MAX_OUTPUT_BYTES} bytes"
        )));
    }
    Ok(())
}

fn collect_manifest(root: &Dir, base: &Path) -> Result<Vec<OutputEntry>, SkillError> {
    let mut entries = Vec::new();
    let mut total = 0_u64;
    collect_manifest_inner(root, base, base, &mut entries, &mut total)
        .map_err(|error| io_error(&error))?;
    Ok(entries)
}

fn collect_manifest_inner(
    root: &Dir,
    current: &Path,
    base: &Path,
    entries: &mut Vec<OutputEntry>,
    total: &mut u64,
) -> io::Result<()> {
    let mut children = root.read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(cap_std::fs::DirEntry::file_name);
    for child in children {
        let path = current.join(child.file_name());
        if path == base.join(".symskills.json") {
            continue;
        }
        if entries.len() >= MAX_OUTPUT_ENTRIES {
            return Err(io::Error::other(
                "rendered output exceeds maximum entry count",
            ));
        }
        let metadata = root.symlink_metadata(&path)?;
        let relative = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if metadata.file_type().is_symlink() {
            return Err(io::Error::other("rendered output contains a symlink"));
        }
        let mode = file_mode(&metadata);
        if metadata.is_dir() {
            entries.push(OutputEntry {
                path: relative,
                kind: "dir".into(),
                mode: format!("{mode:04o}"),
                size: 0,
                sha256: String::new(),
            });
            collect_manifest_inner(root, &path, base, entries, total)?;
        } else if metadata.is_file() {
            let bytes = root.read(&path)?;
            *total = total.saturating_add(bytes.len() as u64);
            if *total > MAX_OUTPUT_BYTES {
                return Err(io::Error::other("rendered output exceeds maximum size"));
            }
            let sum = Sha256::digest(&bytes);
            entries.push(OutputEntry {
                path: relative,
                kind: "file".into(),
                mode: format!("{mode:04o}"),
                size: bytes.len() as u64,
                sha256: format!("{sum:x}"),
            });
        } else {
            return Err(io::Error::other(
                "rendered output contains a non-regular entry",
            ));
        }
    }
    Ok(())
}

fn manifest_digest(manifest: &[OutputEntry]) -> Result<String, SkillError> {
    let bytes = serde_json::to_vec(manifest)
        .map_err(|error| SkillError(format!("encode output manifest: {error}")))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
