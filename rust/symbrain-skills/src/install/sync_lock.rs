//! Per-target pull locks used to serialize sync reinstalls.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::model::SkillError;

const PULL_LOCK_STALE_AFTER: Duration = Duration::from_mins(10);

#[derive(Debug)]
pub(crate) struct PullLock {
    path: PathBuf,
}

impl Drop for PullLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn pull_lock_path(home: &Path, target: &str, name: &str) -> Result<PathBuf, SkillError> {
    let home = super::destination::effective_home(home)?;
    Ok(home
        .join(".local/share/symskills/pending/.locks")
        .join(target)
        .join(format!("{name}.lock")))
}

fn pull_lock_held(target: &str, name: &str, path: &Path) -> SkillError {
    SkillError(format!(
        "pull lock held for {target}/{name} ({})",
        path.display()
    ))
}

pub(crate) fn acquire_pull_lock(
    home: &Path,
    target: &str,
    name: &str,
) -> Result<PullLock, SkillError> {
    let path = pull_lock_path(home, target, name)?;
    let parent = path
        .parent()
        .ok_or_else(|| SkillError("pull lock path has no parent".to_owned()))?;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o755);
    builder
        .create(parent)
        .map_err(|error| SkillError(error.to_string()))?;

    let record = format!(
        "{{\"pid\":{},\"created_at\":\"{}\"}}\n",
        std::process::id(),
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
    );
    for attempt in 0..2 {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o644);
        match options.open(&path) {
            Ok(mut file) => {
                if let Err(error) = file.write_all(record.as_bytes()) {
                    let _ = fs::remove_file(&path);
                    return Err(SkillError(error.to_string()));
                }
                return Ok(PullLock { path });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempt == 0 => {
                let stale = fs::metadata(&path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                    .is_some_and(|age| age > PULL_LOCK_STALE_AFTER);
                if stale && fs::remove_file(&path).is_ok() {
                    continue;
                }
                return Err(pull_lock_held(target, name, &path));
            }
            Err(_) => return Err(pull_lock_held(target, name, &path)),
        }
    }
    Err(pull_lock_held(target, name, &path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn lock_record_and_path_match_go_contract() {
        let home = tempdir().expect("home");
        let path = pull_lock_path(home.path(), "opencode", "example").expect("path");
        assert_eq!(
            path,
            home.path()
                .join(".local/share/symskills/pending/.locks/opencode/example.lock")
        );
        let lock = acquire_pull_lock(home.path(), "opencode", "example").expect("lock");
        let record = fs::read_to_string(&lock.path).expect("record");
        assert!(record.starts_with("{\"pid\":"));
        assert!(record.contains(",\"created_at\":\""));
        assert!(record.ends_with("}\n"));
        drop(lock);
        assert!(!path.exists());
    }

    #[test]
    fn existing_fresh_lock_reports_held() {
        let home = tempdir().expect("home");
        let path = pull_lock_path(home.path(), "opencode", "example").expect("path");
        fs::create_dir_all(path.parent().expect("parent")).expect("parent");
        fs::write(&path, b"{\"pid\":1}\n").expect("held lock");
        let error = acquire_pull_lock(home.path(), "opencode", "example")
            .expect_err("fresh lock must remain held");
        assert!(error.0.contains("pull lock held for opencode/example"));
    }
}
