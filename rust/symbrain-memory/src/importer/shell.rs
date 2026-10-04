use std::{
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
};

use super::{
    Batch, Fact, ImportError, Metadata, SessionImporter, SessionRef, Traits,
    bytes::{fields, set, value},
    files, markdown,
    model::Time,
};

pub struct ShellHistoryImporter {
    path: PathBuf,
    filters: Vec<Vec<u8>>,
}

impl ShellHistoryImporter {
    /// `success_only` is retained as a constructor input but the frozen owner
    /// never applies it (nor its 1000 ms duration field) to history records.
    #[must_use]
    pub fn new(path: PathBuf, _success_only: bool, filters: Vec<Vec<u8>>) -> Self {
        let path = if path.as_os_str().is_empty() {
            files::home()
                .ok()
                .and_then(|home| {
                    let shell = std::env::var_os("SHELL").unwrap_or_default();
                    let shell = files::path_bytes(Path::new(&shell)).ok()?;
                    let zsh = files::join(&home, b".zsh_history").ok()?;
                    if contains(&shell, b"zsh") && fs::metadata(&zsh).is_ok() {
                        Some(zsh)
                    } else {
                        files::join(&home, b".bash_history").ok()
                    }
                })
                .unwrap_or_default()
        } else {
            path
        };
        Self { path, filters }
    }

    /// Selects the default path using only the caller-owned HOME and SHELL.
    ///
    /// # Errors
    /// Refuses unported native non-Unix path contracts.
    pub fn from_home(
        home: &Path,
        shell: &[u8],
        success_only: bool,
        filters: Vec<Vec<u8>>,
    ) -> Result<Self, ImportError> {
        let zsh = files::join(home, b".zsh_history")?;
        let path = if contains(shell, b"zsh") && fs::metadata(&zsh).is_ok() {
            zsh
        } else {
            files::join(home, b".bash_history")?
        };
        Ok(Self::new(path, success_only, filters))
    }
}

impl SessionImporter for ShellHistoryImporter {
    fn name(&self) -> &'static str {
        "shell-history"
    }
    fn traits(&self) -> Traits {
        Traits {
            category: Some("code"),
            privacy: Some("confidential"),
            pii_guard: Some(true),
            ..Traits::default()
        }
    }

    fn discover(&self, since: Option<Time>) -> Batch<SessionRef> {
        if self.path.as_os_str().is_empty() {
            return Batch::failed(ImportError::Message("no shell history file found"));
        }
        if let Err(error) = files::path_bytes(&self.path) {
            return Batch::failed(error);
        }
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) => return Batch::failed(error.into()),
        };
        discover_lines(&mut BufReader::new(file), &self.path, &self.filters, since)
    }

    fn import(&self, session: &SessionRef) -> Batch<Fact> {
        let command = value(&session.metadata, b"command");
        if command.is_empty() {
            return Batch::nil();
        }
        let mut metadata = Metadata::new();
        set(&mut metadata, b"command", command.to_vec());
        set(&mut metadata, b"source", b"shell-history".to_vec());
        let words = fields(command);
        if let Some(bin) = words.first() {
            let bin = match files::from_bytes(bin.to_vec()).and_then(|path| files::basename(&path))
            {
                Ok(bin) => bin,
                Err(error) => return Batch::failed(error),
            };
            let tag = match bin.as_slice() {
                b"brew" | b"npm" | b"yarn" | b"pnpm" | b"pip" | b"uv" => "package-manager",
                b"go" | b"git" => "vcs",
                b"gh" => "github",
                b"docker" => "container",
                b"make" | b"cargo" | b"cmake" => "build",
                _ => "",
            };
            if !tag.is_empty() {
                set(&mut metadata, b"tag", tag.as_bytes().to_vec());
            }
        }
        let mut content = b"User ran: ".to_vec();
        content.extend(command);
        Batch {
            rows: Some(vec![Fact {
                content,
                source: b"shell-history".to_vec(),
                session_id: session.session_id.clone(),
                timestamp: session.modified_at,
                metadata,
                evidence: Vec::new(),
            }]),
            error: None,
        }
    }
}

pub(super) fn discover_lines(
    reader: &mut impl std::io::BufRead,
    path: &Path,
    filters: &[Vec<u8>],
    since: Option<Time>,
) -> Batch<SessionRef> {
    let mut result = Batch::nil();
    let mut unsupported = false;
    let error = files::scan(reader, 65536, |line| {
        let Some((seconds, command)) = history_line(line) else {
            return;
        };
        let Some(timestamp) = markdown::epoch(seconds) else {
            unsupported = true;
            return;
        };
        if !files::eligible(timestamp, since) {
            return;
        }
        let words = fields(command);
        if excluded(command) || words.first().is_some_and(|bin| excluded(bin)) {
            return;
        }
        if !filters.is_empty() && !filters.iter().any(|filter| contains(command, filter)) {
            return;
        }
        let mut metadata = Metadata::new();
        set(&mut metadata, b"command", command.to_vec());
        result.push(SessionRef {
            tool: b"shell-history".to_vec(),
            session_id: seconds.to_string().into_bytes(),
            path: path.to_path_buf(),
            modified_at: Some(timestamp),
            metadata,
        });
    });
    result.error = if unsupported {
        Some(ImportError::Unsupported(
            "Go epoch lies outside the native timestamp domain",
        ))
    } else {
        error.err()
    };
    result
}

fn history_line(line: &[u8]) -> Option<(i64, &[u8])> {
    if let Some(raw) = line.strip_prefix(b"#") {
        if raw.is_empty() || !raw.iter().all(u8::is_ascii_digit) {
            return None;
        }
        return Some((seconds(raw), &[]));
    }
    let raw = line.strip_prefix(b": ")?;
    let colon = raw.iter().position(|b| *b == b':')?;
    let number = &raw[..colon];
    if number.is_empty() || !number.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let tail = &raw[colon + 1..];
    let semicolon = tail.iter().position(|b| *b == b';')?;
    if semicolon == 0
        || !tail[..semicolon].iter().all(u8::is_ascii_digit)
        || semicolon + 1 == tail.len()
    {
        return None;
    }
    Some((seconds(number), &tail[semicolon + 1..]))
}

fn seconds(raw: &[u8]) -> i64 {
    // strconv.ParseInt's positive overflow value is MaxInt64; Go ignores
    // its error. The chrono boundary is refused explicitly by the caller.
    std::str::from_utf8(raw)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(i64::MAX)
}

fn excluded(command: &[u8]) -> bool {
    matches!(
        command,
        b"cd" | b"ls" | b"pwd" | b"echo" | b"cat" | b"exit" | b"logout" | b"history" | b"clear"
    )
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || bytes.windows(needle.len()).any(|value| value == needle)
}
