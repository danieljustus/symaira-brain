use std::fmt;
use std::io::{self, BufRead, Read, Write};

use serde::Serialize;

use crate::Request;

pub const MAX_MESSAGE_BYTES: usize = 1 << 20;
/// Maximum aggregate bytes in one framed header block.
pub const MAX_HEADER_BYTES: usize = 64 << 10;
/// Maximum number of lines in one framed header block.
pub const MAX_HEADER_LINES: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Framed,
    Line,
}

#[derive(Debug)]
pub enum FrameError {
    Io(io::Error),
    InvalidContentLength(String),
    MissingContentLength,
    HeaderTooLong,
    TooManyHeaders,
    LineTooLong,
    InvalidRequest { mode: Mode },
    Parse { mode: Mode, message: String },
}

impl FrameError {
    #[must_use]
    pub const fn mode(&self) -> Mode {
        match self {
            Self::Parse { mode, .. } | Self::InvalidRequest { mode } => *mode,
            _ => Mode::Framed,
        }
    }

    #[must_use]
    pub const fn is_parse(&self) -> bool {
        matches!(self, Self::Parse { .. })
    }
}

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::InvalidContentLength(value) => {
                write!(formatter, "invalid Content-Length: {value}")
            }
            Self::MissingContentLength => write!(formatter, "missing Content-Length header"),
            Self::HeaderTooLong => {
                write!(formatter, "framed header exceeds {MAX_HEADER_BYTES} bytes")
            }
            Self::TooManyHeaders => {
                write!(formatter, "framed header exceeds {MAX_HEADER_LINES} lines")
            }
            Self::LineTooLong => write!(formatter, "line exceeds {MAX_MESSAGE_BYTES} bytes"),
            Self::InvalidRequest { .. } => formatter.write_str("Invalid Request"),
            Self::Parse { message, .. } => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<io::Error> for FrameError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub struct Decoder<R> {
    reader: R,
}

impl<R: BufRead> Decoder<R> {
    #[must_use]
    pub const fn new(reader: R) -> Self {
        Self { reader }
    }

    /// Reads one newline-delimited or Content-Length-framed JSON-RPC request.
    ///
    /// # Errors
    /// Returns framing, I/O, size-limit, or JSON parse errors.
    pub fn read_request(&mut self) -> Result<Option<(Request, Mode)>, FrameError> {
        let Some(first) = self.read_non_empty_line()? else {
            return Ok(None);
        };
        let trimmed = std::str::from_utf8(&first).map_or_else(
            |_| crate::raw_skills::trim_go_space(&first),
            |text| text.trim().as_bytes(),
        );
        if trimmed.starts_with(b"{") || !trimmed.contains(&b':') {
            if let Some(request) = crate::raw_skills::transport_request(trimmed) {
                return request
                    .map(|request| Some((request, Mode::Line)))
                    .map_err(|message| FrameError::Parse {
                        mode: Mode::Line,
                        message,
                    });
            }
            // Other line owners retain the original whole-line UTF-8 error,
            // including its byte offset before whitespace trimming.
            let line = decode_header(first)?;
            return parse_request(line.trim().as_bytes(), Mode::Line).map(Some);
        }

        let first = decode_header(first)?.trim().to_owned();
        let mut content_length = parse_content_length_header(&first)?;
        let mut header_bytes = first.len();
        let mut header_lines = 1;
        if header_bytes > MAX_HEADER_BYTES {
            return Err(FrameError::HeaderTooLong);
        }
        loop {
            let Some(line) = self.read_line_limited()? else {
                return Err(FrameError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unexpected EOF",
                )));
            };
            header_bytes = header_bytes.saturating_add(line.len());
            header_lines += 1;
            if header_bytes > MAX_HEADER_BYTES {
                return Err(FrameError::HeaderTooLong);
            }
            if header_lines > MAX_HEADER_LINES {
                return Err(FrameError::TooManyHeaders);
            }
            let line = decode_header(line)?;
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                break;
            }
            if let Some(value) = parse_content_length_header(line)? {
                content_length = Some(value);
            }
        }
        let length = content_length.ok_or(FrameError::MissingContentLength)?;
        if length <= 0 || length > 1_i64 << 20 {
            return Err(FrameError::InvalidContentLength(length.to_string()));
        }
        let length = usize::try_from(length)
            .map_err(|_| FrameError::InvalidContentLength(length.to_string()))?;
        let mut body = vec![0; length];
        self.reader.read_exact(&mut body).map_err(|error| {
            let detail = if error.kind() == io::ErrorKind::UnexpectedEof {
                "unexpected EOF".to_string()
            } else {
                error.to_string()
            };
            FrameError::Io(io::Error::new(error.kind(), format!("read body: {detail}")))
        })?;
        parse_request(&body, Mode::Framed).map(Some)
    }

    fn read_non_empty_line(&mut self) -> Result<Option<Vec<u8>>, FrameError> {
        loop {
            let Some(line) = self.read_line_limited()? else {
                return Ok(None);
            };
            let trimmed = std::str::from_utf8(&line).map_or_else(
                |_| line.trim_ascii().to_vec(),
                |text| text.trim().as_bytes().to_vec(),
            );
            if !trimmed.is_empty() {
                return Ok(Some(line));
            }
        }
    }

    fn read_line_limited(&mut self) -> Result<Option<Vec<u8>>, FrameError> {
        let mut bytes = Vec::new();
        let read = self
            .reader
            .by_ref()
            .take((MAX_MESSAGE_BYTES + 1) as u64)
            .read_until(b'\n', &mut bytes)?;
        if read == 0 {
            return Ok(None);
        }
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(FrameError::LineTooLong);
        }
        Ok(Some(bytes))
    }
}

fn decode_header(bytes: Vec<u8>) -> Result<String, FrameError> {
    String::from_utf8(bytes).map_err(|error| FrameError::Parse {
        mode: Mode::Line,
        message: error.to_string(),
    })
}

fn parse_content_length_header(line: &str) -> Result<Option<i64>, FrameError> {
    let Some(value) = line.strip_prefix("Content-Length:") else {
        return Ok(None);
    };
    let value = value.trim();
    let parsed = value
        .parse::<i64>()
        .map_err(|_| FrameError::InvalidContentLength(format!("{value:?}")))?;
    Ok(Some(parsed))
}

fn parse_request(bytes: &[u8], mode: Mode) -> Result<(Request, Mode), FrameError> {
    // RawMessage arguments must reach the Skills decoder before Value rejects
    // ignored large numbers or lone surrogates. Every other route keeps the
    // original recursive Value validation below, including its error mapping.
    if let Some(request) = crate::raw_skills::transport_request(bytes) {
        return request
            .map(|request| (request, mode))
            .map_err(|message| FrameError::Parse { mode, message });
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| FrameError::Parse {
            mode,
            message: error.to_string(),
        })?;
    if !valid_request_envelope(&value) {
        return Err(FrameError::InvalidRequest { mode });
    }
    serde_json::from_slice(bytes)
        .map(|request| (request, mode))
        .map_err(|error| FrameError::Parse {
            mode,
            message: error.to_string(),
        })
}

fn valid_request_envelope(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.get("jsonrpc").and_then(serde_json::Value::as_str) == Some(crate::JSONRPC_VERSION)
        && object
            .get("method")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|method| !method.is_empty())
}

/// Encodes a response using the same transport mode as its request.
///
/// # Errors
/// Returns an error when JSON serialization or writing fails.
pub fn write_message<T: Serialize>(
    writer: &mut dyn Write,
    mode: Mode,
    value: &T,
) -> io::Result<()> {
    let data = serde_json::to_vec(value).map_err(io::Error::other)?;
    match mode {
        Mode::Line => {
            writer.write_all(&data)?;
            writer.write_all(b"\n")?;
            writer.flush()
        }
        Mode::Framed => {
            write!(writer, "Content-Length: {}\r\n\r\n", data.len())?;
            writer.write_all(&data)?;
            writer.flush()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::{Value, json};

    use super::*;
    use crate::Response;

    #[test]
    fn reads_line_and_framed_requests() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        let mut line = Decoder::new(Cursor::new(format!("\n{body}\n")));
        assert_eq!(line.read_request().unwrap().unwrap().1, Mode::Line);
        let mut framed = Decoder::new(Cursor::new(format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )));
        assert_eq!(framed.read_request().unwrap().unwrap().1, Mode::Framed);
    }

    #[test]
    fn raw_skills_are_admitted_in_both_transports_without_widening_other_routes() {
        for arguments in [
            r#"{"name":"\ud800"}"#,
            r#"{"ignored":1e9999}"#,
            r#"{"\ud800":1e9999}"#,
        ] {
            let body = format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"skills_history","arguments":{arguments}}}}}"#
            );
            for mode in [Mode::Line, Mode::Framed] {
                let incoming = if mode == Mode::Line {
                    format!("{body}\n")
                } else {
                    format!("Content-Length: {}\r\n\r\n{body}", body.len())
                };
                let request = Decoder::new(Cursor::new(incoming))
                    .read_request()
                    .unwrap()
                    .unwrap();
                assert_eq!(request.1, mode);
                assert!(request.0.params.unwrap().get().contains(arguments));
            }
            let ordinary = body.replace("skills_history", "memory_get");
            assert!(crate::raw_skills::transport_request(ordinary.as_bytes()).is_none());
            // Workspace dependencies can enable serde_json's arbitrary_precision.
            // Other owners must retain the pre-Skills Value validation in either build.
            let original = serde_json::from_slice::<Value>(ordinary.as_bytes());
            for mode in [Mode::Line, Mode::Framed] {
                let decoded = parse_request(ordinary.as_bytes(), mode);
                match &original {
                    Ok(_) => assert!(decoded.is_ok(), "arguments={arguments}: {decoded:?}"),
                    Err(error) => assert!(
                        matches!(decoded, Err(FrameError::Parse { message, .. })
                            if message == error.to_string()),
                        "arguments={arguments}"
                    ),
                }
            }
        }
        let malformed = br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list","arguments":{}}"#;
        assert!(matches!(
            parse_request(malformed, Mode::Line),
            Err(FrameError::Parse { .. })
        ));
    }

    #[test]
    fn preserves_null_id_as_a_request() {
        let mut decoder = Decoder::new(Cursor::new(
            b"{\"jsonrpc\":\"2.0\",\"id\":null,\"method\":\"ping\"}\n",
        ));
        let request = decoder.read_request().unwrap().unwrap().0;
        assert!(request.has_id);
        assert!(!request.is_notification());
    }

    #[test]
    fn response_mode_is_symmetric() {
        let response = Response::success(Value::from(1), json!({}));
        let mut line = Vec::new();
        write_message(&mut line, Mode::Line, &response).unwrap();
        assert_eq!(line, b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n");
        let mut framed = Vec::new();
        write_message(&mut framed, Mode::Framed, &response).unwrap();
        assert!(framed.starts_with(b"Content-Length: 36\r\n\r\n"));
    }
}
