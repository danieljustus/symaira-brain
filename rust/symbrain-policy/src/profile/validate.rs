//! Validation logic for profile names and boundaries.

use crate::error::ProfileError;

/// Validates whether `name` is safe to use as a profile name.
///
/// Matches Go's `validNamePattern = regexp.MustCompile("^[a-zA-Z0-9_-]+$")`.
/// Profiles are safe filesystem basenames and TOML string values.
///
/// # Errors
///
/// Returns [`ProfileError::InvalidName`] if the name is empty or contains
/// characters other than ASCII letters, digits, hyphen, or underscore.
pub fn validate_name(name: &str) -> Result<(), ProfileError> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ProfileError::InvalidName {
            name: name.to_string(),
            message: format!(
                "profile name \"{name}\" must be non-empty and contain only letters, digits, '-', or '_'"
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_matrix() {
        let valid = ["cursor-arbeit", "personal", "restricted_2"];
        for name in valid {
            assert!(validate_name(name).is_ok(), "expected {name} to be valid");
        }

        let invalid = [
            "",
            "..",
            "../../etc/passwd",
            "has/slash",
            "has\\backslash",
            "has space",
            "has\"quote",
        ];
        for name in invalid {
            assert!(
                validate_name(name).is_err(),
                "expected {name} to be invalid"
            );
        }
    }
}
