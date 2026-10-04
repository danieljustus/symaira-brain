#![deny(unsafe_code)]

//! Runtime-detected SymVault, Keychain, and environment key sources.

use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitStatus,
    time::Duration,
};

use fs2::FileExt;
use zeroize::Zeroizing;

use crate::key_resolver::{
    KeyProvisioner, KeySources, MissingReason, ProbeError, ProvisionOutcome,
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
mod command;
mod startup_owner;
#[cfg(test)]
use command::INJECT_ETXTBSY;
use command::run_command;
pub use startup_owner::{STARTUP_PROVIDER_ARGUMENT, run_startup_provider};

#[derive(Clone)]
pub struct SystemKeySources {
    symvault: PathBuf,
    #[cfg(target_os = "macos")]
    security: PathBuf,
    timeout: Duration,
    lock_path: PathBuf,
    startup_owner: Option<PathBuf>,
}

impl Default for SystemKeySources {
    fn default() -> Self {
        Self {
            symvault: PathBuf::from("symvault"),
            #[cfg(target_os = "macos")]
            security: PathBuf::from("security"),
            timeout: DEFAULT_TIMEOUT,
            lock_path: default_lock_path(),
            startup_owner: None,
        }
    }
}

impl SystemKeySources {
    #[must_use]
    pub fn with_programs(
        symvault: impl Into<PathBuf>,
        security: impl Into<PathBuf>,
        timeout: Duration,
    ) -> Self {
        #[cfg(target_os = "macos")]
        let security = security.into();
        #[cfg(not(target_os = "macos"))]
        let _ = security.into();
        let symvault = symvault.into();
        let lock_path = symvault.with_extension("key-init.lock");
        Self {
            symvault,
            #[cfg(target_os = "macos")]
            security,
            timeout,
            lock_path,
            startup_owner: None,
        }
    }

    /// Attach an explicit CLI startup owner. Ordinary vault/provisioning users
    /// retain the existing standalone subprocess contract.
    #[must_use]
    pub fn with_startup_owner(mut self, executable: PathBuf) -> Self {
        self.startup_owner = Some(executable);
        self
    }

    fn lookup(&self, program: &Path, args: &[&str]) -> Result<command::CommandOutput, ProbeError> {
        match &self.startup_owner {
            Some(owner) => startup_owner::lookup(owner, program, args, self.timeout),
            None => run_command(program, args, None, self.timeout),
        }
    }

    fn acquire_init_lock(&self) -> Result<fs::File, ProbeError> {
        let lock = open_init_lock(&self.lock_path)
            .map_err(|error| ProbeError::Failed(format!("open key-init lock: {error}")))?;
        lock.lock_exclusive()
            .map_err(|error| ProbeError::Failed(format!("lock key initialization: {error}")))?;
        Ok(lock)
    }

    pub fn set_vault(&self, entry: &str, key: &[u8; 32]) -> Result<(), ProbeError> {
        let mut input = Zeroizing::new(hex_key(key).into_bytes());
        input.push(b'\n');
        let output = run_command(
            &self.symvault,
            &["set", entry, "--stdin-value"],
            Some(&input),
            self.timeout,
        )?;
        require_success(output.status, "symvault set")
    }

    #[cfg(target_os = "macos")]
    pub fn set_keychain(
        &self,
        service: &str,
        account: &str,
        key: &[u8; 32],
    ) -> Result<(), ProbeError> {
        let mut input = Zeroizing::new(hex_key(key).into_bytes());
        input.push(b'\n');
        let output = run_command(
            &self.security,
            &["add-generic-password", "-s", service, "-a", account, "-w"],
            Some(&input),
            self.timeout,
        )?;
        require_success(output.status, "keychain set")
    }

    #[cfg(not(target_os = "macos"))]
    pub fn set_keychain(
        &self,
        _service: &str,
        _account: &str,
        _key: &[u8; 32],
    ) -> Result<(), ProbeError> {
        Err(ProbeError::Missing(MissingReason::Unavailable))
    }
}

impl KeySources for SystemKeySources {
    fn vault(&self, entry: &str) -> Result<Option<Vec<u8>>, ProbeError> {
        let output = match self.lookup(&self.symvault, &["get", entry]) {
            Ok(output) => output,
            Err(ProbeError::Missing(MissingReason::Unavailable)) => return Ok(None),
            Err(error) => return Err(error),
        };
        match output.status.code() {
            Some(0) => {
                Ok((!output.stdout.iter().all(u8::is_ascii_whitespace)).then_some(output.stdout))
            }
            Some(2) => Err(ProbeError::Missing(MissingReason::NotFound)),
            Some(3) => Err(ProbeError::Missing(MissingReason::NotInitialized)),
            Some(code) => Err(ProbeError::Failed(format!("exit status {code}"))),
            None => Err(ProbeError::Failed("terminated by signal".to_owned())),
        }
    }

    #[cfg(target_os = "macos")]
    fn keychain(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, ProbeError> {
        let output = match self.lookup(
            &self.security,
            &["find-generic-password", "-s", service, "-a", account, "-w"],
        ) {
            Ok(output) => output,
            Err(ProbeError::Missing(MissingReason::Unavailable)) => {
                return Err(ProbeError::Failed(format!(
                    "keychain lookup: exec: {:?}: executable file not found in $PATH",
                    self.security
                )));
            }
            Err(error) => return Err(error),
        };
        match output.status.code() {
            Some(0) => {
                let value = trim_ascii_space(&output.stdout);
                Ok((!value.is_empty()).then(|| value.to_vec()))
            }
            Some(44) => Ok(None),
            Some(code) => Err(ProbeError::Failed(format!(
                "keychain lookup: exit status {code}"
            ))),
            None => Err(ProbeError::Failed(
                "keychain lookup: terminated by signal".to_owned(),
            )),
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn keychain(&self, _service: &str, _account: &str) -> Result<Option<Vec<u8>>, ProbeError> {
        Ok(None)
    }

    fn environment(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

impl KeyProvisioner for SystemKeySources {
    fn provision_vault(&self, entry: &str, key: &[u8; 32]) -> Result<ProvisionOutcome, ProbeError> {
        let _lock = self.acquire_init_lock()?;
        match self.vault(entry) {
            Ok(Some(_)) => return Ok(ProvisionOutcome::Existing),
            Ok(None) | Err(ProbeError::Missing(_)) => {}
            Err(error) => return Err(error),
        }
        self.set_vault(entry, key)?;
        Ok(ProvisionOutcome::Created)
    }

    fn provision_keychain(
        &self,
        service: &str,
        account: &str,
        key: &[u8; 32],
    ) -> Result<ProvisionOutcome, ProbeError> {
        let _lock = self.acquire_init_lock()?;
        match self.keychain(service, account) {
            Ok(Some(_)) => return Ok(ProvisionOutcome::Existing),
            Ok(None) | Err(ProbeError::Missing(_)) => {}
            Err(error) => return Err(error),
        }
        self.set_keychain(service, account, key)?;
        Ok(ProvisionOutcome::Created)
    }
}

fn require_success(status: ExitStatus, action: &str) -> Result<(), ProbeError> {
    match status.code() {
        Some(0) => Ok(()),
        Some(code) => Err(ProbeError::Failed(format!("{action}: exit status {code}"))),
        None => Err(ProbeError::Failed(format!(
            "{action}: terminated by signal"
        ))),
    }
}

#[cfg(target_os = "macos")]
fn trim_ascii_space(value: &[u8]) -> &[u8] {
    value.trim_ascii()
}

fn default_lock_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("symbrowse/key-init.lock")
}

fn prepare_lock_parent(path: &Path) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("key-init lock has no parent"))?;
    if let Ok(metadata) = fs::symlink_metadata(parent)
        && (!metadata.is_dir() || metadata.file_type().is_symlink())
    {
        return Err(std::io::Error::other(
            "key-init lock parent is not a regular directory",
        ));
    }
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(unix)]
fn open_init_lock(path: &Path) -> std::io::Result<fs::File> {
    use rustix::fs::{Mode, OFlags, open};

    prepare_lock_parent(path)?;
    let descriptor = open(
        path,
        OFlags::CREATE | OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR,
    )?;
    Ok(fs::File::from(descriptor))
}

#[cfg(not(unix))]
fn open_init_lock(path: &Path) -> std::io::Result<fs::File> {
    prepare_lock_parent(path)?;
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        return Err(std::io::Error::other("key-init lock is a symlink"));
    }
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

fn hex_key(key: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in key {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etxtbsy_spawn_is_retried_within_command_deadline() {
        INJECT_ETXTBSY.with(|remaining| remaining.set(1));
        #[cfg(windows)]
        let (program, args) = (
            std::env::var_os("ComSpec")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe")),
            vec!["/C", "exit", "0"],
        );
        #[cfg(not(windows))]
        let (program, args) = (PathBuf::from("/usr/bin/true"), Vec::new());
        let output = run_command(&program, &args, None, Duration::from_secs(1))
            .expect("transient ETXTBSY should be retried");
        assert_eq!(output.status.code(), Some(0));
    }

    #[test]
    fn etxtbsy_retry_does_not_extend_expired_deadline() {
        INJECT_ETXTBSY.with(|remaining| remaining.set(1));
        let error = run_command(Path::new("/usr/bin/true"), &[], None, Duration::ZERO)
            .expect_err("expired deadline must fail before retry");
        assert!(matches!(
            error,
            ProbeError::Failed(message) if message.contains("timed out before spawn")
        ));
    }
}
