pub(crate) const BUILTINS: [&str; 3] = ["memory", "skills", "usage"];

use serde::Serialize;
use symbrain_audit::Degradation;

#[derive(Debug, Serialize)]
pub(crate) struct DirCheck {
    pub(crate) path: String,
    pub(crate) exists: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ConfigCheck {
    pub(crate) path: String,
    pub(crate) exists: bool,
    pub(crate) parsed: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) error: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ServerCheck {
    pub(crate) name: String,
    pub(crate) binary: String,
    pub(crate) found: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) path: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) version: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) managed_version: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) origin: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) probe_error: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) install_hint: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ManagedCoreCheck {
    pub(crate) name: String,
    pub(crate) pinned: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) version: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct MemoryDbCheck {
    pub(crate) path: String,
    pub(crate) exists: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) mode: String,
    pub(crate) mode_ok: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) quick_check: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) legacy: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) error: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SkillsLibraryCheck {
    pub(crate) path: String,
    pub(crate) exists: bool,
    pub(crate) count: usize,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) legacy: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) error: String,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)] // Existing JSON schema exposes independent check facts.
pub(crate) struct HarnessCheck {
    pub(crate) name: String,
    pub(crate) config_path: String,
    pub(crate) config_found: bool,
    pub(crate) config_parsed: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) config_error: String,
    pub(crate) supports_mcp_install: bool,
    pub(crate) installed: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) profile: String,
    pub(crate) profile_exists: bool,
    pub(crate) profile_missing: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) superseded: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProfileHandshake {
    pub(crate) profile: String,
    pub(crate) server: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) protocol_version: String,
    pub(crate) tool_count: usize,
    pub(crate) exposed: usize,
    pub(crate) hidden: usize,
    pub(crate) unknown: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) error: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LinkCheck {
    pub(crate) name: String,
    pub(crate) status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) detail: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) remedy: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ForeignAccessRisk {
    pub(crate) profile: String,
    pub(crate) server: String,
    pub(crate) detail: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DoctorReport {
    pub(crate) config_dir: DirCheck,
    pub(crate) data_dir: DirCheck,
    pub(crate) cache_dir: DirCheck,
    pub(crate) config: ConfigCheck,
    pub(crate) managed_dir: DirCheck,
    pub(crate) builtins: Vec<String>,
    pub(crate) servers: Vec<ServerCheck>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) managed_cores: Vec<ManagedCoreCheck>,
    pub(crate) memory_db: MemoryDbCheck,
    pub(crate) skills_library: SkillsLibraryCheck,
    pub(crate) profiles: Vec<String>,
    pub(crate) harnesses: Vec<HarnessCheck>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) handshakes: Vec<ProfileHandshake>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) links: Vec<LinkCheck>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) foreign_access_risks: Vec<ForeignAccessRisk>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) degradations: Vec<Degradation>,
}

#[derive(Clone, Copy)]
pub(crate) struct HarnessSpec {
    pub(crate) name: &'static str,
    pub(crate) format: &'static str,
    pub(crate) servers_key: &'static str,
    pub(crate) supports_mcp_install: bool,
}

pub(crate) const HARNESSES: [HarnessSpec; 9] = [
    HarnessSpec {
        name: "claude",
        format: "json",
        servers_key: "mcpServers",
        supports_mcp_install: true,
    },
    HarnessSpec {
        name: "claude-desktop",
        format: "json",
        servers_key: "mcpServers",
        supports_mcp_install: true,
    },
    HarnessSpec {
        name: "cursor",
        format: "json",
        servers_key: "mcpServers",
        supports_mcp_install: true,
    },
    HarnessSpec {
        name: "opencode",
        format: "json",
        servers_key: "mcpServers",
        supports_mcp_install: true,
    },
    HarnessSpec {
        name: "codex",
        format: "toml",
        servers_key: "mcp_servers",
        supports_mcp_install: true,
    },
    HarnessSpec {
        name: "antigravity",
        format: "json",
        servers_key: "mcpServers",
        supports_mcp_install: true,
    },
    HarnessSpec {
        name: "agents",
        format: "unsupported",
        servers_key: "",
        supports_mcp_install: false,
    },
    HarnessSpec {
        name: "hermes",
        format: "unsupported",
        servers_key: "",
        supports_mcp_install: false,
    },
    HarnessSpec {
        name: "openclaw",
        format: "unsupported",
        servers_key: "",
        supports_mcp_install: false,
    },
];
