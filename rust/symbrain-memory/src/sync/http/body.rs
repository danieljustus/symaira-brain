//! Stop at the first JSON value rather than waiting for a trailing body/EOF.

use super::super::{SyncError, json};
use std::io::Read;

pub(in crate::sync) fn first_json(
    reader: &mut impl Read,
    limit: usize,
) -> Result<Vec<u8>, SyncError> {
    let mut bytes = Vec::new();
    let mut eof = false;
    loop {
        match json::first_range(&bytes) {
            Ok(range) => {
                let container = bytes
                    .get(range.start)
                    .is_some_and(|c| matches!(c, b'{' | b'['));
                // Decoder needs a next byte (not necessarily space)/EOF for scalars;
                // object
                // and array closers finish immediately without reading more.
                if container || range.end < bytes.len() || eof || bytes.len() == limit {
                    return Ok(bytes[range].to_vec());
                }
            }
            Err(error) if matches!(error.0.as_str(), "unexpected EOF" | "EOF") => {
                if eof || bytes.len() == limit {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
        let available = (limit - bytes.len()).min(8192);
        if available == 0 {
            return Err(SyncError("unexpected EOF".into()));
        }
        let mut chunk = [0; 8192];
        let count = reader
            .read(&mut chunk[..available])
            .map_err(|error| SyncError(error.to_string()))?;
        eof = count == 0;
        bytes.extend_from_slice(&chunk[..count]);
    }
}
