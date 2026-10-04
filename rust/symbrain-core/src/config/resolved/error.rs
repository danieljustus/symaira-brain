//! Keep raw filesystem and environment context until the output boundary.
use crate::GoText;
use std::fmt;
use std::io::{self, Write};

#[derive(Debug, Clone)]
pub struct ConfigError(pub(super) GoText);
impl ConfigError {
    #[must_use]
    pub fn text(&self) -> &GoText {
        &self.0
    }
    /// Writes a command's existing prefix while preserving raw Go error bytes.
    ///
    /// # Errors
    /// Returns the writer error from this single Go-style operation.
    pub fn write(&self, prefix: &str, writer: &mut dyn Write) -> io::Result<()> {
        let mut bytes = prefix.as_bytes().to_vec();
        bytes.extend_from_slice(self.0.as_ref());
        bytes.push(b'\n');
        // Like one Go fmt operation: no retry of an unwritten suffix.
        writer.write(&bytes).map(|_| ())
    }
}
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}
impl std::error::Error for ConfigError {}

pub(super) fn cause(error: &io::Error) -> String {
    let literal = error.to_string();
    let Some(code) = error.raw_os_error() else {
        return literal;
    };
    let suffix = format!(" (os error {code})");
    let message = literal.strip_suffix(&suffix).unwrap_or(&literal);
    #[cfg(unix)]
    {
        message.to_lowercase()
    }
    #[cfg(not(unix))]
    {
        message.to_owned()
    }
}
