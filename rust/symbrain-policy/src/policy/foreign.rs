//! Foreign-server exposure model and read/write classification.

use std::collections::{BTreeMap, BTreeSet};

use crate::constants::{
    EXPOSURE_SOURCE_DEFAULT_WRITE, EXPOSURE_SOURCE_READ_ONLY_HINT, EXPOSURE_SOURCE_TOOLS_READ,
    EXPOSURE_SOURCE_TOOLS_WRITE, FOREIGN_ACCESS_READ, FOREIGN_ACCESS_WRITE,
};
use crate::error::PolicyError;
use crate::policy::{ForeignTool, Report, ToolExposure};
use crate::profile::ServerConfig;

/// Computes the exposure [`Report`] for a foreign (non-core) server.
///
/// Foreign servers operate under a filter model rather than a closed-universe preset.
///
/// # Errors
///
/// Returns [`PolicyError::InvalidAccessClass`] if `cfg.access` is neither "read" nor "write".
pub fn evaluate_foreign(
    alias: &str,
    cfg: &ServerConfig,
    tools: &[ForeignTool],
) -> Result<Report, PolicyError> {
    let access = if cfg.access.is_empty() {
        FOREIGN_ACCESS_WRITE
    } else {
        cfg.access.as_str()
    };

    match access {
        FOREIGN_ACCESS_READ | FOREIGN_ACCESS_WRITE => {}
        _ => {
            return Err(PolicyError::InvalidAccessClass {
                server: alias.to_string(),
                access: access.to_string(),
            });
        }
    }

    let mut exposures = BTreeMap::new();

    if !cfg.enabled {
        let mut hidden = Vec::with_capacity(tools.len());
        for t in tools {
            hidden.push(t.name.clone());
            exposures.insert(t.name.clone(), classify_foreign_tool(cfg, t));
        }
        hidden.sort();
        return Ok(Report {
            server: alias.to_string(),
            enabled: false,
            mode: String::new(),
            exposed: Vec::new(),
            hidden,
            unknown: Vec::new(),
            exposures: Some(exposures),
        });
    }

    let base: BTreeSet<&str> = if cfg.tools_allow.is_empty() {
        tools.iter().map(|t| t.name.as_str()).collect()
    } else {
        cfg.tools_allow.iter().map(String::as_str).collect()
    };

    let deny: BTreeSet<&str> = cfg.tools_deny.iter().map(String::as_str).collect();

    let mut exposed = Vec::new();
    let mut hidden_set = BTreeSet::new();

    for t in tools {
        let exposure = classify_foreign_tool(cfg, t);
        exposures.insert(t.name.clone(), exposure.clone());

        if access == FOREIGN_ACCESS_READ && exposure.class != FOREIGN_ACCESS_READ {
            hidden_set.insert(t.name.clone());
            continue;
        }

        if !base.contains(t.name.as_str()) || deny.contains(t.name.as_str()) {
            hidden_set.insert(t.name.clone());
            continue;
        }

        exposed.push(t.name.clone());
    }

    let mut hidden: Vec<String> = hidden_set.into_iter().collect();
    exposed.sort();
    hidden.sort();

    Ok(Report {
        server: alias.to_string(),
        enabled: true,
        mode: String::new(),
        exposed,
        hidden,
        unknown: Vec::new(),
        exposures: Some(exposures),
    })
}

/// Resolves a single foreign tool's read/write exposure class by precedence:
/// 1. Explicit `tools_read` match -> "read" (`ExposureSourceToolsRead`).
/// 2. Explicit `tools_write` match -> "write" (`ExposureSourceToolsWrite`).
/// 3. Upstream `readOnlyHint: true` -> "read" (`ExposureSourceReadOnlyHint`).
/// 4. Default -> "write" (`ExposureSourceDefaultWrite`).
#[must_use]
pub fn classify_foreign_tool(cfg: &ServerConfig, t: &ForeignTool) -> ToolExposure {
    for name in &cfg.tools_read {
        if name == &t.name {
            return ToolExposure {
                class: FOREIGN_ACCESS_READ.to_string(),
                source: EXPOSURE_SOURCE_TOOLS_READ.to_string(),
            };
        }
    }

    for name in &cfg.tools_write {
        if name == &t.name {
            return ToolExposure {
                class: FOREIGN_ACCESS_WRITE.to_string(),
                source: EXPOSURE_SOURCE_TOOLS_WRITE.to_string(),
            };
        }
    }

    if t.effective_read_only_hint() == Some(true) {
        return ToolExposure {
            class: FOREIGN_ACCESS_READ.to_string(),
            source: EXPOSURE_SOURCE_READ_ONLY_HINT.to_string(),
        };
    }

    ToolExposure {
        class: FOREIGN_ACCESS_WRITE.to_string(),
        source: EXPOSURE_SOURCE_DEFAULT_WRITE.to_string(),
    }
}
