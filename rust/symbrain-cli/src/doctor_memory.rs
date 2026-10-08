//! Read-only memory health checks; inspection never invokes store repair.

use super::super::doctor_types::MemoryDbCheck;
use super::component_location;
use std::fs;

pub(in crate::doctor_cli) fn check_memory_db() -> MemoryDbCheck {
    let Some(data) = component_location("memory", "symmemory", ".local/share") else {
        return MemoryDbCheck {
            path: String::new(),
            exists: false,
            mode: String::new(),
            mode_ok: false,
            quick_check: String::new(),
            legacy: false,
            error: "resolve memory data directory".to_string(),
        };
    };
    let path = data.path.join("default.db");
    let mut check = MemoryDbCheck {
        path: path.display().to_string(),
        exists: false,
        mode: String::new(),
        mode_ok: false,
        quick_check: String::new(),
        legacy: data.legacy,
        error: String::new(),
    };
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return check,
        Err(error) => {
            check.error = format!("stat database: {error}");
            return check;
        }
    };
    check.exists = true;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode() & 0o777;
        check.mode = format!("{mode:04o}");
        check.mode_ok = mode == 0o600;
    }
    #[cfg(not(unix))]
    {
        // Go's Windows FileMode exposes the readonly attribute as 0444/0666,
        // not POSIX owner permissions. Neither value proves a private ACL.
        let mut mode = if metadata.permissions().readonly() {
            0o444
        } else {
            0o666
        };
        if metadata.is_dir() {
            mode |= 0o111;
        }
        check.mode = format!("{mode:04o}");
        check.mode_ok = false;
    }
    match rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(connection) => {
            let result =
                connection.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0));
            match result {
                Ok(value) => {
                    check.quick_check = value;
                    if check.quick_check == "ok" {
                        match symbrain_memory::missing_required_columns(&connection) {
                            Ok(missing) if !missing.is_empty() => {
                                check.error =
                                    format!("missing required columns: {}", missing.join(", "));
                            }
                            Ok(_) => {}
                            Err(error) => check.error = format!("inspect database schema: {error}"),
                        }
                    } else {
                        check.error = format!("quick_check: {}", check.quick_check);
                    }
                }
                Err(error) => check.error = format!("quick_check: {error}"),
            }
        }
        Err(error) => check.error = format!("open database: {error}"),
    }
    check
}
