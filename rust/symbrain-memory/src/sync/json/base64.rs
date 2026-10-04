//! Go StdEncoding's original-byte error offsets, including ignored CR/LF.
// Adapted from Go1.26.7 encoding/base64 decodeQuantum (Go Authors, BSD-3-Clause).
// License: scripts/memory-sync-oracle/reference/GO-SDK-LICENSE.

use super::super::SyncError;
pub(super) fn check(bytes: &[u8]) -> Result<(), SyncError> {
    let mut at = 0;
    loop {
        let mut digit = 0;
        while digit < 4 {
            if at == bytes.len() {
                return if digit == 0 {
                    Ok(())
                } else {
                    Err(error(at - digit))
                };
            }
            let byte = bytes[at];
            at += 1;
            if byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/') {
                digit += 1;
                continue;
            }
            if matches!(byte, b'\r' | b'\n') {
                continue;
            }
            if byte != b'=' {
                return Err(error(at - 1));
            }
            if digit < 2 {
                return Err(error(at - 1));
            }
            if digit == 2 {
                while at < bytes.len() && matches!(bytes[at], b'\r' | b'\n') {
                    at += 1;
                }
                if at == bytes.len() {
                    return Err(error(bytes.len()));
                }
                if bytes[at] != b'=' {
                    return Err(error(at - 1));
                }
                at += 1;
            }
            while at < bytes.len() && matches!(bytes[at], b'\r' | b'\n') {
                at += 1;
            }
            return if at < bytes.len() {
                Err(error(at))
            } else {
                Ok(())
            };
        }
    }
}
fn error(at: usize) -> SyncError {
    SyncError(format!("illegal base64 data at input byte {at}"))
}
