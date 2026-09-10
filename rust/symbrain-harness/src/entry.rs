/// Legacy core-server commands replaced by the unified gateway.
pub const SUPERSEDED_CORE_NAMES: &[&str] = &["symmemory", "symskills"];

/// A stdio MCP server entry stored in a harness configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Executable name or path.
    pub command: String,
    /// Arguments passed to the executable.
    pub args: Vec<String>,
}

impl Entry {
    #[must_use]
    /// Builds the canonical entry for a named symbrain profile.
    pub fn new(profile: &str) -> Self {
        Self {
            command: super::SERVER_NAME.to_owned(),
            args: vec!["mcp".into(), "--profile".into(), profile.into()],
        }
    }

    #[must_use]
    /// Reports whether the command resolves to the symbrain executable name.
    pub fn is_symbrain(&self) -> bool {
        !self.command.is_empty() && basename(&self.command) == Some(super::SERVER_NAME)
    }

    #[must_use]
    /// Returns the legacy core name when this entry points at one.
    pub fn superseded_core(&self) -> Option<&'static str> {
        let basename = basename(&self.command);
        SUPERSEDED_CORE_NAMES
            .iter()
            .copied()
            .find(|name| Some(*name) == basename)
    }

    #[must_use]
    /// Extracts the profile from `--profile value` or `--profile=value`.
    pub fn profile(&self) -> Option<&str> {
        for (index, arg) in self.args.iter().enumerate() {
            if arg == "--profile" {
                return self.args.get(index + 1).map(String::as_str);
            }
            if let Some(value) = arg.strip_prefix("--profile=") {
                return Some(value);
            }
        }
        None
    }
}

fn basename(command: &str) -> Option<&str> {
    std::path::Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
}
