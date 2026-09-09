fn collect_existing(
    root: &Dir,
    relative: &Path,
    output_root: &Path,
    source_hash: String,
) -> Result<Materialized, SkillError> {
    let mut files = Vec::new();
    collect_files(root, relative, relative, &mut files).map_err(|error| io_error(&error))?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(Materialized {
        root: output_root.join(relative),
        source_hash,
        files,
    })
}

fn collect_files(
    root: &Dir,
    current: &Path,
    base: &Path,
    files: &mut Vec<MaterializedFile>,
) -> io::Result<()> {
    let mut entries = root.read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(cap_std::fs::DirEntry::file_name);
    for entry in entries {
        let path = current.join(entry.file_name());
        let metadata = root.symlink_metadata(&path)?;
        if metadata.is_dir() {
            collect_files(root, &path, base, files)?;
        } else if metadata.is_file() {
            files.push(MaterializedFile {
                path: path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                mode: format!("{:04o}", file_mode(&metadata)),
                bytes: root.read(&path)?,
            });
        }
    }
    Ok(())
}

use fs2::FileExt;

fn lock_destination(root: &Dir, parent: &Path, destination: &Path) -> io::Result<std::fs::File> {
    let name = destination
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no name"))?;
    let path = parent.join(format!(".symskills-lock-{}", name.to_string_lossy()));
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .follow(FollowSymlinks::No);
    let file = {
        let mut opened = None;
        for attempt in 0..16 {
            match root.open_with(&path, &options) {
                Ok(file) => {
                    opened = Some(file);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound && attempt < 15 => {
                    std::thread::yield_now();
                }
                Err(error) => {
                    return Err(io::Error::new(
                        error.kind(),
                        format!("open lock {}: {error}", path.display()),
                    ));
                }
            }
        }
        opened.expect("lock open retry loop must either return or open")
    }
    .into_std();
    file.lock_exclusive().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("lock destination {}: {error}", path.display()),
        )
    })?;
    Ok(file)
}

fn open_root(path: &Path) -> io::Result<Dir> {
    let absolute = std::path::absolute(path)?;
    let absolute = normalize_system_alias(&absolute);
    let mut root = Dir::open_ambient_dir(Path::new("/"), ambient_authority())?;
    for component in absolute.components() {
        let Component::Normal(name) = component else {
            if matches!(component, Component::RootDir) {
                continue;
            }
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "unsafe destination root"));
        };
        match root.open_dir_nofollow(name) {
            Ok(child) => root = child,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                root.create_dir(name).map_err(|error| {
                    io::Error::new(
                        error.kind(),
                        format!("create destination component {}: {error}", name.to_string_lossy()),
                    )
                })?;
                root = root.open_dir_nofollow(name)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(root)
}

fn normalize_system_alias(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    for (alias, real) in [("/var", "/private/var"), ("/tmp", "/private/tmp")] {
        if (text == alias || text.starts_with(&format!("{alias}/")))
            && std::fs::read_link(alias).is_ok_and(|target| {
                target == Path::new(real) || target == Path::new(real.trim_start_matches('/'))
            })
        {
            let suffix = text.strip_prefix(alias).unwrap_or_default();
            return PathBuf::from(real).join(suffix.trim_start_matches('/'));
        }
    }
    path.to_path_buf()
}


fn ensure_dir(root: &Dir, relative: &Path) -> io::Result<()> {
    let mut current = root.try_clone()?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsafe destination path",
            ));
        };
        match current.create_dir(name) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        current = current.open_dir_nofollow(name)?;
    }
    Ok(())
}

fn unique_sibling_name(root: &Dir, parent: &Path, prefix: &str) -> io::Result<PathBuf> {
    for _ in 0..128 {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let name = format!("{prefix}{}-{id:016x}", std::process::id());
        if name.len() > 255 {
            return Err(io::Error::other("temporary name exceeds component limit"));
        }
        let path = parent.join(name);
        if root.symlink_metadata(&path).is_err() {
            return Ok(path);
        }
    }
    Err(io::Error::other(
        "unable to allocate collision-free temporary name",
    ))
}

fn make_sibling_dir(root: &Dir, parent: &Path, prefix: &str) -> io::Result<PathBuf> {
    for _ in 0..128 {
        let path = unique_sibling_name(root, parent, prefix)?;
        match root.create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(
        "unable to create collision-free staging directory",
    ))
}

fn rollback_swap(
    root: &Dir,
    new_path: &Path,
    destination: &Path,
    backup: Option<&Path>,
    old_exists: bool,
    cause: &io::Error,
    parent: &Path,
) -> SkillError {
    let mut rollback_errors = Vec::new();
    if root.symlink_metadata(new_path).is_ok() {
        let removal = root.remove_dir_all(new_path);
        if let Err(error) = removal {
            rollback_errors.push(format!("remove new tree: {error}"));
        }
    }
    if let Some(backup) = backup.filter(|_| old_exists) {
        match root.rename(backup, root, destination) {
            Ok(()) => {
                if let Err(error) = sync_dir(root, parent, None) {
                    rollback_errors.push(format!("sync restored parent: {error}"));
                }
            }
            Err(error) => rollback_errors.push(format!("restore old tree: {error}")),
        }
    }
    if rollback_errors.is_empty() {
        io_error(cause)
    } else {
        SkillError(format!(
            "materialize: {cause} (rollback failed: {})",
            rollback_errors.join("; ")
        ))
    }
}

fn fault(operation: Option<&str>, expected: &str) -> io::Result<()> {
    if operation == Some(expected) {
        return Err(io::Error::other(format!(
            "injected materialization fault: {expected}"
        )));
    }
    Ok(())
}

fn write_file(
    root: &Dir,
    path: &Path,
    bytes: &[u8],
    mode: u32,
    fault_operation: Option<&str>,
) -> io::Result<()> {
    fault(fault_operation, "write")?;
    #[cfg(not(unix))]
    let _ = mode;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    ensure_dir(root, parent)?;
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create(true)
        .truncate(true)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    options.mode(mode);
    let mut file = root.open_with(path, &options)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

fn sync_tree(root: &Dir, path: &Path, fault_operation: Option<&str>) -> io::Result<()> {
    let mut entries = root.read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(cap_std::fs::DirEntry::file_name);
    for entry in entries {
        let child = path.join(entry.file_name());
        if root.symlink_metadata(&child)?.is_dir() {
            sync_tree(root, &child, fault_operation)?;
        }
    }
    sync_dir(root, path, fault_operation)
}

fn sync_dir(root: &Dir, path: &Path, fault_operation: Option<&str>) -> io::Result<()> {
    fault(fault_operation, "sync-dir")?;
    root.open_dir_nofollow(path)?.into_std_file().sync_all()
}

fn parse_mode(value: &str) -> u32 {
    u32::from_str_radix(value.trim(), 8).unwrap_or(0o644) & 0o777
}

fn file_mode(metadata: &cap_std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o777
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0o644
    }
}

fn io_error(error: &io::Error) -> SkillError {
    SkillError(format!("materialize: {error}"))
}
