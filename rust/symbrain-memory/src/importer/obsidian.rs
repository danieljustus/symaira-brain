use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::BufReader,
    path::PathBuf,
};

use super::{
    Batch, Fact, ImportError, Metadata, SessionImporter, SessionRef, Traits,
    bytes::{json_list, json_map, set, trim},
    curated, files, markdown,
    model::Time,
};

pub struct ObsidianImporter {
    root: PathBuf,
    tags: Vec<Vec<u8>>,
    excluded_folders: Vec<Vec<u8>>,
    excluded_tags: Vec<Vec<u8>>,
}

impl ObsidianImporter {
    /// The frozen `folder` constructor parameter is not used by discovery.
    #[must_use]
    pub fn new(
        root: PathBuf,
        _folder: Vec<u8>,
        tags: Vec<Vec<u8>>,
        excluded_folders: Vec<Vec<u8>>,
        excluded_tags: Vec<Vec<u8>>,
    ) -> Self {
        let root = if root.as_os_str().is_empty() {
            detect().unwrap_or_default()
        } else {
            root
        };
        Self {
            root,
            tags,
            excluded_folders,
            excluded_tags,
        }
    }

    fn discover_owned(&self, since: Option<Time>) -> Result<Batch<SessionRef>, ImportError> {
        if self.root.as_os_str().is_empty() {
            return Err(ImportError::Message("no vault path configured"));
        }
        let root = self.root.clone();
        let mut result = Batch::nil();
        for (path, info) in files::walk(&root)? {
            if info.is_dir() || !files::basename(&path)?.ends_with(b".md") {
                continue;
            }
            let relative = files::relative(&root, &path)?;
            if self.excluded(&relative) {
                continue;
            }
            let Ok(modified) = files::modified(&info) else {
                continue;
            };
            if !files::eligible(modified, since) {
                continue;
            }
            let Ok(note) = parse(&path) else {
                continue;
            };
            if !self.tags.is_empty() && !note.tags.iter().any(|tag| self.tags.contains(tag)) {
                continue;
            }
            if note.tags.iter().any(|tag| self.excluded_tags.contains(tag)) {
                continue;
            }
            let mut metadata = Metadata::new();
            set(&mut metadata, b"title", note.title);
            set(&mut metadata, b"vault", files::basename(&root)?);
            set(
                &mut metadata,
                b"word_count",
                note.word_count.to_string().into_bytes(),
            );
            if !note.tags.is_empty() {
                set(&mut metadata, b"tags", json_list(&note.tags));
            }
            result.push(SessionRef {
                tool: b"obsidian".to_vec(),
                session_id: relative,
                path,
                modified_at: Some(modified),
                metadata,
            });
        }
        Ok(result)
    }

    fn excluded(&self, relative: &[u8]) -> bool {
        [
            b".obsidian".as_slice(),
            b".trash",
            b"Templates",
            b"node_modules",
        ]
        .into_iter()
        .chain(self.excluded_folders.iter().map(Vec::as_slice))
        .any(|folder| {
            let prefix = [folder, b"/"].concat();
            let contained = [b"/", folder, b"/"].concat();
            relative == folder
                || relative.starts_with(&prefix)
                || relative
                    .windows(contained.len())
                    .any(|part| part == contained)
        })
    }
}

impl SessionImporter for ObsidianImporter {
    fn name(&self) -> &'static str {
        "obsidian"
    }
    fn traits(&self) -> Traits {
        Traits {
            category: Some("notes"),
            privacy: Some("internal"),
            pii_guard: Some(false),
            ..Traits::default()
        }
    }

    fn discover(&self, since: Option<Time>) -> Batch<SessionRef> {
        self.discover_owned(since).unwrap_or_else(Batch::failed)
    }

    fn import(&self, session: &SessionRef) -> Batch<Fact> {
        let note = match parse(&session.path) {
            Ok(note) => note,
            Err(error) => {
                return Batch::failed(ImportError::Context(
                    "failed to parse note",
                    Box::new(error),
                ));
            }
        };
        let root = self.root.clone();
        let mut content = note.content;
        if content.len() > 5000 {
            content.truncate(5000);
            content.extend(b"...");
        }
        let mut fact_content = b"Note: ".to_vec();
        fact_content.extend(&note.title);
        fact_content.extend(b"\n\n");
        fact_content.extend(content);
        let mut metadata = Metadata::new();
        set(&mut metadata, b"source", b"obsidian".to_vec());
        match files::basename(&root) {
            Ok(vault) => set(&mut metadata, b"vault", vault),
            Err(error) => return Batch::failed(error),
        }
        set(&mut metadata, b"note_path", session.session_id.clone());
        set(&mut metadata, b"title", note.title);
        set(
            &mut metadata,
            b"word_count",
            note.word_count.to_string().into_bytes(),
        );
        set(
            &mut metadata,
            b"modified",
            markdown::rfc_seconds(note.timestamp),
        );
        if !note.tags.is_empty() {
            set(&mut metadata, b"tags", json_list(&note.tags));
        }
        if !note.links.is_empty() {
            set(&mut metadata, b"wikilinks", json_list(&note.links));
        }
        if !note.frontmatter.is_empty() {
            set(&mut metadata, b"frontmatter", json_map(&note.frontmatter));
        }
        Batch {
            rows: Some(vec![Fact {
                content: fact_content,
                source: b"obsidian".to_vec(),
                session_id: session.session_id.clone(),
                timestamp: note.timestamp,
                metadata,
                evidence: Vec::new(),
            }]),
            error: None,
        }
    }
}

struct Note {
    content: Vec<u8>,
    title: Vec<u8>,
    tags: Vec<Vec<u8>>,
    links: Vec<Vec<u8>>,
    frontmatter: Metadata,
    timestamp: Option<Time>,
    word_count: usize,
}

fn parse(path: &std::path::Path) -> Result<Note, ImportError> {
    files::path_bytes(path)?;
    let (content, frontmatter, _, word_count) =
        curated::parse(&mut BufReader::new(File::open(path)?))?;
    let title = content
        .split(|b| *b == b'\n')
        .find_map(|line| trim(line).strip_prefix(b"# ").map(<[u8]>::to_vec));
    let title = match title {
        Some(title) => title,
        None => {
            let basename = files::basename(path)?;
            basename.strip_suffix(b".md").unwrap_or(&basename).to_vec()
        }
    };
    let tags = collect(&content, r"#([a-zA-Z0-9_/\-]+)");
    // Unlike curated-memory, Obsidian does NOT TrimSpace inside wikilinks.
    let links = collect(&content, r"(?-u:\[\[([^\]|]+)(?:\|[^\]]+)?\]\])");
    let timestamp = fs::metadata(path)
        .ok()
        .and_then(|info| files::modified(&info).ok());
    Ok(Note {
        content,
        title,
        tags,
        links,
        frontmatter,
        timestamp,
        word_count,
    })
}

fn collect(content: &[u8], pattern: &str) -> Vec<Vec<u8>> {
    let re = regex::bytes::Regex::new(pattern).expect("fixed note regex");
    let mut seen = BTreeSet::new();
    re.captures_iter(content)
        .filter_map(|captures| {
            let value = captures.get(1)?.as_bytes().to_vec();
            if seen.insert(value.clone()) {
                Some(value)
            } else {
                None
            }
        })
        .collect()
}

fn detect() -> Result<PathBuf, ImportError> {
    if let Some(path) = std::env::var_os("OBSIDIAN_VAULT").filter(|path| !path.is_empty()) {
        if fs::metadata(&path).is_ok() {
            return Ok(path.into());
        }
    }
    let home = files::home()?;
    let path = files::join(
        &home,
        b"Library/Mobile Documents/iCloud~md~obsidian/Documents/LifeOS",
    )?;
    if fs::metadata(&path).is_ok() {
        Ok(path)
    } else {
        Ok(PathBuf::new())
    }
}
