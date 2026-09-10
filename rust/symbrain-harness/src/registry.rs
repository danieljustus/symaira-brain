use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{HarnessError, relative_path, resolve_path, trusted_root};

/// Canonical server key written into harness configurations.
pub const SERVER_NAME: &str = "symbrain";

/// Stable identifier for a known harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessName {
    /// Claude Code.
    Claude,
    /// Claude Desktop.
    ClaudeDesktop,
    /// Cursor.
    Cursor,
    /// `OpenCode`.
    Opencode,
    /// Codex CLI.
    Codex,
    /// Antigravity.
    Antigravity,
    /// Generic `AGENTS.md` instruction target.
    Agents,
    /// Hermes skill target.
    Hermes,
    #[serde(rename = "openclaw")]
    /// `OpenClaw` skill target.
    OpenClaw,
}

/// Backward-compatible alias for [`HarnessName`].
pub type Name = HarnessName;

impl HarnessName {
    #[must_use]
    /// Returns the stable command-line name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::ClaudeDesktop => "claude-desktop",
            Self::Cursor => "cursor",
            Self::Opencode => "opencode",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::Agents => "agents",
            Self::Hermes => "hermes",
            Self::OpenClaw => "openclaw",
        }
    }
}

impl fmt::Display for HarnessName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Configuration serialization format used by a harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Ordered JSON.
    Json,
    /// TOML.
    Toml,
    /// No MCP configuration file is supported.
    Unsupported,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Unsupported => "unsupported",
        })
    }
}

/// Instruction-file adapter associated with a harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstructionAdapter {
    /// No instruction adapter.
    None,
    /// Generic `AGENTS.md` adapter.
    Agents,
    /// Claude instruction adapter.
    Claude,
    /// Cursor instruction adapter.
    Cursor,
    /// Antigravity instruction adapter.
    Antigravity,
}

/// Skills installation target associated with a harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillTarget {
    /// No skills target.
    None,
    /// `OpenCode` target.
    OpenCode,
    /// Claude target.
    Claude,
    /// Codex target.
    Codex,
    /// Hermes target.
    Hermes,
    /// Antigravity target.
    Antigravity,
    /// `OpenClaw` target.
    OpenClaw,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PathKind {
    Home(&'static [&'static str]),
    Xdg(&'static [&'static str]),
    ClaudeDesktop,
    Unsupported,
}

#[derive(Debug, Clone)]
/// A resolved configuration path plus the trusted root and relative capability path used to access it.
pub struct ConfigLocation {
    /// User-visible absolute or environment-derived path.
    pub path: PathBuf,
    /// Directory treated as the capability boundary.
    pub trusted_root: PathBuf,
    /// Target path relative to [`Self::trusted_root`].
    pub relative_path: PathBuf,
}

/// Static capabilities and path rules for one harness.
#[derive(Debug, Clone, Copy)]
pub struct Harness {
    /// Stable name.
    pub name: HarnessName,
    /// Human-readable name.
    pub display_name: &'static str,
    /// Configuration format.
    pub format: Format,
    /// Root key containing MCP servers, when supported.
    pub servers_key: Option<&'static str>,
    path_kind: PathKind,
    /// Whether MCP install/uninstall is supported.
    pub supports_mcp_install: bool,
    /// Instruction adapter.
    pub instruction_adapter: InstructionAdapter,
    /// Skills target.
    pub skill_target: SkillTarget,
    /// Whether project-local MCP configuration is supported.
    pub supports_project: bool,
}

impl Harness {
    /// Resolves this harness's config path from the process environment.
    ///
    /// # Errors
    /// Returns an error for unsupported harnesses or when the home directory is unavailable.
    pub fn config_path(&self) -> Result<PathBuf, HarnessError> {
        let env = std::env::vars().collect::<Vec<_>>();
        self.config_path_for(std::env::consts::OS, &env)
    }

    /// Resolves a path using an injected target OS and environment.
    ///
    /// # Errors
    /// Returns an error for unsupported harnesses or when the required home directory is absent.
    pub fn config_path_for(
        &self,
        target_os: &str,
        env: &[(String, String)],
    ) -> Result<PathBuf, HarnessError> {
        resolve_path(self.path_kind, target_os, env)
    }

    /// Resolves the configuration as a trusted-root capability target.
    ///
    /// The returned relative path is the only path opened beneath the trusted
    /// root; parent components and the final file are opened without following
    /// symlinks.
    ///
    /// # Errors
    /// Returns an error for unsupported harnesses or when the required home directory is absent.
    pub fn config_location(&self) -> Result<ConfigLocation, HarnessError> {
        let env = std::env::vars().collect::<Vec<_>>();
        self.config_location_for(std::env::consts::OS, &env)
    }

    /// Resolves a capability target using an injected target OS and environment.
    ///
    /// # Errors
    /// Returns an error for unsupported harnesses or when the required home directory is absent.
    pub fn config_location_for(
        &self,
        target_os: &str,
        env: &[(String, String)],
    ) -> Result<ConfigLocation, HarnessError> {
        Ok(ConfigLocation {
            path: self.config_path_for(target_os, env)?,
            trusted_root: trusted_root(self.path_kind, target_os, env)?,
            relative_path: relative_path(self.path_kind, target_os),
        })
    }

    #[must_use]
    /// Resolves the project-local configuration path when supported.
    pub fn project_config_path(&self, project_dir: &Path) -> Option<PathBuf> {
        self.supports_project.then(|| project_dir.join(".mcp.json"))
    }

    #[must_use]
    /// Returns the stable harness name.
    pub const fn name(&self) -> HarnessName {
        self.name
    }

    #[must_use]
    /// Returns the human-readable harness name.
    pub const fn display_name(&self) -> &'static str {
        self.display_name
    }

    #[must_use]
    /// Returns the harness configuration format.
    pub const fn format(&self) -> Format {
        self.format
    }

    #[must_use]
    /// Returns the MCP server-map key when supported.
    pub const fn server_key(&self) -> Option<&'static str> {
        self.servers_key
    }

    #[must_use]
    /// Reports whether MCP install/uninstall is supported.
    pub const fn supports_mcp_install(&self) -> bool {
        self.supports_mcp_install
    }
}

const HOME_CLAUDE: &[&str] = &[".claude.json"];
const HOME_CURSOR: &[&str] = &[".cursor", "mcp.json"];
const HOME_CODEX: &[&str] = &[".codex", "config.toml"];
const HOME_ANTIGRAVITY: &[&str] = &[".gemini", "config", "mcp_config.json"];
const XDG_OPENCODE: &[&str] = &["opencode", "config.json"];

static REGISTRY: [Harness; 9] = [
    Harness {
        name: HarnessName::Claude,
        display_name: "Claude Code",
        format: Format::Json,
        servers_key: Some("mcpServers"),
        path_kind: PathKind::Home(HOME_CLAUDE),
        supports_mcp_install: true,
        instruction_adapter: InstructionAdapter::Claude,
        skill_target: SkillTarget::Claude,
        supports_project: true,
    },
    Harness {
        name: HarnessName::ClaudeDesktop,
        display_name: "Claude Desktop",
        format: Format::Json,
        servers_key: Some("mcpServers"),
        path_kind: PathKind::ClaudeDesktop,
        supports_mcp_install: true,
        instruction_adapter: InstructionAdapter::None,
        skill_target: SkillTarget::None,
        supports_project: false,
    },
    Harness {
        name: HarnessName::Cursor,
        display_name: "Cursor",
        format: Format::Json,
        servers_key: Some("mcpServers"),
        path_kind: PathKind::Home(HOME_CURSOR),
        supports_mcp_install: true,
        instruction_adapter: InstructionAdapter::Cursor,
        skill_target: SkillTarget::None,
        supports_project: false,
    },
    Harness {
        name: HarnessName::Opencode,
        display_name: "OpenCode",
        format: Format::Json,
        servers_key: Some("mcpServers"),
        path_kind: PathKind::Xdg(XDG_OPENCODE),
        supports_mcp_install: true,
        instruction_adapter: InstructionAdapter::None,
        skill_target: SkillTarget::OpenCode,
        supports_project: false,
    },
    Harness {
        name: HarnessName::Codex,
        display_name: "Codex CLI",
        format: Format::Toml,
        servers_key: Some("mcp_servers"),
        path_kind: PathKind::Home(HOME_CODEX),
        supports_mcp_install: true,
        instruction_adapter: InstructionAdapter::None,
        skill_target: SkillTarget::Codex,
        supports_project: false,
    },
    Harness {
        name: HarnessName::Antigravity,
        display_name: "Antigravity",
        format: Format::Json,
        servers_key: Some("mcpServers"),
        path_kind: PathKind::Home(HOME_ANTIGRAVITY),
        supports_mcp_install: true,
        instruction_adapter: InstructionAdapter::Antigravity,
        skill_target: SkillTarget::Antigravity,
        supports_project: false,
    },
    Harness {
        name: HarnessName::Agents,
        display_name: "AGENTS.md",
        format: Format::Unsupported,
        servers_key: None,
        path_kind: PathKind::Unsupported,
        supports_mcp_install: false,
        instruction_adapter: InstructionAdapter::Agents,
        skill_target: SkillTarget::None,
        supports_project: false,
    },
    Harness {
        name: HarnessName::Hermes,
        display_name: "Hermes",
        format: Format::Unsupported,
        servers_key: None,
        path_kind: PathKind::Unsupported,
        supports_mcp_install: false,
        instruction_adapter: InstructionAdapter::None,
        skill_target: SkillTarget::Hermes,
        supports_project: false,
    },
    Harness {
        name: HarnessName::OpenClaw,
        display_name: "OpenClaw",
        format: Format::Unsupported,
        servers_key: None,
        path_kind: PathKind::Unsupported,
        supports_mcp_install: false,
        instruction_adapter: InstructionAdapter::None,
        skill_target: SkillTarget::OpenClaw,
        supports_project: false,
    },
];

#[must_use]
/// Returns all harnesses in canonical registry order.
pub fn all() -> &'static [Harness] {
    &REGISTRY
}

/// Looks up one of the nine registered harnesses.
///
/// # Errors
/// Returns `HarnessError::UnknownHarness` for an unregistered name.
pub fn lookup(name: &str) -> Result<&'static Harness, HarnessError> {
    REGISTRY
        .iter()
        .find(|h| h.name.as_str() == name)
        .ok_or_else(|| HarnessError::UnknownHarness(name.to_owned()))
}
#[must_use]
/// Returns all stable harness names in lexical order.
pub fn names() -> Vec<String> {
    let mut result = REGISTRY
        .iter()
        .map(|h| h.name.to_string())
        .collect::<Vec<_>>();
    result.sort();
    result
}
