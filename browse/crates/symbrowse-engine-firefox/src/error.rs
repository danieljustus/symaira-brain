use std::time::Duration;

#[derive(Debug)]
pub enum FirefoxError {
    Unsupported {
        operation: String,
    },
    Timeout {
        operation: String,
        timeout: Duration,
    },
    Driver(String),
    InvalidTarget(String),
}

impl std::fmt::Display for FirefoxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported { operation } => write!(f, "unsupported: {operation}"),
            Self::Timeout { operation, timeout } => {
                write!(f, "{operation} timed out after {timeout:?}")
            }
            Self::Driver(s) | Self::InvalidTarget(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for FirefoxError {}
