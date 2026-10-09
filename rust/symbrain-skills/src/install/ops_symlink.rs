pub(crate) fn install_symlink(
    source: &Path,
    destination: &Path,
    marker: &[u8],
    fault: Option<FaultPoint>,
) -> Result<(), SkillError> {
    let parent = destination
        .parent()
        .ok_or_else(|| SkillError("destination has no parent".to_owned()))?;
    let root = replace::open_trusted_dir(parent)?;
    // Update the cache marker before publishing the link. The source is a
    // stable, capability-rooted render cache tree, never a user destination.
    replace::write_bytes(
        &replace::open_trusted_dir(source)?,
        Path::new(MARKER_FILE),
        marker,
        0o644,
        fault,
    )?;
    #[cfg(any(unix, windows))]
    {
        let name = replace::safe_name(destination.file_name())?;
        let backup = backup_existing(destination, fault)?;
        let temp = replace::unique_name(".symskills-link-")?;
        #[cfg(windows)]
        // Windows directory symlinks require an absolute target here. cap-std
        // rejects absolute link targets by design, so use the already-validated
        // managed cache source and the trusted destination parent path.
        let link_result = std::os::windows::fs::symlink_dir(source, parent.join(&temp));
        #[cfg(not(windows))]
        let link_result = root.symlink_contents(source, &temp);
        if let Err(error) = link_result {
            if let Some(backup) = backup {
                let _ = restore_backup(&backup, destination);
            }
            return Err(SkillError(format!("create managed symlink: {error}")));
        }
        let result = root.rename(&temp, &root, &name).map_err(|error| {
            let _ = root.remove_file(&temp);
            SkillError(format!("publish managed symlink: {error}"))
        });
        if let Err(error) = result {
            if let Some(backup) = backup {
                let _ = restore_backup(&backup, destination);
            }
            return Err(error);
        }
        if fault == Some(FaultPoint::RemoveBackup) {
            if let Some(backup) = backup {
                let _ = remove_entry(destination);
                restore_backup(&backup, destination)?;
                return Err(SkillError(
                    "injected replacement fault: RemoveBackup".to_owned(),
                ));
            }
        }
        if let Some(backup) = backup {
            remove_tree(&backup)?;
        }
        Ok(())
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        let _ = root;
        let _ = fault;
        let _ = marker;
        Err(SkillError(
            "symlink install is unsupported on this platform".to_owned(),
        ))
    }
}
