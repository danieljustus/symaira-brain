use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;

use serde::Serialize;
use symbrain_core::exit;
use symbrain_managed::{
    InstallOutcome, Installer, Manifest, Platform, installed_version, versions_match,
};

#[path = "setup_config.rs"]
mod config;
pub(crate) use config::enabled_cores;
pub(crate) use config::parse_go_bool;
#[path = "setup_args.rs"]
mod flags;
use flags::{SetupArgs, parse_args};

#[path = "setup_source.rs"]
mod source;

#[cfg(windows)]
const HOME_VARIABLE: &str = "USERPROFILE";
#[cfg(not(windows))]
const HOME_VARIABLE: &str = "HOME";

fn setup_home() -> Option<std::path::PathBuf> {
    std::env::var_os(HOME_VARIABLE)
        .filter(|home| !home.is_empty())
        .map(std::path::PathBuf::from)
}

/// Preserve full typed configuration diagnostics until their native cutover.
pub(crate) fn requires_go_fallback(args: &[OsString]) -> bool {
    parse_args(args, &mut Vec::new()).is_ok_and(|parsed| {
        if setup_home().is_none() {
            return false;
        }
        let source = !parsed.from_source.is_empty();
        if source && (parsed.fix || parsed.allow_unsigned) {
            return false;
        }
        if !source && !parsed.modules.is_empty() {
            return false;
        }
        // Go validates the source root before loading configuration.
        if source && !source::valid_root(&parsed.from_source) {
            return false;
        }
        (parsed.fix || source)
            && (!crate::vault_config::valid_configuration() || enabled_cores().is_err())
    })
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

impl CoreResult {
    fn skipped(name: String, version: &str) -> Self {
        Self {
            name,
            version: version.to_owned(),
            status: "skipped",
            error: None,
        }
    }
}

pub fn run(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let parsed = match parse_args(args, stderr) {
        Ok(args) => args,
        Err(code) => return code,
    };
    #[cfg(windows)]
    let home_label = "%userprofile%";
    #[cfg(not(windows))]
    let home_label = "$HOME";
    let Some(bin_dir) = setup_home().map(|home| home.join(".symaira").join("bin")) else {
        let _ = writeln!(
            stderr,
            "symbrain setup: managed: cannot determine home directory: {home_label} is not defined"
        );
        return exit::GENERIC;
    };
    if !parsed.from_source.is_empty() {
        if parsed.fix || parsed.allow_unsigned {
            let _ = writeln!(
                stderr,
                "symbrain setup: --from-source cannot be combined with --fix or --allow-unsigned"
            );
            return exit::USAGE;
        }
        return source::run(
            &bin_dir,
            &parsed.from_source,
            &parsed.modules,
            parsed.json,
            stdout,
            stderr,
        );
    }
    if !parsed.modules.is_empty() {
        let _ = writeln!(stderr, "symbrain setup: --modules requires --from-source");
        return exit::USAGE;
    }
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
        run_fix(&manifest, &bin_dir, &parsed, &enabled, stdout, stderr)
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
            report
                .results
                .push(CoreResult::skipped(name.clone(), &core.version));
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

    if !json {
        let _ = writeln!(stdout, "\nInstalled to {}", bin_dir.display());
    }
    finish(&report, json, "symbrain setup", stdout, stderr)
}

fn run_fix(
    manifest: &Manifest,
    bin_dir: &std::path::Path,
    parsed: &SetupArgs,
    enabled: &BTreeMap<String, bool>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let (json, allow_unsigned, force_release) =
        (parsed.json, parsed.allow_unsigned, parsed.force_release);
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
            report
                .results
                .push(CoreResult::skipped(name.clone(), &core.version));
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
            report
                .results
                .push(CoreResult::skipped(name.clone(), &core.version));
            if !json {
                let _ = writeln!(stdout, "  ✓  {name} {existing} (already installed)");
            }
            continue;
        }

        if !force_release && symbrain_managed::is_brain_source_install(bin_dir, &core.binary_name) {
            skipped += 1;
            report
                .results
                .push(CoreResult::skipped(name.clone(), &core.version));
            if !json {
                let _ = writeln!(
                    stdout,
                    "  -  {name} {existing} (brain-source build; use --force-release to replace)"
                );
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

    if !json {
        let _ = writeln!(stdout, "\n{fixed} fixed, {skipped} already correct");
    }
    finish(&report, json, "symbrain setup --fix", stdout, stderr)
}

fn finish(
    report: &SetupReport,
    json: bool,
    prefix: &str,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    if json
        && serde_json::to_writer(&mut *stdout, report)
            .and_then(|()| writeln!(stdout).map_err(serde_json::Error::io))
            .is_err()
    {
        let _ = writeln!(stderr, "{prefix}: encode JSON");
        return exit::GENERIC;
    }
    if report.errors.is_empty() {
        exit::OK
    } else {
        exit::GENERIC
    }
}
