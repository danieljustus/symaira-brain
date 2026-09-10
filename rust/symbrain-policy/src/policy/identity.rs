//! Identity parameter mappings for profile-aware MCP servers.

use crate::constants::SERVER_MEMORY;

/// Returns the mapped profile identity parameter name for `alias`, if known.
///
/// Symaira Memory maps profile identity to `client_id`.
#[must_use]
pub fn identity_parameter(alias: &str) -> Option<&'static str> {
    match alias {
        SERVER_MEMORY => Some("client_id"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{SERVER_SKILLS, SERVER_VAULT};

    #[test]
    fn identity_parameter_matrix() {
        assert_eq!(identity_parameter(SERVER_MEMORY), Some("client_id"));
        assert_eq!(identity_parameter(SERVER_VAULT), None);
        assert_eq!(identity_parameter(SERVER_SKILLS), None);
        assert_eq!(identity_parameter("some_other_server"), None);
        assert_eq!(identity_parameter(""), None);
    }
}
