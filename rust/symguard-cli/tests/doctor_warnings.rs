//! The actual Brain compatibility adapter must buffer warnings before fallback.
use std::{fs, process::Command};
#[cfg(windows)]
#[path = "support/windows_discovery.rs"]
mod windows_discovery;

#[path = "../../symbrain-cli/src/guard_cli.rs"]
#[expect(
    unused_imports,
    reason = "The exact Brain adapter also re-exports run_at_path, covered by raw_diagnostics"
)]
mod brain_adapter;

#[cfg(unix)]
const ROLES: &[&str] = &["healthy", "semantic", "type", "discovery", "raw"];
#[cfg(not(unix))]
const ROLES: &[&str] = &["healthy", "semantic", "type", "discovery"];

fn config_path(directory: &std::path::Path, role: &str) -> std::path::PathBuf {
    #[cfg(not(unix))]
    let _ = role;
    #[cfg(unix)]
    if role == "raw" {
        use std::os::unix::ffi::OsStrExt;
        return directory.join(std::ffi::OsStr::from_bytes(b"owned-\xe2\x82<&>"));
    }
    directory.join("config.toml")
}

#[test]
fn brain_doctor_warning_admission() {
    if let Ok(role) = std::env::var("OWNED_GUARD_WARNING_ROLE") {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let outcome = brain_adapter::run(&["doctor".into()], &mut stdout, &mut stderr);
        if matches!(role.as_str(), "type" | "discovery") {
            assert_eq!(outcome, None);
            assert!(stdout.is_empty() && stderr.is_empty());
        } else {
            assert_eq!(
                outcome,
                Some(u8::from(role == "semantic")),
                "role {role}: stdout={stdout:?}, stderr={stderr:?}"
            );
            assert!(stdout.starts_with(b"symguard doctor\n"));
            assert!(stderr.starts_with(b"config: warning: unknown key \"owned\" in "));
            assert!(!stderr.strip_suffix(b"\n").unwrap().contains(&b'\n'));
            #[cfg(unix)]
            if role == "raw" {
                assert!(stderr.windows(2).any(|bytes| bytes == b"\xe2\x82"));
            }
        }
        return;
    }
    for &role in ROLES {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let directory = home.join(".config/symguard");
        fs::create_dir_all(&directory).unwrap();
        #[cfg(windows)]
        windows_discovery::create_parents(&home, &home.join(".config"));
        let config = config_path(&directory, role);
        let text = match role {
            "semantic" => "owned=1\nsequence={enabled=true,threshold=1}\n",
            "type" => "owned=1\nsequence={enabled=\"bad\"}\n",
            _ => "owned=1\n",
        };
        fs::write(&config, text).unwrap();
        if role == "discovery" {
            fs::create_dir_all(home.join(".cursor")).unwrap();
            fs::write(home.join(".cursor/mcp.json"), "{bad").unwrap();
        }
        let before = fs::metadata(&config).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "brain_doctor_warning_admission", "--nocapture"])
            .env_clear()
            .env("OWNED_GUARD_WARNING_ROLE", role)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", root.path().join("data"))
            .env("XDG_CACHE_HOME", root.path().join("cache"))
            .env("SYMGUARD_CONFIG", &config)
            .env("PATH", "")
            .current_dir(root.path());
        for key in ["SystemRoot", "WINDIR", "TMP", "TEMP"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let child = command.output().unwrap();
        assert!(child.status.success(), "role {role}: {child:?}");
        assert_eq!(fs::read(&config).unwrap(), text.as_bytes());
        assert_eq!(
            fs::metadata(&config).unwrap().modified().unwrap(),
            before.modified().unwrap()
        );
    }
}
