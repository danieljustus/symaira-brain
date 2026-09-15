//! Independent subprocess coverage for `symbrain init`.

use std::fs;
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

const DEFAULT_CONFIG_TOML: &str = include_str!("../src/init_templates/default_config.toml");
const PERSONAL_PROFILE_TOML: &str = include_str!("../src/init_templates/personal.toml");
const RESTRICTED_PROFILE_TOML: &str = include_str!("../src/init_templates/restricted.toml");
const FOREIGN_READ_ONLY_PROFILE_TOML: &str =
    include_str!("../src/init_templates/foreign-read-only.toml");

fn init_command(root: &TempDir, args: &[&str]) -> Command {
    let cwd = root.path().join("cwd");
    fs::create_dir_all(&cwd).unwrap();
    let home = root.path().join("home");
    fs::create_dir_all(&home).unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command.env_clear();
    #[cfg(windows)]
    {
        for key in [
            "SystemRoot",
            "SYSTEMROOT",
            "windir",
            "WINDIR",
            "PATHEXT",
            "ComSpec",
            "COMSPEC",
            "TEMP",
            "TMP",
            "SystemDrive",
        ] {
            if let Some(val) = std::env::var_os(key) {
                command.env(key, val);
            }
        }
    }
    command
        .current_dir(&cwd)
        .args(args)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("PATH", root.path().join("empty-path"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn run_init(root: &TempDir, args: &[&str]) -> Output {
    init_command(root, args).output().unwrap()
}

#[test]
fn fresh_init_creates_files_and_dirs_with_proper_modes() {
    let root = TempDir::new().unwrap();
    let home = root.path().join("home");
    let output = run_init(&root, &["init"]);

    assert!(
        output.status.success(),
        "stderr: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {:?}",
        output.stderr
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let config_file = home.join(".config").join("symbrain").join("config.toml");
    let personal_file = home
        .join(".config")
        .join("symbrain")
        .join("profiles")
        .join("personal.toml");
    let restricted_file = home
        .join(".config")
        .join("symbrain")
        .join("profiles")
        .join("restricted.toml");
    let foreign_file = home
        .join(".config")
        .join("symbrain")
        .join("profiles")
        .join("foreign-read-only.toml");

    let data_dir = home.join(".local").join("share").join("symbrain");
    let audit_dir = data_dir.join("audit");
    let cache_dir = home.join(".cache").join("symbrain");

    assert!(config_file.parent().unwrap().is_dir());
    assert!(personal_file.parent().unwrap().is_dir());
    assert!(data_dir.is_dir());
    assert!(audit_dir.is_dir());
    assert!(cache_dir.is_dir());

    assert!(config_file.is_file());
    assert!(personal_file.is_file());
    assert!(restricted_file.is_file());
    assert!(foreign_file.is_file());

    assert_eq!(
        fs::read_to_string(&config_file).unwrap(),
        DEFAULT_CONFIG_TOML
    );
    assert_eq!(
        fs::read_to_string(&personal_file).unwrap(),
        PERSONAL_PROFILE_TOML
    );
    assert_eq!(
        fs::read_to_string(&restricted_file).unwrap(),
        RESTRICTED_PROFILE_TOML
    );
    assert_eq!(
        fs::read_to_string(&foreign_file).unwrap(),
        FOREIGN_READ_ONLY_PROFILE_TOML
    );

    assert!(stdout.contains(&format!("created {}", config_file.display())));
    assert!(stdout.contains(&format!("created {}", personal_file.display())));
    assert!(stdout.contains(&format!("created {}", restricted_file.display())));
    assert!(stdout.contains(&format!("created {}", foreign_file.display())));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&config_file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(config_file.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn rerun_skips_existing_files_and_preserves_user_edits() {
    let root = TempDir::new().unwrap();
    let home = root.path().join("home");
    let output1 = run_init(&root, &["init"]);
    assert!(output1.status.success());

    let config_file = home.join(".config").join("symbrain").join("config.toml");
    let personal_file = home
        .join(".config")
        .join("symbrain")
        .join("profiles")
        .join("personal.toml");
    let restricted_file = home
        .join(".config")
        .join("symbrain")
        .join("profiles")
        .join("restricted.toml");
    let foreign_file = home
        .join(".config")
        .join("symbrain")
        .join("profiles")
        .join("foreign-read-only.toml");

    fs::write(&config_file, b"# user edited config\n").unwrap();
    fs::write(&personal_file, b"# user edited personal profile\n").unwrap();

    let output2 = run_init(&root, &["init"]);
    assert!(output2.status.success());
    assert!(output2.stderr.is_empty());

    let stdout2 = String::from_utf8(output2.stdout).unwrap();
    assert!(stdout2.contains(&format!(
        "skipped {} (already exists)",
        config_file.display()
    )));
    assert!(stdout2.contains(&format!(
        "skipped {} (already exists)",
        personal_file.display()
    )));
    assert!(stdout2.contains(&format!(
        "skipped {} (already exists)",
        restricted_file.display()
    )));
    assert!(stdout2.contains(&format!(
        "skipped {} (already exists)",
        foreign_file.display()
    )));

    assert_eq!(
        fs::read_to_string(&config_file).unwrap(),
        "# user edited config\n"
    );
    assert_eq!(
        fs::read_to_string(&personal_file).unwrap(),
        "# user edited personal profile\n"
    );
    assert_eq!(
        fs::read_to_string(&restricted_file).unwrap(),
        RESTRICTED_PROFILE_TOML
    );
    assert_eq!(
        fs::read_to_string(&foreign_file).unwrap(),
        FOREIGN_READ_ONLY_PROFILE_TOML
    );
}

#[test]
fn missing_home_fails_with_expected_error() {
    let root = TempDir::new().unwrap();
    let cwd = root.path().join("cwd");
    fs::create_dir_all(&cwd).unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command.env_clear();
    #[cfg(windows)]
    {
        for key in [
            "SystemRoot",
            "SYSTEMROOT",
            "windir",
            "WINDIR",
            "PATHEXT",
            "ComSpec",
            "COMSPEC",
            "TEMP",
            "TMP",
            "SystemDrive",
        ] {
            if let Some(val) = std::env::var_os(key) {
                command.env(key, val);
            }
        }
    }
    command
        .current_dir(&cwd)
        .arg("init")
        .env("PATH", root.path().join("empty-path"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    #[cfg(windows)]
    assert_eq!(stderr, "symbrain init: %userprofile% is not defined\n");
    #[cfg(not(windows))]
    assert!(stderr.contains("symbrain init: $HOME is not defined"));
}

#[test]
fn positionals_and_terminators_are_accepted() {
    let root = TempDir::new().unwrap();
    let home = root.path().join("home");
    let output = run_init(&root, &["init", "pos1", "--bogus"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("created "));
    assert!(
        home.join(".config")
            .join("symbrain")
            .join("config.toml")
            .is_file()
    );

    let root2 = TempDir::new().unwrap();
    let home2 = root2.path().join("home");
    let output2 = run_init(&root2, &["init", "--", "--bogus"]);
    assert!(output2.status.success());
    assert!(output2.stderr.is_empty());
    assert!(
        home2
            .join(".config")
            .join("symbrain")
            .join("config.toml")
            .is_file()
    );

    let root3 = TempDir::new().unwrap();
    let home3 = root3.path().join("home");
    let output3 = run_init(&root3, &["init", "-", "--bogus"]);
    assert!(output3.status.success());
    assert!(output3.stderr.is_empty());
    assert!(
        home3
            .join(".config")
            .join("symbrain")
            .join("config.toml")
            .is_file()
    );
}

#[test]
fn custom_xdg_environment_variables_honored() {
    let root = TempDir::new().unwrap();
    let custom_config = root.path().join("custom_config");
    let custom_data = root.path().join("custom_data");
    let custom_cache = root.path().join("custom_cache");

    let output = init_command(&root, &["init"])
        .env(
            "XDG_CONFIG_HOME",
            custom_config.to_string_lossy().replace('\\', "/"),
        )
        .env("XDG_DATA_HOME", &custom_data)
        .env("XDG_CACHE_HOME", &custom_cache)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let config_file = custom_config.join("symbrain").join("config.toml");
    let personal_file = custom_config
        .join("symbrain")
        .join("profiles")
        .join("personal.toml");
    let data_dir = custom_data.join("symbrain");
    let audit_dir = data_dir.join("audit");
    let cache_dir = custom_cache.join("symbrain");

    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains(&format!("created {}\n", config_file.display()))
    );
    assert!(config_file.is_file());
    assert!(personal_file.is_file());
    assert!(data_dir.is_dir());
    assert!(audit_dir.is_dir());
    assert!(cache_dir.is_dir());
}

#[cfg(unix)]
#[test]
fn dangling_symlink_is_replaced() {
    let root = TempDir::new().unwrap();
    let home = root.path().join("home");
    let profiles_dir = home.join(".config").join("symbrain").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();

    let target = profiles_dir.join("personal.toml");
    std::os::unix::fs::symlink("nonexistent.toml", &target).unwrap();
    assert!(fs::symlink_metadata(&target).unwrap().is_symlink());

    let output = run_init(&root, &["init"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(!fs::symlink_metadata(&target).unwrap().is_symlink());
    assert_eq!(fs::read_to_string(&target).unwrap(), PERSONAL_PROFILE_TOML);
}

#[cfg(unix)]
#[test]
fn symlink_to_existing_file_is_skipped() {
    let root = TempDir::new().unwrap();
    let home = root.path().join("home");
    let profiles_dir = home.join(".config").join("symbrain").join("profiles");
    fs::create_dir_all(&profiles_dir).unwrap();

    let real_file = root.path().join("real_personal.toml");
    fs::write(&real_file, b"custom content").unwrap();
    let target = profiles_dir.join("personal.toml");
    std::os::unix::fs::symlink(&real_file, &target).unwrap();

    let output = run_init(&root, &["init"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(&format!("skipped {} (already exists)", target.display())));
    assert_eq!(fs::read_to_string(&target).unwrap(), "custom content");
}

#[test]
fn native_success_with_nonexistent_symbrain_go_binary() {
    let root = TempDir::new().unwrap();
    let nonexistent_go = root.path().join("nonexistent-symbrain-go");

    let output = init_command(&root, &["init"])
        .env("SYMBRAIN_GO_BINARY", &nonexistent_go)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("created "));
}

#[test]
fn init_help_and_unknown_flags_via_subprocess() {
    let root = TempDir::new().unwrap();

    let output_help = run_init(&root, &["init", "--help"]);
    assert_eq!(output_help.status.code(), Some(2));
    assert!(output_help.stdout.is_empty());
    assert_eq!(output_help.stderr, b"Usage of init:\n");

    let output_h = run_init(&root, &["init", "-h"]);
    assert_eq!(output_h.status.code(), Some(2));
    assert!(output_h.stdout.is_empty());
    assert_eq!(output_h.stderr, b"Usage of init:\n");

    let output_bogus = run_init(&root, &["init", "--bogus"]);
    assert_eq!(output_bogus.status.code(), Some(2));
    assert!(output_bogus.stdout.is_empty());
    assert_eq!(
        output_bogus.stderr,
        b"flag provided but not defined: -bogus\nUsage of init:\n"
    );
}
