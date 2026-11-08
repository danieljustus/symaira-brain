//! Per-target/skill lock serializing pull mutations.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::model::SkillError;
use crate::pull::{PullOptions, pending_path};

/// A lock older than this may be recovered by a later run.
const LOCK_STALE_AFTER: Duration = Duration::from_secs(600);

/// Serializes mutations for one target/skill pair; released on drop.
pub(crate) struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) fn acquire(options: &PullOptions) -> Result<Lock, SkillError> {
    pending_path(options)?;
    let path = options
        .pending_dir
        .join(".locks")
        .join(&options.target)
        .join(format!("{}.lock", options.name));
    let held = || {
        SkillError(format!(
            "pull lock held for {}/{} ({})",
            options.target,
            options.name,
            path.display()
        ))
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| SkillError(error.to_string()))?;
    }
    for attempt in 0..2 {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write as _;
                let record = format!(
                    "{{\"pid\":{},\"created_at\":\"{}\"}}\n",
                    std::process::id(),
                    chrono::Utc::now().to_rfc3339()
                );
                file.write_all(record.as_bytes())
                    .map_err(|error| SkillError(error.to_string()))?;
                return Ok(Lock(path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempt == 0 => {
                let stale = fs::metadata(&path)
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|time| SystemTime::now().duration_since(time).ok())
                    .is_some_and(|age| age > LOCK_STALE_AFTER);
                if !stale || fs::remove_file(&path).is_err() {
                    return Err(held());
                }
            }
            Err(_) => return Err(held()),
        }
    }
    Err(held())
}
