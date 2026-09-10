//! Version information payload and platform mapping for Symaira tools.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::env;
use std::fmt;
use std::io::{self, Write};

/// Contract schema version for the `version --json` payload.
pub const VERSION_SCHEMA: u8 = 1;

/// Standardized `version --json` payload used across Symaira CLI tools and GUI clients.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VersionInfo<'a> {
    /// Name of the CLI tool (e.g., "symbrain").
    pub tool: Cow<'a, str>,
    /// Version string (e.g., "0.11.0" or "dev").
    pub version: Cow<'a, str>,
    /// Contract schema version for the JSON shape.
    pub schema_version: u8,
}

impl<'a> VersionInfo<'a> {
    /// Creates a new `VersionInfo` payload with the default `VERSION_SCHEMA`.
    #[must_use]
    pub fn new(tool: impl Into<Cow<'a, str>>, version: impl Into<Cow<'a, str>>) -> Self {
        Self {
            tool: tool.into(),
            version: version.into(),
            schema_version: VERSION_SCHEMA,
        }
    }

    /// Creates a new `VersionInfo` payload with a custom schema version.
    #[must_use]
    pub fn with_schema(
        tool: impl Into<Cow<'a, str>>,
        version: impl Into<Cow<'a, str>>,
        schema_version: u8,
    ) -> Self {
        Self {
            tool: tool.into(),
            version: version.into(),
            schema_version,
        }
    }

    /// Serializes this version payload as a compact JSON string.
    ///
    /// # Errors
    ///
    /// Returns a `serde_json::Error` if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Writes this version payload as compact JSON followed by a newline.
    ///
    /// # Errors
    ///
    /// Returns an `io::Error` if serialization or writing fails.
    pub fn write_json<W: Write>(&self, writer: W) -> io::Result<()> {
        crate::output::render_json(writer, self)
    }
}

impl fmt::Display for VersionInfo<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.tool, self.version)
    }
}

/// Maps operating system identifiers to standard Go/Symaira OS names
/// (e.g. "macos" -> "darwin").
#[must_use]
pub fn map_os(os: &str) -> &str {
    match os {
        "macos" => "darwin",
        other => other,
    }
}

/// Maps architecture identifiers to standard Go/Symaira arch names
/// (e.g. `x86_64` -> "amd64", `aarch64` -> "arm64").
#[must_use]
pub fn map_arch(arch: &str) -> &str {
    match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

/// Returns the current runtime's OS in Go-compatible convention.
#[must_use]
pub fn current_os() -> &'static str {
    map_os(env::consts::OS)
}

/// Returns the current runtime's CPU architecture in Go-compatible convention.
#[must_use]
pub fn current_arch() -> &'static str {
    map_arch(env::consts::ARCH)
}

/// Returns the current runtime's platform identifier in `os/arch` format.
#[must_use]
pub fn current_platform() -> String {
    platform_pair(current_os(), current_arch())
}

/// Formats an `os` and `arch` pair into standard `os/arch` string with mapping applied.
#[must_use]
pub fn platform_pair(os: &str, arch: &str) -> String {
    format!("{}/{}", map_os(os), map_arch(arch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_info_sets_correct_fields() {
        let info = VersionInfo::new("symbrain", "dev");
        assert_eq!(info.tool, "symbrain");
        assert_eq!(info.version, "dev");
        assert_eq!(info.schema_version, VERSION_SCHEMA);
        assert_eq!(info.schema_version, 1);
        assert_eq!(info.to_string(), "symbrain dev");
    }

    #[test]
    fn version_info_with_schema() {
        let info = VersionInfo::with_schema("custom", "1.0.0", 2);
        assert_eq!(info.tool, "custom");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.schema_version, 2);
    }

    #[test]
    fn version_info_json_serialization_matches_schema() {
        let info = VersionInfo::new("symbrain", "0.11.0");
        let json_str = info.to_json().unwrap();
        assert_eq!(
            json_str,
            "{\"tool\":\"symbrain\",\"version\":\"0.11.0\",\"schema_version\":1}"
        );

        let mut buf = Vec::new();
        info.write_json(&mut buf).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "{\"tool\":\"symbrain\",\"version\":\"0.11.0\",\"schema_version\":1}\n"
        );
    }

    #[test]
    fn version_info_json_roundtrip() {
        let info = VersionInfo::new("symbrain", "1.2.3");
        let json_str = info.to_json().unwrap();
        let decoded: VersionInfo<'_> = serde_json::from_str(&json_str).unwrap();
        assert_eq!(info, decoded);
    }

    #[test]
    fn platform_mapping_os() {
        assert_eq!(map_os("macos"), "darwin");
        assert_eq!(map_os("linux"), "linux");
        assert_eq!(map_os("windows"), "windows");
        assert_eq!(map_os("freebsd"), "freebsd");
    }

    #[test]
    fn platform_mapping_arch() {
        assert_eq!(map_arch("x86_64"), "amd64");
        assert_eq!(map_arch("aarch64"), "arm64");
        assert_eq!(map_arch("amd64"), "amd64");
        assert_eq!(map_arch("arm64"), "arm64");
        assert_eq!(map_arch("arm"), "arm");
    }

    #[test]
    fn platform_pair_formatting() {
        assert_eq!(platform_pair("macos", "aarch64"), "darwin/arm64");
        assert_eq!(platform_pair("macos", "x86_64"), "darwin/amd64");
        assert_eq!(platform_pair("linux", "x86_64"), "linux/amd64");
        assert_eq!(platform_pair("windows", "x86_64"), "windows/amd64");
    }

    #[test]
    fn current_platform_non_empty() {
        assert!(!current_os().is_empty());
        assert!(!current_arch().is_empty());
        assert!(current_platform().contains('/'));
    }
}
