use std::{collections::BTreeMap, path::PathBuf};

use chrono::{DateTime, FixedOffset};

/// Go strings are bytes. File ownership and byte-cut content must survive
/// discovery without a lossy UTF-8 conversion.
pub type Metadata = BTreeMap<Vec<u8>, Vec<u8>>;
pub(super) type Time = DateTime<FixedOffset>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRef {
    pub tool: Vec<u8>,
    pub session_id: Vec<u8>,
    pub path: PathBuf,
    /// `None` represents the Go zero time, not an absent filesystem entry.
    pub modified_at: Option<Time>,
    pub metadata: Metadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub content: Vec<u8>,
    pub source: Vec<u8>,
    pub session_id: Vec<u8>,
    pub timestamp: Option<Time>,
    pub metadata: Metadata,
    /// Importer DTOs retain the bytes until the shared evidence owner can
    /// accept them. No extraction or storage is performed at this boundary.
    pub evidence: Vec<GroundedFact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroundedFact {
    pub source_id: Vec<u8>,
    pub source_kind: Vec<u8>,
    pub text: Vec<u8>,
    pub evidence_text: Vec<u8>,
    pub start: usize,
    pub end: usize,
    pub alignment: &'static str,
}

impl GroundedFact {
    /// Converts to the existing evidence owner without replacing source bytes.
    /// Raw facts remain available when this narrower text API cannot accept
    /// them; the registry must retain its admission gate in that case.
    ///
    /// # Errors
    /// Refuses invalid UTF-8 or offsets outside the shared evidence span type.
    pub fn to_extraction(&self) -> Result<crate::evidence::Extraction, ImportError> {
        let text = |bytes: &[u8]| {
            std::str::from_utf8(bytes)
                .map(str::to_owned)
                .map_err(|_| ImportError::Unsupported("shared evidence text requires valid UTF-8"))
        };
        Ok(crate::evidence::Extraction {
            source: crate::evidence::SourceRef {
                id: text(&self.source_id)?,
                kind: text(&self.source_kind)?,
            },
            text: text(&self.text)?,
            evidence_text: text(&self.evidence_text)?,
            span: crate::evidence::Span {
                start: i64::try_from(self.start)
                    .map_err(|_| ImportError::Unsupported("evidence start exceeds i64"))?,
                end: i64::try_from(self.end)
                    .map_err(|_| ImportError::Unsupported("evidence end exceeds i64"))?,
            },
            alignment_status: self.alignment.into(),
            attributes: BTreeMap::new(),
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Traits {
    pub category: Option<&'static str>,
    pub privacy: Option<&'static str>,
    pub pii_guard: Option<bool>,
    /// Presence matters: the Go registry tests interface implementation.
    pub transcript: Option<bool>,
    pub staged: Option<bool>,
    pub untrusted: Option<bool>,
    pub incremental: bool,
}

#[derive(Debug)]
pub enum ImportError {
    Io(std::io::Error),
    Message(&'static str),
    Unsupported(&'static str),
    Context(&'static str, Box<Self>),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Message(message) | Self::Unsupported(message) => formatter.write_str(message),
            Self::Context(context, error) => write!(formatter, "{context}: {error}"),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<std::io::Error> for ImportError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Go returns a slice AND an error. Scanner failures can retain earlier rows;
/// a `Result<Vec<_>, _>` would erase that distinction.
#[derive(Debug)]
pub struct Batch<T> {
    /// `None` retains a nil slice; `Some(vec![])` retains a non-nil empty slice.
    pub rows: Option<Vec<T>>,
    pub error: Option<ImportError>,
}

impl<T> Batch<T> {
    pub(super) fn nil() -> Self {
        Self {
            rows: None,
            error: None,
        }
    }

    pub(super) fn failed(error: ImportError) -> Self {
        Self {
            rows: None,
            error: Some(error),
        }
    }

    pub(super) fn push(&mut self, row: T) {
        self.rows.get_or_insert_with(Vec::new).push(row);
    }
}

pub trait SessionImporter {
    fn name(&self) -> &'static str;
    fn traits(&self) -> Traits;
    fn discover(&self, since: Option<Time>) -> Batch<SessionRef>;
    fn import(&self, session: &SessionRef) -> Batch<Fact>;
}
