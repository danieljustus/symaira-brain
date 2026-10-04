use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use chrono::Duration;

use super::{
    Batch, CodexMemoryImporter, ImportError, Metadata, SessionRef,
    bytes::{equal_fold, set},
    files, markdown,
    model::Time,
};

impl CodexMemoryImporter {
    pub(super) fn discover_sessions(
        &self,
        since: Option<Time>,
    ) -> Result<Batch<SessionRef>, ImportError> {
        let root = self.root_dir()?;
        match fs::metadata(&root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Batch::nil()),
            Err(error) => return Err(error.into()),
            Ok(_) => (),
        }
        let paths = discover_paths(&root)?;
        let mut parents = Vec::new();
        for path in &paths {
            if let Some(("6h", start)) = markdown::resource(&files::basename(path)?) {
                if self.application_allowed(path) {
                    parents.push(start);
                }
            }
        }
        // Go allocates its output slice after a present root, even if empty.
        let mut result = Batch {
            rows: Some(Vec::new()),
            error: None,
        };
        for path in paths {
            let Ok(info) = fs::metadata(&path) else {
                continue;
            };
            if !info.is_file() {
                continue;
            }
            let Ok(modified) = files::modified(&info) else {
                continue;
            };
            if !files::eligible(modified, since) || !self.application_allowed(&path) {
                continue;
            }
            let relative = files::relative(&root, &path)?;
            let mut metadata = Metadata::new();
            set(&mut metadata, b"source_kind", b"codex_markdown".to_vec());
            set(&mut metadata, b"file_path", files::path_bytes(&path)?);
            set(&mut metadata, b"sync_exclude", b"true".to_vec());
            set(
                &mut metadata,
                b"promotion_policy",
                markdown::PROMOTION.to_vec(),
            );
            if let Some((kind, start)) = markdown::resource(&files::basename(&path)?) {
                let parts: Vec<_> = relative.split(|b| *b == b'/').collect();
                set(
                    &mut metadata,
                    b"extension",
                    if parts.len() >= 2 && parts[0] == b"extensions" {
                        parts[1].to_vec()
                    } else {
                        Vec::new()
                    },
                );
                markdown::activity_metadata(&mut metadata, kind, start);
                // Coverage is global across extensions, and includes old
                // parent files. Preserve that source quirk explicitly.
                if kind == "10min"
                    && parents
                        .iter()
                        .any(|parent| start >= *parent && start < *parent + Duration::hours(6))
                {
                    continue;
                }
            } else {
                set(&mut metadata, b"granularity", b"consolidated".to_vec());
            }
            if relative == b"." || relative.starts_with(b"../") {
                continue;
            }
            let mut id = b"codex-memory:".to_vec();
            id.extend(relative);
            result.push(SessionRef {
                tool: b"codex-memory".to_vec(),
                session_id: id,
                path,
                modified_at: Some(modified),
                metadata,
            });
        }
        Ok(result)
    }
}

fn discover_paths(root: &Path) -> Result<Vec<PathBuf>, ImportError> {
    let mut paths = BTreeSet::new();
    for name in [
        b"MEMORY.md".as_slice(),
        b"memory_summary.md",
        b"raw_memories.md",
    ] {
        let path = files::join(root, name)?;
        if fs::metadata(&path).is_ok_and(|info| info.is_file()) {
            paths.insert(path);
        }
    }
    for (path, info) in files::walk(root)? {
        if info.is_dir() {
            continue;
        }
        let Ok(relative) = files::relative(root, &path) else {
            continue;
        };
        let parts: Vec<_> = relative.split(|b| *b == b'/').collect();
        let base = files::basename(&path)?;
        let extension = base
            .iter()
            .rposition(|b| *b == b'.')
            .map_or(&[][..], |i| &base[i..]);
        if !equal_fold(extension, b".md") {
            continue;
        }
        if (parts.len() == 2 && parts[0] == b"rollout_summaries")
            || (parts.len() >= 4 && parts[0] == b"extensions" && parts[2] == b"resources")
        {
            paths.insert(path);
        }
    }
    Ok(paths.into_iter().collect())
}
