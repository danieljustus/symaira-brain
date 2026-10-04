use std::{fs::File, io::BufReader, path::PathBuf};

use super::{
    Batch, Fact, Metadata, SessionImporter, SessionRef, Traits,
    bytes::{set, value},
    files,
    model::Time,
};

pub struct AiderImporter {
    roots: Vec<PathBuf>,
}

impl AiderImporter {
    /// The caller supplies the configured roots, or the owned HOME default.
    #[must_use]
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self { roots }
    }
}

impl SessionImporter for AiderImporter {
    fn name(&self) -> &'static str {
        "aider"
    }
    fn traits(&self) -> Traits {
        Traits::default()
    }

    fn discover(&self, since: Option<Time>) -> Batch<SessionRef> {
        let mut result = Batch::nil();
        let default;
        let roots = if self.roots.is_empty() {
            default = match files::home() {
                Ok(home) => vec![home],
                Err(error) => return Batch::failed(error),
            };
            &default
        } else {
            &self.roots
        };
        for root in roots {
            let entries = match files::walk(root) {
                Ok(entries) => entries,
                Err(error) => return Batch::failed(error),
            };
            for (path, info) in entries {
                if info.is_dir() {
                    continue;
                }
                let Ok(base) = files::basename(&path) else {
                    continue;
                };
                if base != b".aider.chat.history.md" {
                    continue;
                }
                let Ok(modified) = files::modified(&info) else {
                    continue;
                };
                if !files::eligible(modified, since) {
                    continue;
                }
                let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
                let Ok(project) = files::clean(parent).and_then(|p| files::path_bytes(&p)) else {
                    continue;
                };
                let mut metadata = Metadata::new();
                set(&mut metadata, b"project", project.clone());
                result.push(SessionRef {
                    tool: b"aider".to_vec(),
                    session_id: project,
                    path,
                    modified_at: Some(modified),
                    metadata,
                });
            }
        }
        result
    }

    fn import(&self, session: &SessionRef) -> Batch<Fact> {
        let file = match File::open(&session.path) {
            Ok(file) => file,
            Err(error) => return Batch::failed(error.into()),
        };
        parse(&mut BufReader::new(file), session)
    }
}

pub(super) fn parse(reader: &mut impl std::io::BufRead, session: &SessionRef) -> Batch<Fact> {
    let mut result = Batch::nil();
    let mut assistant = false;
    let mut has_role = false;
    let mut content = Vec::new();
    // The frozen Aider importer intentionally discards Scanner.Err, and
    // still flushes the successfully scanned assistant prefix at EOF/error.
    let _ignored_scanner_error = files::scan(reader, 65536, |line| {
        let header = line.starts_with(b"# ") || line.starts_with(b"## ");
        let human = line.starts_with(b"**Human**:");
        let bot = line.starts_with(b"**Assistant**:");
        if header || human || bot {
            flush(&mut result, session, assistant, &content);
            content.clear();
            assistant = bot;
            has_role = human || bot;
        } else if has_role {
            content.extend(line);
            content.push(b'\n');
        }
    });
    flush(&mut result, session, assistant, &content);
    result
}

fn flush(result: &mut Batch<Fact>, session: &SessionRef, assistant: bool, content: &[u8]) {
    if !assistant || content.len() <= 50 {
        return;
    }
    let mut metadata = Metadata::new();
    set(
        &mut metadata,
        b"project",
        value(&session.metadata, b"project").to_vec(),
    );
    result.push(Fact {
        content: content.to_vec(),
        source: b"aider".to_vec(),
        session_id: session.session_id.clone(),
        timestamp: session.modified_at,
        metadata,
        evidence: Vec::new(),
    });
}
