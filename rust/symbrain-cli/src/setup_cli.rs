use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;

use serde::Serialize;
use symbrain_core::{exit, xdg};
use symbrain_managed::{
    InstallOutcome, Installer, Manifest, Platform, installed_version, versions_match,
};

const USAGE: &str = "Usage of setup:\n  -allow-unsigned\n    \tinstall even if cosign or a core's signature is unavailable (prints a warning; skips publisher verification for that core)\n  -fix\n    \trepair missing or version-mismatched binaries (alias for doctor --fix)\n  -json\n    \temit machine-readable JSON\n";

#[derive(Debug, Default)]
struct SetupArgs {
    json: bool,
    fix: bool,
    allow_unsigned: bool,
}

#[derive(Serialize)]
struct SetupReport {
    bin_dir: String,
    results: Vec<CoreResult>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    errors: Vec<String>,
}

#[derive(Serialize)]
struct CoreResult {
    name: String,
    version: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub fn run(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let parsed = match parse_args(args, stderr) {
        Ok(args) => args,
        Err(code) => return code,
    };
    let Some(bin_dir) = xdg::managed_bin_dir() else {
        let _ = writeln!(stderr, "symbrain setup: user home directory not found");
        return exit::GENERIC;
    };
    let manifest = match Manifest::load_embedded() {
        Ok(manifest) => manifest,
        Err(error) => {
            let prefix = if parsed.fix {
                "symbrain setup --fix"
            } else {
                "symbrain setup"
            };
            let _ = writeln!(stderr, "{prefix}: {error}");
            return exit::GENERIC;
        }
    };
    let enabled = match enabled_cores() {
        Ok(enabled) => enabled,
        Err(error) => {
            let prefix = if parsed.fix {
                "symbrain setup --fix"
            } else {
                "symbrain setup"
            };
            let _ = writeln!(stderr, "{prefix}: {error}");
            return exit::GENERIC;
        }
    };
    if parsed.fix {
        run_fix(
            &manifest,
            &bin_dir,
            parsed.json,
            parsed.allow_unsigned,
            &enabled,
            stdout,
            stderr,
        )
    } else {
        run_install(
            &manifest,
            &bin_dir,
            parsed.json,
            parsed.allow_unsigned,
            &enabled,
            stdout,
            stderr,
        )
    }
}

fn run_install(
    manifest: &Manifest,
    bin_dir: &std::path::Path,
    json: bool,
    allow_unsigned: bool,
    enabled: &BTreeMap<String, bool>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let platform = match Platform::current() {
        Ok(platform) => platform,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain setup: {error}");
            return exit::GENERIC;
        }
    };
    let installer = match Installer::new(bin_dir, allow_unsigned) {
        Ok(installer) => installer,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain setup: {error}");
            return exit::GENERIC;
        }
    };
    let mut report = SetupReport {
        bin_dir: bin_dir.display().to_string(),
        results: Vec::new(),
        errors: Vec::new(),
    };

    for (name, core) in manifest.active_cores(enabled) {
        if !core.supports_platform(platform.os) {
            report.results.push(CoreResult {
                name: name.clone(),
                version: core.version.clone(),
                status: "skipped",
                error: None,
            });
            if !json {
                let _ = writeln!(
                    stdout,
                    "  -  {name} {} (unsupported platform)",
                    core.version
                );
            }
            continue;
        }
        match installer.install(&core, platform, stderr) {
            Ok(InstallOutcome::Installed) => {
                report.results.push(CoreResult {
                    name: name.clone(),
                    version: core.version.clone(),
                    status: "installed",
                    error: None,
                });
                if !json {
                    let _ = writeln!(stdout, "  ✓  {name} {}", core.version);
                }
            }
            Ok(InstallOutcome::SkippedPlatform) => unreachable!("checked above"),
            Err(error) => {
                let detail = error.to_string();
                report.errors.push(format!("{name}: {detail}"));
                report.results.push(CoreResult {
                    name: name.clone(),
                    version: core.version.clone(),
                    status: "error",
                    error: Some(detail.clone()),
                });
                if !json {
                    let _ = writeln!(stderr, "  ✗  {name}: {detail}");
                }
            }
        }
    }

    if json {
        if serde_json::to_writer(&mut *stdout, &report)
            .and_then(|()| writeln!(stdout).map_err(serde_json::Error::io))
            .is_err()
        {
            let _ = writeln!(stderr, "symbrain setup: encode JSON");
            return exit::GENERIC;
        }
    } else {
        let _ = writeln!(stdout, "\nInstalled to {}", bin_dir.display());
    }
    if report.errors.is_empty() {
        exit::OK
    } else {
        exit::GENERIC
    }
}

fn run_fix(
    manifest: &Manifest,
    bin_dir: &std::path::Path,
    json: bool,
    allow_unsigned: bool,
    enabled: &BTreeMap<String, bool>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let platform = match Platform::current() {
        Ok(platform) => platform,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain setup --fix: {error}");
            return exit::GENERIC;
        }
    };
    let installer = match Installer::new(bin_dir, allow_unsigned) {
        Ok(installer) => installer,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain setup --fix: {error}");
            return exit::GENERIC;
        }
    };
    let mut report = SetupReport {
        bin_dir: bin_dir.display().to_string(),
        results: Vec::new(),
        errors: Vec::new(),
    };
    let mut fixed = 0;
    let mut skipped = 0;

    for (name, core) in manifest.active_cores(enabled) {
        if !core.supports_platform(platform.os) {
            skipped += 1;
            report.results.push(CoreResult {
                name: name.clone(),
                version: core.version.clone(),
                status: "skipped",
                error: None,
            });
            if !json {
                let _ = writeln!(
                    stdout,
                    "  -  {name} {} (unsupported platform)",
                    core.version
                );
            }
            continue;
        }

        let existing = installed_version(bin_dir, &core.binary_name).unwrap_or_default();
        if versions_match(&existing, &core.version) {
            skipped += 1;
            report.results.push(CoreResult {
                name: name.clone(),
                version: core.version.clone(),
                status: "skipped",
                error: None,
            });
            if !json {
                let _ = writeln!(stdout, "  ✓  {name} {existing} (already installed)");
            }
            continue;
        }

        match installer.install(&core, platform, stderr) {
            Ok(InstallOutcome::Installed) => {
                fixed += 1;
                report.results.push(CoreResult {
                    name: name.clone(),
                    version: core.version.clone(),
                    status: "installed",
                    error: None,
                });
                if !json {
                    let _ = writeln!(stdout, "  ✓  {name} {} (repaired)", core.version);
                }
            }
            Ok(InstallOutcome::SkippedPlatform) => unreachable!("checked above"),
            Err(error) => {
                let detail = error.to_string();
                report.errors.push(format!("{name}: {detail}"));
                report.results.push(CoreResult {
                    name: name.clone(),
                    version: core.version.clone(),
                    status: "error",
                    error: Some(detail.clone()),
                });
                if !json {
                    let _ = writeln!(stderr, "  ✗  {name}: {detail}");
                }
            }
        }
    }

    if json {
        if serde_json::to_writer(&mut *stdout, &report)
            .and_then(|()| writeln!(stdout).map_err(serde_json::Error::io))
            .is_err()
        {
            let _ = writeln!(stderr, "symbrain setup --fix: encode JSON");
            return exit::GENERIC;
        }
    } else {
        let _ = writeln!(stdout, "\n{fixed} fixed, {skipped} already correct");
    }
    if report.errors.is_empty() {
        exit::OK
    } else {
        exit::GENERIC
    }
}

fn enabled_cores() -> Result<BTreeMap<String, bool>, String> {
    let mut enabled = BTreeMap::new();
    if let Some(value) = std::env::var_os("SYMBRAIN_MODULES_BROWSE") {
        let value = value.to_string_lossy();
        enabled.insert(
            "symbrowse".to_string(),
            value.parse::<bool>().map_err(|_| {
                format!("config: invalid boolean SYMBRAIN_MODULES_BROWSE={value:?}")
            })?,
        );
    }
    let path = symbrain_core::xdg::config_path();
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(enabled),
        Err(error) => return Err(format!("config: read {}: {error}", path.display())),
    };
    let mut from_file = parse_enabled_cores(&bytes, &path)?;
    from_file.extend(enabled);
    Ok(from_file)
}

fn parse_enabled_cores(
    bytes: &[u8],
    path: &std::path::Path,
) -> Result<BTreeMap<String, bool>, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| format!("config: parse {}: {error}", path.display()))?;
    let document: toml_edit::DocumentMut = text
        .parse()
        .map_err(|error| format!("config: parse {}: {error}", path.display()))?;
    let mut enabled = BTreeMap::new();
    if let Some(value) = document.get("modules").and_then(|item| item.get("browse")) {
        let browse = value.as_bool().ok_or_else(|| {
            format!(
                "config: modules.browse must be boolean in {}",
                path.display()
            )
        })?;
        enabled.insert("symbrowse".to_string(), browse);
    }
    Ok(enabled)
}

fn parse_args(args: &[OsString], stderr: &mut dyn Write) -> Result<SetupArgs, u8> {
    let normalized = crate::normalize_flags(args);
    let mut parsed = SetupArgs::default();
    for argument in normalized {
        let argument = argument.to_string_lossy();
        if argument == "--" || argument == "-" || !argument.starts_with('-') {
            break;
        }
        let flag = argument.trim_start_matches('-');
        if flag == "h" || flag == "help" {
            let _ = write!(stderr, "{USAGE}");
            return Err(exit::USAGE);
        }
        let (name, value) = flag
            .split_once('=')
            .map_or((flag, None), |(name, value)| (name, Some(value)));
        let target = match name {
            "json" => &mut parsed.json,
            "fix" => &mut parsed.fix,
            "allow-unsigned" => &mut parsed.allow_unsigned,
            _ => {
                let _ = writeln!(stderr, "flag provided but not defined: -{name}");
                let _ = write!(stderr, "{USAGE}");
                return Err(exit::USAGE);
            }
        };
        match value {
            None | Some("true") => *target = true,
            Some("false") => *target = false,
            Some(value) => {
                let _ = writeln!(
                    stderr,
                    "invalid value {value:?} for flag -{name}: parse error"
                );
                let _ = write!(stderr, "{USAGE}");
                return Err(exit::USAGE);
            }
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flags_and_stop_at_first_positional_match_go_flagset() {
        let mut stderr = Vec::new();
        let parsed = parse_args(
            &[
                "--fix".into(),
                "--allow-unsigned=false".into(),
                "positional".into(),
                "--json".into(),
            ],
            &mut stderr,
        )
        .unwrap();
        assert!(parsed.fix);
        assert!(!parsed.allow_unsigned);
        assert!(!parsed.json);
        assert!(stderr.is_empty());
    }

    #[test]
    fn optional_browse_selection_matches_go_default_and_explicit_values() {
        let path = std::path::Path::new("config.toml");
        let empty = parse_enabled_cores(b"", path).unwrap();
        assert!(!empty.get("symbrowse").copied().unwrap_or(false));
        assert!(
            parse_enabled_cores(b"[modules]\nbrowse = false\n", path)
                .unwrap()
                .get("symbrowse")
                .is_some_and(|value| !value)
        );
        assert!(
            parse_enabled_cores(b"[modules]\nbrowse = true\n", path)
                .unwrap()
                .get("symbrowse")
                .is_some_and(|value| *value)
        );
        assert!(parse_enabled_cores(b"[modules]\nbrowse = 1\n", path).is_err());
    }

    #[test]
    fn unknown_flag_has_go_usage_shape() {
        let mut stderr = Vec::new();
        let result = parse_args(&["--unknown".into()], &mut stderr);
        assert_eq!(result.unwrap_err(), exit::USAGE);
        let text = String::from_utf8(stderr).unwrap();
        assert!(text.starts_with("flag provided but not defined: -unknown\nUsage of setup:\n"));
    }
}
