//! Read-only Hermes inventory for `symbrain harness list`.
//!
//! Hermes is not an install target: its `config.yaml` (and the per-profile
//! `profiles/<name>/config.yaml` files) are only read to show which MCP
//! servers are registered. Nothing here writes to a Hermes directory.

use std::path::{Path, PathBuf};

use symbrain_harness::{
    ConfigInventory, HarnessInventory, HarnessName, MAX_CONFIG_BYTES, ProfileInventory, ServerInfo,
    hermes_locations,
};

/// Key under which Hermes registers MCP servers.
pub(crate) const SERVERS_KEY: &str = "mcp_servers";

/// Hermes inventory entry, or `None` when no home directory is resolvable.
pub(crate) fn inventory() -> Option<HarnessInventory> {
    let locations = hermes_locations()?;
    Some(HarnessInventory {
        name: HarnessName::Hermes,
        display_name: "Hermes".to_owned(),
        global: inspect(&locations.global),
        project: None,
        profiles: locations
            .profiles
            .iter()
            .map(|profile| ProfileInventory {
                name: profile.name.clone(),
                config: inspect(&profile.config),
            })
            .collect(),
        read_only: true,
    })
}

fn inspect(path: &Path) -> ConfigInventory {
    let mut config = ConfigInventory {
        path: path.to_string_lossy().into_owned(),
        raw_path: PathBuf::from(path),
        exists: false,
        parsed: false,
        error: None,
        servers: Vec::new(),
    };
    let bytes = match read_limited(path) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return config,
        Err(message) => {
            config.error = Some(message);
            return config;
        }
    };
    config.exists = true;
    match servers(&bytes) {
        Ok(servers) => {
            config.parsed = true;
            config.servers = servers;
        }
        Err(detail) => {
            config.error = Some(format!(
                "harness: hermes config is not valid yaml; symbrain only reads it: parse yaml: {detail}"
            ));
        }
    }
    config
}

/// `Ok(None)` for a missing file; the read is capped like every harness config.
fn read_limited(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("stat {}: {error}", path.display())),
    };
    if metadata.len() > MAX_CONFIG_BYTES as u64 {
        return Err(format!(
            "read {}: config exceeds {MAX_CONFIG_BYTES} bytes",
            path.display()
        ));
    }
    std::fs::read(path)
        .map(Some)
        .map_err(|error| format!("read {}: {error}", path.display()))
}

fn servers(bytes: &[u8]) -> Result<Vec<ServerInfo>, String> {
    let root: serde_json::Value =
        serde_yaml_ng::from_slice(bytes).map_err(|error| error.to_string())?;
    let Some(map) = root.get(SERVERS_KEY).and_then(|value| value.as_object()) else {
        return Ok(Vec::new());
    };
    let mut names: Vec<&String> = map.keys().collect();
    names.sort();
    Ok(names
        .into_iter()
        .map(|name| ServerInfo::from_value(name, &map[name]))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::servers;

    #[test]
    fn lists_sorted_servers_without_env_values() {
        let yaml = b"mcp_servers:\n  zeta:\n    url: https://x.test/mcp\n  alpha:\n    command: npx\n    args: [-y, pkg]\n    env:\n      TOKEN: secret-value\n";
        let found = servers(yaml).unwrap();
        let names: Vec<_> = found.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["alpha", "zeta"]);
        assert_eq!(found[0].transport, "stdio");
        assert_eq!(found[0].env_names, ["TOKEN"]);
        assert_eq!(found[1].transport, "http");
        assert!(!format!("{found:?}").contains("secret-value"));
    }

    #[test]
    fn empty_or_serverless_config_is_parsed_with_no_servers() {
        assert!(servers(b"").unwrap().is_empty());
        assert!(servers(b"model: x\n").unwrap().is_empty());
        assert!(servers(b"mcp_servers: [\n").is_err());
    }
}
