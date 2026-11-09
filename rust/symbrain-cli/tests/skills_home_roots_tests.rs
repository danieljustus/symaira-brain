//! HOME-derived skill roots at the CLI boundary (#924).
//!
//! Go was removed, so these are Rust behavior tests: a root below a search-only
//! ancestor reads as "No installed skills found" (exit 0) and an unset HOME is
//! a clear error for both `status` and `sync`.

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::path::Path;
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

fn run(root: &Path, home: Option<&Path>, args: &[&str]) -> Output {
    for name in ["config", "data", "cache", "project"] {
        std::fs::create_dir_all(root.join(name)).unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .envs(coverage::profile_environment())
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C.UTF-8")
        .current_dir(root.join("project"))
        .arg("skills")
        .args(args)
        .stdin(Stdio::null());
    if let Some(home) = home {
        command.env("HOME", home);
    }
    command.output().unwrap()
}

#[test]
fn status_below_search_only_ancestor_reports_no_installed_skills() {
    use std::os::unix::fs::PermissionsExt;

    for mode in [0o311, 0o111] {
        for extra in [&[][..], &["--target", "opencode"][..]] {
            let temp = TempDir::new().unwrap();
            let ancestor = temp.path().join("ancestor");
            let home = ancestor.join("home");
            std::fs::create_dir_all(home.join(".claude/skills")).unwrap();
            std::fs::create_dir_all(home.join(".config/opencode/skills")).unwrap();
            std::fs::set_permissions(&ancestor, std::fs::Permissions::from_mode(mode)).unwrap();
            let denied = std::fs::read_dir(&ancestor).is_err();
            let mut args = vec!["status"];
            args.extend(extra);
            let output = run(temp.path(), Some(&home), &args);
            std::fs::set_permissions(&ancestor, std::fs::Permissions::from_mode(0o755)).unwrap();
            if !denied {
                continue; // privileged runner: permissions are not enforced
            }
            let label = format!("mode {mode:o} {args:?}");
            assert_eq!(
                output.status.code(),
                Some(0),
                "{label}: {:?}",
                output.stderr
            );
            assert_eq!(output.stdout, b"No installed skills found.\n", "{label}");
            assert!(output.stderr.is_empty(), "{label}");
        }
    }
}

#[test]
fn unset_home_is_a_clear_error_for_status_and_sync() {
    for args in [
        &["status"][..],
        &["status", "--target", "opencode"][..],
        &["sync", "--dry-run"][..],
        &["sync"][..],
    ] {
        let temp = TempDir::new().unwrap();
        let output = run(temp.path(), None, args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {stderr}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(
            stderr.contains("scan installs: $HOME is not defined"),
            "{args:?}: {stderr}"
        );
    }
}
