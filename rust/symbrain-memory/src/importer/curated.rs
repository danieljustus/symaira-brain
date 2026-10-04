use std::{
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
};

use super::{
    Batch, Fact, ImportError, Metadata, SessionImporter, SessionRef, Traits,
    bytes::{fields, json_list, json_map, set, trim, value},
    files, markdown,
    model::Time,
};

pub struct CuratedMemoryImporter {
    home: PathBuf,
}

impl CuratedMemoryImporter {
    /// The HOME value is supplied by the owner; it is not canonicalized.
    #[must_use]
    pub fn new(home: PathBuf) -> Self {
        Self { home }
    }

    fn discover_owned(&self, since: Option<Time>) -> Result<Batch<SessionRef>, ImportError> {
        let mut result = Batch::nil();
        let home = if self.home.as_os_str().is_empty() {
            files::home()?
        } else {
            self.home.clone()
        };
        let base = files::join(&home, b".claude/projects")?;
        for (path, info) in files::walk(&base)? {
            if info.is_dir() {
                continue;
            }
            let name = files::basename(&path)?;
            if !name.ends_with(b".md") {
                continue;
            }
            if name == b"MEMORY.md" {
                let parent = path.parent().unwrap_or_else(|| Path::new("."));
                if files::basename(parent)? == b"memory"
                    || fs::metadata(files::join(parent, b"memory")?).is_ok()
                {
                    continue;
                }
            }
            let Ok(modified) = files::modified(&info) else {
                continue;
            };
            if !files::eligible(modified, since) {
                continue;
            }
            let relative = files::relative(&base, &path)?;
            let mut metadata = Metadata::new();
            set(&mut metadata, b"source_tool", b"claude-code".to_vec());
            set(
                &mut metadata,
                b"project",
                relative
                    .split(|b| *b == b'/')
                    .next()
                    .unwrap_or_default()
                    .to_vec(),
            );
            let mut id = b"claude-code:".to_vec();
            id.extend(relative);
            result.push(SessionRef {
                tool: b"curated-memory".to_vec(),
                session_id: id,
                path,
                modified_at: Some(modified),
                metadata,
            });
        }
        let base = files::join(&home, b".hermes/memories")?;
        for name in [b"MEMORY.md".as_slice(), b"USER.md"] {
            let path = files::join(&base, name)?;
            let Ok(info) = fs::metadata(&path) else {
                continue;
            };
            let Ok(modified) = files::modified(&info) else {
                continue;
            };
            if !files::eligible(modified, since) {
                continue;
            }
            let mut id = b"hermes:".to_vec();
            id.extend(name);
            let mut metadata = Metadata::new();
            set(&mut metadata, b"source_tool", b"hermes".to_vec());
            result.push(SessionRef {
                tool: b"curated-memory".to_vec(),
                session_id: id,
                path,
                modified_at: Some(modified),
                metadata,
            });
        }
        Ok(result)
    }
}

impl SessionImporter for CuratedMemoryImporter {
    fn name(&self) -> &'static str {
        "curated-memory"
    }
    fn traits(&self) -> Traits {
        Traits {
            category: Some("notes"),
            privacy: Some("confidential"),
            pii_guard: Some(true),
            ..Traits::default()
        }
    }

    fn discover(&self, since: Option<Time>) -> Batch<SessionRef> {
        self.discover_owned(since).unwrap_or_else(Batch::failed)
    }

    fn import(&self, session: &SessionRef) -> Batch<Fact> {
        if let Err(error) = files::path_bytes(&session.path) {
            return Batch::failed(error);
        }
        let file = match File::open(&session.path) {
            Ok(file) => file,
            Err(error) => return Batch::failed(error.into()),
        };
        let parsed = parse(&mut BufReader::new(file));
        let (mut content, frontmatter, links, word_count) = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                return Batch::failed(ImportError::Context(
                    "failed to parse memory file",
                    Box::new(error),
                ));
            }
        };
        if content.is_empty() {
            return Batch::nil();
        }
        if content.len() > 5000 {
            content.truncate(5000);
            content.extend(b"...");
        }
        // Go stats again after parsing, ignores that error and ignores the
        // supplied ModifiedAt. Discovery timestamps are not import timestamps.
        let timestamp = fs::metadata(&session.path)
            .ok()
            .and_then(|info| files::modified(&info).ok());
        let mut metadata = Metadata::new();
        set(&mut metadata, b"source", session.tool.clone());
        set(&mut metadata, b"file_path", session.session_id.clone());
        set(
            &mut metadata,
            b"word_count",
            word_count.to_string().into_bytes(),
        );
        set(&mut metadata, b"modified", markdown::rfc_seconds(timestamp));
        if !value(&session.metadata, b"project").is_empty() {
            set(
                &mut metadata,
                b"project",
                value(&session.metadata, b"project").to_vec(),
            );
        }
        for (input, output) in [
            (b"name".as_slice(), b"title".as_slice()),
            (b"description", b"description"),
            (b"type", b"memory_type"),
        ] {
            if let Some(value) = frontmatter.get(input) {
                set(&mut metadata, output, value.clone());
            }
        }
        if !frontmatter.is_empty() {
            set(&mut metadata, b"frontmatter", json_map(&frontmatter));
        }
        if !links.is_empty() {
            set(&mut metadata, b"links", json_list(&links));
        }
        Batch {
            rows: Some(vec![Fact {
                content,
                source: session.tool.clone(),
                session_id: session.session_id.clone(),
                timestamp,
                metadata,
                evidence: Vec::new(),
            }]),
            error: None,
        }
    }
}

type Parsed = (Vec<u8>, Metadata, Vec<Vec<u8>>, usize);

pub(super) fn parse(reader: &mut impl std::io::BufRead) -> Result<Parsed, ImportError> {
    let mut frontmatter = Metadata::new();
    let mut lines = Vec::new();
    let mut first = true;
    let mut in_frontmatter = false;
    let mut content = Vec::new();
    files::scan(reader, 65536, |line| {
        if first {
            first = false;
            if line == b"---" {
                in_frontmatter = true;
                return;
            }
        }
        if in_frontmatter {
            if line == b"---" {
                in_frontmatter = false;
                frontmatter = markdown::simple_yaml(&lines);
            } else {
                lines.push(line.to_vec());
            }
            return;
        }
        content.extend(line);
        content.push(b'\n');
    })?;
    let content = trim(&content).to_vec();
    let word_count = fields(&content).len();
    let links = markdown::links(&content);
    Ok((content, frontmatter, links, word_count))
}
