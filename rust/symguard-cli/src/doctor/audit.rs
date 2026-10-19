//! Guard doctor audit-anchor inspection.
use crate::guard_scan::io_detail;
use std::path::{Path, PathBuf};
use std::{fs, io};
use symbrain_core::GoText;

#[path = "anchor_decode.rs"]
mod decoder;

pub(super) fn audit_status(log_path: &Path) -> (GoText, bool) {
    match fs::metadata(log_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return (
                "not initialized (created on first 'symguard decide')".into(),
                false,
            );
        }
        // Go prints `error: <the os.Stat error>`; the detail is the platform text.
        Err(error) => {
            return (
                GoText::path("error: stat ", log_path, ": ")
                    .with_suffix(io_detail(&error).as_bytes()),
                true,
            );
        }
        Ok(_) => {}
    }
    // audit.DefaultAnchorPath(logPath) == logPath + ".anchor"
    let mut anchor_path = log_path.as_os_str().to_owned();
    anchor_path.push(".anchor");
    let anchor_path = PathBuf::from(anchor_path);
    let data = match fs::read(&anchor_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return (
                "ok (JSONL, chain anchor pending Phase 3 sink)".into(),
                false,
            );
        }
        Err(error) => {
            let detail = format!("open {}: {}", anchor_path.display(), io_detail(&error));
            return (
                GoText::path("error: anchor ", &anchor_path, ": ").with_suffix(detail.as_bytes()),
                true,
            );
        }
        Ok(data) => data,
    };
    // Syntax errors come from the shared Go-compatible scanner. Inputs the
    // typed decoder cannot classify like Go get one fixed message (#770).
    let invalid = symbrain_guard_core::external_decision::validate_go_json_syntax(&data)
        .err()
        .or_else(|| {
            decoder::validate(&data)
                .unwrap_or_else(|| Err("json: unsupported string encoding in anchor".into()))
                .err()
        });
    match invalid {
        Some(error) => (
            GoText::path("error: anchor ", &anchor_path, ": auditkit: parse anchor: ")
                .with_suffix(error.as_bytes()),
            true,
        ),
        None => ("ok (hash-chained, anchor present)".into(), false),
    }
}
