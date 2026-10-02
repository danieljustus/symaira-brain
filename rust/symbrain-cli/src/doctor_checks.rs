use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use symbrain_audit::Degradation;
use symbrain_core::xdg;
use symbrain_managed::{Manifest, Platform, installed_version};
use toml_edit::DocumentMut;

use super::doctor_core::{check_harnesses, check_memory_db, check_skills_library, probe_version};
use super::doctor_links::{check_foreign_access_risks, check_handshakes, check_links};
use super::doctor_process::which;
use super::doctor_types::{
    BUILTINS, ConfigCheck, DirCheck, DoctorReport, ManagedCoreCheck, ManagedModuleCheck,
    ManagedModuleProvenance, OPTIONAL_MODULES, ServerCheck,
};

pub(super) fn run_checks(vault_agent: &str) -> DoctorReport {
    let config_dir = xdg::config_dir();
    let data_dir = xdg::data_dir().unwrap_or_default();
    let cache_dir = xdg::cache_dir().unwrap_or_default();
    let managed_dir = xdg::managed_bin_dir().unwrap_or_default();
    let profiles_dir = xdg::profiles_dir();
    let config_path = xdg::config_path();
    let profiles = symbrain_policy::list_names().unwrap_or_default();
    let harnesses = check_harnesses(&profiles_dir);
    DoctorReport {
        config_dir: check_dir(&config_dir),
        data_dir: check_dir(&data_dir),
        cache_dir: check_dir(&cache_dir),
        config: check_config(&config_path),
        managed_dir: check_dir(&managed_dir),
        builtins: BUILTINS.iter().map(|s| (*s).to_string()).collect(),
        servers: check_servers(&managed_dir),
        managed_cores: check_managed_cores(&managed_dir),
        managed_modules: check_managed_modules(&managed_dir),
        memory_db: check_memory_db(),
        skills_library: check_skills_library(),
        profiles,
        handshakes: check_handshakes(vault_agent),
        links: check_links(&harnesses),
        foreign_access_risks: check_foreign_access_risks(),
        degradations: match symbrain_audit::latest_degradations("") {
            Ok(degradations) => degradations,
            Err(error) => vec![Degradation {
                server: "audit".to_string(),
                reason: format!("read audit log: {error}"),
                level: "warning".to_string(),
                ..Degradation::default()
            }],
        },
        harnesses,
    }
}

fn check_dir(path: &Path) -> DirCheck {
    DirCheck {
        path: path.display().to_string(),
        exists: path.is_dir(),
    }
}

fn check_config(path: &Path) -> ConfigCheck {
    let exists = path.is_file();
    let mut result = ConfigCheck {
        path: path.display().to_string(),
        exists,
        parsed: false,
        error: String::new(),
    };
    match fs::read_to_string(path) {
        Ok(contents) => match contents.parse::<DocumentMut>() {
            Ok(_) => result.parsed = true,
            Err(error) => {
                result.error = format!("config: failed to load {}: {error}", path.display());
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => result.parsed = true,
        Err(error) => result.error = format!("config: failed to load {}: {error}", path.display()),
    }
    result
}

fn check_servers(managed_dir: &Path) -> Vec<ServerCheck> {
    let mut check = ServerCheck {
        name: "vault".to_string(),
        binary: "symvault".to_string(),
        found: false,
        path: String::new(),
        version: String::new(),
        managed_version: String::new(),
        origin: String::new(),
        probe_error: String::new(),
        install_hint: "brew install danieljustus/tap/symvault".to_string(),
    };
    if let Ok(version) = installed_version(managed_dir, "symvault") {
        check.managed_version = version;
    }
    let path = which("symvault");
    if let Some(path) = path {
        check.found = true;
        check.install_hint.clear();
        check.path = path.display().to_string();
        check.origin = if path.starts_with(managed_dir) {
            "managed"
        } else {
            "path"
        }
        .to_string();
        match probe_version(&path) {
            Ok(version) => check.version = version,
            Err(error) => check.probe_error = error,
        }
    } else if !check.managed_version.is_empty() {
        check.found = true;
        check.install_hint.clear();
        check.path = managed_dir.join("symvault").display().to_string();
        check.origin = "managed".to_string();
        check.version.clone_from(&check.managed_version);
    }
    vec![check]
}

fn check_managed_cores(managed_dir: &Path) -> Vec<ManagedCoreCheck> {
    let Ok(manifest) = Manifest::load_embedded() else {
        return Vec::new();
    };
    let Ok(platform) = Platform::current() else {
        return Vec::new();
    };
    let mut checks = manifest
        .cores
        .iter()
        .filter(|(_, core)| core.supports_platform(platform.os))
        .map(|(name, core)| ManagedCoreCheck {
            name: name.clone(),
            pinned: core.version.clone(),
            version: installed_version(managed_dir, &core.binary_name).unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    checks.sort_by(|a, b| a.name.cmp(&b.name));
    checks
}

const MAX_MODULE_PROVENANCE_BYTES: u64 = 16 * 1024;

fn check_managed_modules(managed_dir: &Path) -> Vec<ManagedModuleCheck> {
    OPTIONAL_MODULES
        .iter()
        .filter_map(|(module, binary)| {
            let path = managed_dir.join(binary);
            let metadata = fs::symlink_metadata(&path).ok()?;
            if !metadata.file_type().is_file() {
                return None;
            }
            Some(ManagedModuleCheck {
                module: (*module).to_string(),
                binary: (*binary).to_string(),
                installed: true,
                path: path.display().to_string(),
                provenance: read_module_provenance(managed_dir, module, binary),
            })
        })
        .collect()
}

fn read_module_provenance(
    managed_dir: &Path,
    module: &str,
    binary: &str,
) -> Option<ManagedModuleProvenance> {
    let sidecar_path = managed_dir.join(format!("{binary}.provenance.json"));
    let metadata = fs::symlink_metadata(&sidecar_path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_MODULE_PROVENANCE_BYTES {
        return None;
    }

    let file = File::open(&sidecar_path).ok()?;
    let opened_metadata = file.metadata().ok()?;
    if !opened_metadata.is_file() || opened_metadata.len() > MAX_MODULE_PROVENANCE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_MODULE_PROVENANCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_MODULE_PROVENANCE_BYTES {
        return None;
    }

    let provenance: ManagedModuleProvenance = serde_json::from_slice(&bytes).ok()?;
    if provenance.binary != binary
        || !matches!(provenance.source.as_str(), "brain-source" | "release")
        || (provenance.source == "brain-source" && provenance.module_dir != module)
    {
        return None;
    }
    Some(provenance)
}
