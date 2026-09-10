//! Policy evaluation against live tool lists and mode presets.

use std::collections::BTreeSet;

use crate::constants::{SERVER_MEMORY, SERVER_SKILLS, SERVER_USAGE, SERVER_VAULT, VAULT_MODE_OFF};
use crate::error::PolicyError;
use crate::policy::Report;
use crate::policy::presets::{
    ACTIVITY_TOOLS, USAGE_TOOLS, known_tools, preset_for_mode, universe_for,
};
use crate::profile::ServerConfig;

/// Evaluates exposure policy for one core server alias (`vault`, `memory`, `skills`, `usage`)
/// given its configured profile and the live tools reported by the backend child.
///
/// # Errors
///
/// Returns [`PolicyError`] if the server alias is unknown or an unrecognized mode was provided.
pub fn evaluate(
    alias: &str,
    cfg: &ServerConfig,
    live_tools: &[String],
) -> Result<Report, PolicyError> {
    match alias {
        SERVER_VAULT | SERVER_MEMORY | SERVER_SKILLS | SERVER_USAGE => {}
        _ => return Err(PolicyError::UnknownServerAlias(alias.to_string())),
    }

    let is_off = !cfg.enabled || (alias == SERVER_VAULT && cfg.mode == VAULT_MODE_OFF);
    if is_off {
        let (hidden, unknown) = classify(alias, live_tools, &BTreeSet::new());
        return Ok(Report {
            server: alias.to_string(),
            enabled: cfg.enabled,
            mode: cfg.mode.clone(),
            exposed: Vec::new(),
            hidden,
            unknown,
            exposures: None,
        });
    }

    let base: BTreeSet<String> = match alias {
        SERVER_SKILLS => {
            if cfg.tools_allow.is_empty() {
                live_tools.iter().cloned().collect()
            } else {
                cfg.tools_allow.iter().cloned().collect()
            }
        }
        SERVER_USAGE => {
            if cfg.tools_allow.is_empty() {
                USAGE_TOOLS.iter().map(|&s| s.to_string()).collect()
            } else {
                cfg.tools_allow.iter().cloned().collect()
            }
        }
        _ => {
            let preset =
                preset_for_mode(alias, &cfg.mode).ok_or_else(|| PolicyError::UnknownMode {
                    server: alias.to_string(),
                    mode: cfg.mode.clone(),
                })?;
            if cfg.tools_allow.is_empty() {
                preset.iter().map(|&s| s.to_string()).collect()
            } else {
                cfg.tools_allow.iter().cloned().collect()
            }
        }
    };

    let deny: BTreeSet<&str> = cfg.tools_deny.iter().map(String::as_str).collect();

    let mut exposed = Vec::new();
    let mut exposed_set = BTreeSet::new();
    for tool in live_tools {
        if base.contains(tool) && !deny.contains(tool.as_str()) {
            exposed.push(tool.clone());
            exposed_set.insert(tool.clone());
        }
    }
    exposed.sort();

    let (hidden, unknown) = classify(alias, live_tools, &exposed_set);

    Ok(Report {
        server: alias.to_string(),
        enabled: cfg.enabled,
        mode: cfg.mode.clone(),
        exposed,
        hidden,
        unknown,
        exposures: None,
    })
}

/// Resolves the preview exposure [`Report`] for a mode-based server (`vault` or `memory`)
/// using this crate's versioned reference universe as the stand-in catalog.
///
/// # Errors
///
/// Returns [`PolicyError`] if invoked for a server without mode presets (e.g. `skills`).
pub fn evaluate_preset(alias: &str, cfg: &ServerConfig) -> Result<Report, PolicyError> {
    if alias != SERVER_VAULT && alias != SERVER_MEMORY {
        return Err(PolicyError::PresetUnsupported(format!(
            "EvaluatePreset only supports \"{SERVER_VAULT}\" and \"{SERVER_MEMORY}\" (skills has no mode preset)"
        )));
    }

    let mut live_tools = known_tools(alias);
    if alias == SERVER_MEMORY {
        live_tools.extend(ACTIVITY_TOOLS.iter().map(|&s| s.to_string()));
    }

    evaluate(alias, cfg, &live_tools)
}

fn classify(
    alias: &str,
    live_tools: &[String],
    exposed: &BTreeSet<String>,
) -> (Vec<String>, Vec<String>) {
    let universe = universe_for(alias);
    let bounded = alias == SERVER_VAULT || alias == SERVER_MEMORY || alias == SERVER_USAGE;

    let mut known: BTreeSet<&str> =
        universe.map_or_else(BTreeSet::new, |tools| tools.iter().copied().collect());
    if alias == SERVER_MEMORY {
        for &tool in ACTIVITY_TOOLS {
            known.insert(tool);
        }
    }

    let mut hidden = Vec::new();
    let mut unknown = Vec::new();

    for tool in live_tools {
        if exposed.contains(tool) {
            continue;
        }
        if !bounded || known.contains(tool.as_str()) {
            hidden.push(tool.clone());
        } else {
            unknown.push(tool.clone());
        }
    }

    hidden.sort();
    unknown.sort();
    (hidden, unknown)
}
