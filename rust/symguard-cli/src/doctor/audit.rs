//! Guard doctor audit-anchor inspection.
use std::path::{Path, PathBuf};
use std::{fs, io};

#[path = "anchor_decode.rs"]
mod decoder;

pub(super) fn audit_status(log_path: &Path) -> Option<(String, bool)> {
    match fs::metadata(log_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Some((
                "not initialized (created on first 'symguard decide')".to_owned(),
                false,
            ));
        }
        // Go prints `error: <the os.Stat error>` here; not reproducible.
        Err(_) => return None,
        Ok(_) => {}
    }
    // audit.DefaultAnchorPath(logPath) == logPath + ".anchor"
    let mut anchor_path = log_path.as_os_str().to_owned();
    anchor_path.push(".anchor");
    let anchor_path = PathBuf::from(anchor_path);
    let anchor = match fs::read(&anchor_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => return None,
        Ok(data) => Some(data),
    };
    match anchor {
        None => Some((
            "ok (JSONL, chain anchor pending Phase 3 sink)".to_owned(),
            false,
        )),
        Some(data) => {
            // Syntax errors come from the shared Go-compatible scanner.
            if let Err(error) =
                symbrain_guard_core::external_decision::validate_go_json_syntax(&data)
            {
                return Some((
                    format!(
                        "error: anchor {}: auditkit: parse anchor: {error}",
                        anchor_path.display()
                    ),
                    true,
                ));
            }
            if let Err(error) = decoder::validate(&data)? {
                return Some((
                    format!(
                        "error: anchor {}: auditkit: parse anchor: {error}",
                        anchor_path.display()
                    ),
                    true,
                ));
            }
            Some(("ok (hash-chained, anchor present)".to_owned(), false))
        }
    }
}
