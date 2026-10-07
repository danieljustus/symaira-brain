use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

pub(super) fn safe_cache_dir(path: &Path) -> io::Result<()> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if metadata.uid() == 0 {
                        current = fs::canonicalize(&current)?;
                        continue;
                    }
                }
                return Err(io::Error::other("cache directory crosses a user symlink"));
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(io::Error::other("cache parent is not a directory")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let builder = fs::DirBuilder::new();
                #[cfg(unix)]
                let builder = {
                    use std::os::unix::fs::DirBuilderExt;
                    let mut builder = builder;
                    builder.mode(0o700);
                    builder
                };
                builder.create(&current)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
