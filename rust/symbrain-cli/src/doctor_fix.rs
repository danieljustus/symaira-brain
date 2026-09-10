use std::fs;
use std::io::Write;

use symbrain_core::{exit, xdg};
use symbrain_managed::{Installer, Manifest, Platform, installed_version};

pub(super) fn run_fix(stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let Some(bin_dir) = xdg::managed_bin_dir() else {
        let _ = writeln!(
            stderr,
            "symbrain doctor --fix: user home directory not found"
        );
        return exit::GENERIC;
    };
    let _ = writeln!(stdout, "symbrain doctor --fix\n");
    let manifest = match Manifest::load_embedded() {
        Ok(manifest) => manifest,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain doctor --fix: {error}");
            return exit::GENERIC;
        }
    };
    let platform = match Platform::current() {
        Ok(platform) => platform,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain doctor --fix: {error}");
            return exit::GENERIC;
        }
    };
    let installer = match Installer::new(&bin_dir, false) {
        Ok(installer) => installer,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain doctor --fix: {error}");
            return exit::GENERIC;
        }
    };
    if let Err(error) = fs::create_dir_all(&bin_dir) {
        let _ = writeln!(stderr, "  ✗  repair failed: {error}");
        return exit::GENERIC;
    }
    for core in manifest
        .cores
        .values()
        .filter(|core| core.supports_platform(platform.os))
    {
        let existing = installed_version(&bin_dir, &core.binary_name).unwrap_or_default();
        if symbrain_managed::versions_match(&existing, &core.version) {
            continue;
        }
        if let Err(error) = installer.install(core, platform, stderr) {
            let _ = writeln!(stderr, "  ✗  repair failed: {error}");
            return exit::GENERIC;
        }
    }
    let _ = writeln!(
        stdout,
        "\nDone. Binaries installed to {}",
        bin_dir.display()
    );
    exit::OK
}
