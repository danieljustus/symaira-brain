//! Native Doctor repair, retaining per-core failure and source-build protection.
use std::io::Write;

use symbrain_core::exit;
use symbrain_managed::{
    Installer, Manifest, Platform, installed_version, read_provenance_bytes, versions_match,
};

#[path = "doctor_fix_log.rs"]
mod log;

#[cfg(windows)]
const HOME_VARIABLE: &str = "USERPROFILE";
#[cfg(not(windows))]
const HOME_VARIABLE: &str = "HOME";

pub(super) fn managed_bin_dir() -> Option<std::path::PathBuf> {
    std::env::var_os(HOME_VARIABLE)
        .filter(|home| !home.is_empty())
        .map(|home| std::path::PathBuf::from(home).join(".symaira").join("bin"))
}

pub(super) fn run_fix(force_release: bool, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let Some(bin_dir) = managed_bin_dir() else {
        let home_label = if cfg!(windows) {
            "%userprofile%"
        } else {
            "$HOME"
        };
        let _ = writeln!(
            stderr,
            "symbrain doctor --fix: managed: cannot determine home directory: {home_label} is not defined"
        );
        return exit::GENERIC;
    };
    let _ = writeln!(stdout, "symbrain doctor --fix\n");
    let enabled = match crate::setup_cli::enabled_cores() {
        Ok(enabled) => enabled,
        Err(error) => {
            let _ = writeln!(stderr, "  ✗  {error}");
            return exit::USAGE;
        }
    };
    let repair = repair(&bin_dir, &enabled, force_release, stderr);
    if let Err(error) = repair {
        let _ = writeln!(stderr, "  ✗  repair failed: {error}");
        return exit::GENERIC;
    }
    let _ = stdout.write_all(b"\nDone. Binaries installed to ");
    let _ = stdout.write_all(&symbrain_core::config::os_bytes(bin_dir.as_os_str()));
    let _ = stdout.write_all(b"\n");
    exit::OK
}

fn repair(
    bin_dir: &std::path::Path,
    enabled: &std::collections::BTreeMap<String, bool>,
    force_release: bool,
    stderr: &mut dyn Write,
) -> Result<(), String> {
    let manifest = Manifest::load_embedded().map_err(|error| error.to_string())?;
    let platform = Platform::current().map_err(|error| error.to_string())?;
    let installer = Installer::new(bin_dir, false).map_err(|error| error.to_string())?;
    let (mut repaired, mut skipped, mut failed, mut attempted) = (0, 0, 0, 0);
    for (name, core) in manifest.active_cores(enabled) {
        if !core.supports_platform(platform.os) {
            log::event(
                stderr,
                "INFO",
                "skipping (platform)",
                &[("binary", &name), ("goos", platform.os)],
            );
            continue;
        }
        attempted += 1;
        let existing = match installed_version(bin_dir, &core.binary_name) {
            Ok(version) => version,
            Err(error) => {
                let error = error.into_go_text();
                log::event_bytes(
                    stderr,
                    "WARN",
                    "cannot probe",
                    &[("binary", name.as_bytes()), ("error", error.as_ref())],
                );
                String::new()
            }
        };
        if versions_match(&existing, &core.version) {
            log::event(
                stderr,
                "INFO",
                "already correct",
                &[("binary", &name), ("version", &existing)],
            );
            skipped += 1;
            continue;
        }
        if !force_release && protected(bin_dir, &core, &name, &existing, stderr) {
            skipped += 1;
            continue;
        }
        if existing.is_empty() {
            log::event(
                stderr,
                "INFO",
                "binary missing, installing",
                &[("binary", &name), ("version", &core.version)],
            );
        } else {
            log::event(
                stderr,
                "INFO",
                "version mismatch, repairing",
                &[
                    ("binary", &name),
                    ("installed", &existing),
                    ("wanted", &core.version),
                ],
            );
        }
        match installer.install(&core, platform, stderr) {
            Ok(_) => {
                log::event(
                    stderr,
                    "INFO",
                    "repaired",
                    &[("binary", &name), ("version", &core.version)],
                );
                repaired += 1;
            }
            Err(error) => {
                let error = error.into_go_text();
                log::event_bytes(
                    stderr,
                    "ERROR",
                    "repair failed",
                    &[("binary", name.as_bytes()), ("error", error.as_ref())],
                );
                failed += 1;
            }
        }
    }
    log::event(
        stderr,
        "INFO",
        "doctor --fix complete",
        &[
            ("repaired", &repaired.to_string()),
            ("skipped", &skipped.to_string()),
            ("failed", &failed.to_string()),
        ],
    );
    if failed > 0 {
        Err(format!(
            "managed: {failed}/{attempted} cores failed to repair"
        ))
    } else {
        Ok(())
    }
}

fn protected(
    bin_dir: &std::path::Path,
    core: &symbrain_managed::Core,
    name: &str,
    existing: &str,
    stderr: &mut dyn Write,
) -> bool {
    match read_provenance_bytes(bin_dir, &core.binary_name) {
        Err(error) => {
            log::event_bytes(
                stderr,
                "WARN",
                "cannot read provenance; leaving binary untouched",
                &[("binary", name.as_bytes()), ("error", error.as_ref())],
            );
            return true;
        }
        Ok(Some(record)) if record.source == "brain-source" => {
            log::event(
                stderr,
                "INFO",
                "brain-source build installed; not overwriting with release (use --force-release to replace)",
                &[
                    ("binary", name),
                    ("installed", existing),
                    ("wanted", &core.version),
                    ("receiver_commit", &record.receiver_commit),
                ],
            );
            return true;
        }
        Ok(_) => {}
    }
    false
}
