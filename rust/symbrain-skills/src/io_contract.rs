//! Scoped presentation of actual filesystem errors as Go PathError values.
use std::io;
use std::path::Path;

pub(crate) fn path_error(operation: &str, path: &Path, error: &io::Error) -> String {
    format!("{operation} {}: {}", path.display(), error_text(error))
}
fn error_text(error: &io::Error) -> String {
    let text = error.to_string();
    let Some(code) = error.raw_os_error() else {
        return text;
    };
    let suffix = format!(" (os error {code})");
    let text = text.strip_suffix(&suffix).unwrap_or(&text);
    #[cfg(unix)]
    {
        // Both SDKs use the host's errno descriptions; Go starts them lower-case.
        let mut chars = text.chars();
        chars.next().map_or_else(String::new, |first| {
            first.to_ascii_lowercase().to_string() + chars.as_str()
        })
    }
    #[cfg(not(unix))]
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_read_permission_error_keeps_operation_and_relative_path() {
        #[cfg(unix)]
        let (code, text) = (13, "permission denied");
        #[cfg(windows)]
        let (code, text) = (5, "Access is denied.");
        #[cfg(any(unix, windows))]
        assert_eq!(
            path_error(
                "openat",
                Path::new("SKILL.md"),
                &io::Error::from_raw_os_error(code)
            ),
            format!("openat SKILL.md: {text}")
        );
    }
    #[test]
    fn custom_capability_errors_keep_their_diagnostic() {
        assert_eq!(
            path_error(
                "openat",
                Path::new("SKILL.md"),
                &io::Error::other("owned capability refusal")
            ),
            "openat SKILL.md: owned capability refusal"
        );
    }
}
