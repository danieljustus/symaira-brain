//! Output format parsing, flag extraction, and rendering for Symaira commands.

use serde::Serialize;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Write};

/// Output format for CLI commands: stable human-readable table or machine-readable JSON.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum OutputFormat {
    /// Human-readable layout (table/text).
    #[default]
    Table,
    /// Machine-readable JSON representation.
    Json,
}

impl OutputFormat {
    /// Returns the canonical string representation ("table" or "json").
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Json => "json",
        }
    }

    /// Resolves an explicit format string into an `OutputFormat`, defaulting to `Table`
    /// when empty or unrecognized.
    #[must_use]
    pub fn resolve(explicit: &str) -> Self {
        if explicit.trim().eq_ignore_ascii_case("json") {
            Self::Json
        } else {
            Self::Table
        }
    }

    /// Parses an explicit format string, returning an error for unrecognized formats.
    /// An empty string or "table" resolves to `Table`; "json" resolves to `Json`.
    ///
    /// # Errors
    ///
    /// Returns `OutputError::UnsupportedFormat` if `value` is not empty, "table", or "json".
    pub fn parse(value: &str) -> Result<Self, OutputError> {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("table") {
            Ok(Self::Table)
        } else if trimmed.eq_ignore_ascii_case("json") {
            Ok(Self::Json)
        } else {
            Err(OutputError::UnsupportedFormat(value.to_owned()))
        }
    }
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Errors that can occur during output flag extraction and format parsing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputError {
    /// An `--output` or `-output` flag was provided without a following value.
    MissingValue(String),
    /// An unsupported output format was requested.
    UnsupportedFormat(String),
    /// Two or more conflicting output formats were provided.
    ConflictingFormats {
        current: OutputFormat,
        next: OutputFormat,
    },
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue(flag) => write!(f, "{flag} requires a value"),
            Self::UnsupportedFormat(value) => {
                write!(
                    f,
                    "unsupported output format {value:?} (want table or json)"
                )
            }
            Self::ConflictingFormats { current, next } => {
                write!(
                    f,
                    "conflicting output formats {:?} and {:?}",
                    current.as_str(),
                    next.as_str()
                )
            }
        }
    }
}

impl Error for OutputError {}

/// Removes global output flags (`--output`, `-output`, `--json`, `-json`) from `args`
/// and resolves the requested `OutputFormat`.
///
/// Output flags may appear at any position within `args`.
///
/// # Errors
///
/// Returns an `OutputError` if a flag is missing its value, if an unsupported format is
/// specified, or if conflicting formats (e.g. `--json` and `--output table`) are given.
pub fn extract_format(args: &[OsString]) -> Result<(OutputFormat, Vec<OsString>), OutputError> {
    let mut explicit: Option<OutputFormat> = None;
    let mut remaining = Vec::with_capacity(args.len());
    let mut i = 0;

    while i < args.len() {
        let arg = args[i].to_string_lossy();
        let next = match arg.as_ref() {
            "--json" | "-json" => OutputFormat::Json,
            "--output" | "-output" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    return Err(OutputError::MissingValue("--output".to_string()));
                };
                OutputFormat::parse(&value.to_string_lossy())?
            }
            _ if arg.starts_with("--output=") => OutputFormat::parse(&arg[9..])?,
            _ if arg.starts_with("-output=") => OutputFormat::parse(&arg[8..])?,
            _ => {
                remaining.push(args[i].clone());
                i += 1;
                continue;
            }
        };

        if let Some(current) = explicit
            && current != next
        {
            return Err(OutputError::ConflictingFormats { current, next });
        }
        explicit = Some(next);
        i += 1;
    }

    Ok((explicit.unwrap_or(OutputFormat::Table), remaining))
}

/// Encodes `value` as compact single-line JSON followed by a trailing newline.
///
/// # Errors
///
/// Returns an `io::Error` if serialization or writing fails.
pub fn render_json<W: Write, T: Serialize + ?Sized>(mut writer: W, value: &T) -> io::Result<()> {
    serde_json::to_writer(&mut writer, value).map_err(io::Error::other)?;
    writeln!(writer)?;
    Ok(())
}

/// Renders output according to `format`. For `Json`, `json_value` is serialized.
/// For `Table`, `table_renderer` is invoked.
///
/// # Errors
///
/// Returns an `io::Error` if rendering fails.
pub fn render<W: Write, T: Serialize + ?Sized, F>(
    mut writer: W,
    format: OutputFormat,
    json_value: &T,
    table_renderer: F,
) -> io::Result<()>
where
    F: FnOnce(&mut W) -> io::Result<()>,
{
    match format {
        OutputFormat::Json => render_json(writer, json_value),
        OutputFormat::Table => table_renderer(&mut writer),
    }
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
