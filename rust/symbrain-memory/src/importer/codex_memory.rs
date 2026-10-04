use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use super::{
    Batch, Fact, GroundedFact, ImportError, Metadata, SessionImporter, SessionRef, Traits,
    bytes::{equal_fold, json_map, set, trim, value},
    files, markdown,
    model::Time,
};

#[derive(Debug, Default)]
pub struct ApplicationPolicy {
    pub allowed: Vec<Vec<u8>>,
    pub denied: Vec<Vec<u8>>,
}

pub struct CodexMemoryImporter {
    pub(super) root: PathBuf,
    allow: BTreeSet<Vec<u8>>,
    deny: BTreeSet<Vec<u8>>,
    last_import: Mutex<Option<Time>>,
}

impl CodexMemoryImporter {
    pub(super) fn root_dir(&self) -> Result<PathBuf, ImportError> {
        if self.root.as_os_str().is_empty() {
            files::join(&files::home()?, b".codex/memories")
        } else {
            files::clean(&self.root)
        }
    }
    #[must_use]
    pub fn new(root: PathBuf, policy: ApplicationPolicy) -> Self {
        let strings = |values: Vec<Vec<u8>>| {
            values
                .into_iter()
                .map(|v| trim(&v).to_vec())
                .filter(|v| !v.is_empty())
                .collect()
        };
        Self {
            root,
            allow: strings(policy.allowed),
            deny: strings(policy.denied),
            last_import: Mutex::new(None),
        }
    }

    /// Resolves the default tier from a caller-owned HOME. No environment or
    /// operator directory is read merely by constructing this adapter.
    ///
    /// # Errors
    /// Refuses platforms whose lexical/raw path contract is not ported yet.
    pub fn from_home(home: &Path, policy: ApplicationPolicy) -> Result<Self, ImportError> {
        Ok(Self::new(files::join(home, b".codex/memories")?, policy))
    }

    /// Reads the optional in-memory cursor; durable markers remain separate.
    ///
    /// # Errors
    /// Reports a poisoned cursor lock.
    pub fn last_import_time(&self) -> Result<Option<Time>, ImportError> {
        self.last_import
            .lock()
            .map(|time| *time)
            .map_err(|_| ImportError::Message("import cursor lock poisoned"))
    }

    /// Advances only to a later imported session, without writing its tree.
    ///
    /// # Errors
    /// Reports a poisoned cursor lock.
    pub fn mark_imported(&self, session: &SessionRef) -> Result<(), ImportError> {
        let mut last = self
            .last_import
            .lock()
            .map_err(|_| ImportError::Message("import cursor lock poisoned"))?;
        if let Some(modified) = session.modified_at {
            let after = last.map_or(
                modified.timestamp() > -62_135_596_800
                    || (modified.timestamp() == -62_135_596_800
                        && modified.timestamp_subsec_nanos() > 0),
                |previous| modified > previous,
            );
            if after {
                *last = Some(modified);
            }
        }
        Ok(())
    }

    pub(super) fn application_allowed(&self, path: &Path) -> bool {
        if self.allow.is_empty() && self.deny.is_empty() {
            return true;
        }
        let Ok(data) = files::markdown(path) else {
            return false;
        };
        let (frontmatter, _) = markdown::codex(&data);
        let apps = markdown::applications(value(&frontmatter, b"applications"));
        if apps.iter().any(|app| self.deny.contains(app)) {
            return false;
        }
        self.allow.is_empty() || apps.iter().any(|app| self.allow.contains(app))
    }

    fn import_one(&self, session: &SessionRef) -> Result<Batch<Fact>, ImportError> {
        if equal_fold(&files::basename(&session.path)?, b"instructions.md") {
            return Ok(Batch::nil());
        }
        let root = self.root_dir()?;
        let Ok(relative) = files::relative(&root, &session.path) else {
            return Ok(Batch::nil());
        };
        if relative == b".."
            || relative.starts_with(b"../")
            || !self.application_allowed(&session.path)
        {
            return Ok(Batch::nil());
        }
        let data = files::markdown(&session.path)?;
        let (frontmatter, content) = markdown::codex(&data);
        let content = trim(&content).to_vec();
        if content.is_empty() {
            return Ok(Batch::nil());
        }
        let timestamp = files::modified(&fs::metadata(&session.path)?)?;
        let mut metadata = session.metadata.clone();
        set(
            &mut metadata,
            b"title",
            value(&frontmatter, b"title").to_vec(),
        );
        set(
            &mut metadata,
            b"description",
            value(&frontmatter, b"description").to_vec(),
        );
        if !frontmatter.is_empty() {
            set(&mut metadata, b"frontmatter", json_map(&frontmatter));
        }
        let apps = markdown::applications(value(&frontmatter, b"applications"));
        if !apps.is_empty() {
            set(&mut metadata, b"applications", apps.join(&b','));
        }
        set(
            &mut metadata,
            b"source_path",
            files::path_bytes(&session.path)?,
        );
        let citations = markdown::citations(&content);
        if !citations.is_empty() {
            set(&mut metadata, b"citations", citations);
        }
        if let Some((kind, start)) = markdown::resource(&files::basename(&session.path)?) {
            markdown::activity_metadata(&mut metadata, kind, start);
        }
        if value(&metadata, b"sync_exclude").is_empty() {
            set(&mut metadata, b"sync_exclude", b"true".to_vec());
        }
        if value(&metadata, b"promotion_policy").is_empty() {
            set(
                &mut metadata,
                b"promotion_policy",
                markdown::PROMOTION.to_vec(),
            );
        }
        let evidence = GroundedFact {
            source_id: session.session_id.clone(),
            source_kind: b"codex-memory".to_vec(),
            text: content.clone(),
            evidence_text: content.clone(),
            start: 0,
            end: content.len(),
            alignment: "exact",
        };
        Ok(Batch {
            rows: Some(vec![Fact {
                content,
                source: b"codex-memory".to_vec(),
                session_id: session.session_id.clone(),
                timestamp: session.modified_at.or(Some(timestamp)),
                metadata,
                evidence: vec![evidence],
            }]),
            error: None,
        })
    }
}

impl SessionImporter for CodexMemoryImporter {
    fn name(&self) -> &'static str {
        "codex-memory"
    }

    fn traits(&self) -> Traits {
        Traits {
            category: Some("activity"),
            privacy: Some("confidential"),
            pii_guard: Some(true),
            transcript: Some(true),
            staged: Some(true),
            untrusted: Some(true),
            incremental: true,
        }
    }

    fn discover(&self, since: Option<Time>) -> Batch<SessionRef> {
        self.discover_sessions(since).unwrap_or_else(Batch::failed)
    }

    fn import(&self, session: &SessionRef) -> Batch<Fact> {
        self.import_one(session).unwrap_or_else(Batch::failed)
    }
}
