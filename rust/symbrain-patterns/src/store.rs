use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::Episode;

const MAX_EPISODES: usize = 1000;
const MIN_EPISODE_LINE_BYTES: u64 = 64;

/// Concurrent JSONL episode store with bounded newest-first retention.
pub struct Store {
    path: PathBuf,
    private_dir: bool,
    lock: Mutex<()>,
}

impl Store {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            private_dir: false,
            lock: Mutex::new(()),
        }
    }

    #[must_use]
    pub fn new_private(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            private_dir: true,
            lock: Mutex::new(()),
        }
    }

    /// Appends one episode and prunes history to the newest 1000 valid entries.
    ///
    /// # Errors
    /// Returns an error when directory creation, serialization, writing, or pruning fails.
    pub fn append(&self, episode: &Episode) -> io::Result<()> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let existed = parent.exists();
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if self.private_dir || !existed {
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
            }
        }
        let data = serde_json::to_vec(episode).map_err(io::Error::other)?;
        let mut file = private_append(&self.path)?;
        file.write_all(&data)?;
        file.write_all(b"\n")?;
        file.flush()?;
        drop(file);
        self.prune_locked()
    }

    /// Loads every valid episode, silently skipping malformed lines.
    ///
    /// # Errors
    /// Returns an error when an existing file cannot be opened or read.
    pub fn load(&self) -> io::Result<Vec<Episode>> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        load_file(&self.path)
    }

    fn prune_locked(&self) -> io::Result<()> {
        let Ok(metadata) = fs::metadata(&self.path) else {
            return Ok(());
        };
        if metadata.len() < MAX_EPISODES as u64 * MIN_EPISODE_LINE_BYTES {
            return Ok(());
        }
        let episodes = load_file(&self.path)?;
        if episodes.len() <= MAX_EPISODES {
            return Ok(());
        }
        let temporary = self.path.with_extension(format!(
            "{}.tmp",
            self.path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
        ));
        let file = private_truncate(&temporary)?;
        let mut writer = BufWriter::new(file);
        for episode in &episodes[episodes.len() - MAX_EPISODES..] {
            serde_json::to_writer(&mut writer, episode).map_err(io::Error::other)?;
            writer.write_all(b"\n")?;
        }
        writer.flush()?;
        drop(writer);
        fs::rename(temporary, &self.path)
    }
}

fn load_file(path: &Path) -> io::Result<Vec<Episode>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut episodes = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if let Ok(episode) = serde_json::from_str(&line) {
            episodes.push(episode);
        }
    }
    Ok(episodes)
}

fn private_append(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).write(true).append(true);
    set_private_mode(&mut options);
    options.open(path)
}

fn private_truncate(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    set_private_mode(&mut options);
    options.open(path)
}

fn set_private_mode(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::Step;

    fn episode(index: usize) -> Episode {
        Episode {
            profile: "p".to_string(),
            steps: vec![Step {
                server: "vault".to_string(),
                tool: format!("tool-{index}"),
            }],
            started_at: "a".to_string(),
            ended_at: "b".to_string(),
        }
    }

    #[test]
    fn roundtrip_skips_malformed_and_prunes() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("p.jsonl");
        let store = Store::new(&path);
        fs::write(&path, "bad\n").expect("seed");
        for index in 0..1050 {
            store.append(&episode(index)).expect("append");
        }
        let episodes = store.load().expect("load");
        assert_eq!(episodes.len(), 1000);
        assert_eq!(episodes[0].steps[0].tool, "tool-50");
        assert_eq!(episodes[999].steps[0].tool, "tool-1049");
    }
}
