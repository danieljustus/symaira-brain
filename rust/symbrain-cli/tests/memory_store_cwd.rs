//! A failing working-directory lookup stops store creation before any directory
//! or database exists (#935): relative input must never be resolved from `/`.

#![cfg(unix)]

use std::path::Path;
use std::process::Command;

/// Runs `symbrain memory <verb> --db <database>` from a directory that is
/// removed while the child's shell is still inside it, with no `PWD`.
fn run_from_deleted_cwd(root: &Path, verb: &[&str], database: &str) -> (i32, String, String) {
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(
            "export PATH=/bin:/usr/bin; mkdir gone && cd gone && rmdir ../gone && unset PWD \
             && PATH= && exec \"$0\" \"$@\"",
        )
        .arg(env!("CARGO_BIN_EXE_symbrain"))
        .arg("memory")
        .args(verb)
        .args(["--db", database])
        .env_clear()
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("PATH", "")
        .current_dir(root)
        .output()
        .expect("actual CLI child");
    (
        output.status.code().expect("exit status"),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn entries(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root)
        .expect("root")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn unresolvable_cwd_fails_before_directory_or_database_creation() {
    let guard = tempfile::tempdir().expect("root");
    let root = guard.path().canonicalize().expect("canonical root");
    let before = entries(&root);
    let set = ["set", "hello", "--kind", "fact", "--staged"];
    for verb in [&["list"][..], &["delete", "a"], &set] {
        for database in ["rel/sub/c.db", "c.db"] {
            let (code, stdout, stderr) = run_from_deleted_cwd(&root, verb, database);
            let name = verb[0];
            assert_eq!(code, 1, "{name} {database}: {stderr}");
            assert_eq!(stdout, "", "{name} {database}");
            assert_eq!(
                stderr,
                format!(
                    "symbrain memory {name}: open memory database: failed to open sqlite database: failed to create database directory: getwd: no such file or directory\n"
                ),
                "{name} {database}"
            );
            // Neither the fixture root nor the filesystem root received anything.
            assert_eq!(entries(&root), before, "{name} {database}");
            assert!(!Path::new("/rel").exists() && !Path::new("/c.db").exists());
        }
    }
}

#[test]
fn resolvable_cwd_without_pwd_still_creates_nested_and_bare_stores() {
    for database in ["rel/sub/c.db", "c.db"] {
        let guard = tempfile::tempdir().expect("root");
        let root = guard.path().canonicalize().expect("canonical root");
        let output = Command::new(env!("CARGO_BIN_EXE_symbrain"))
            .args(["memory", "list", "--db", database])
            .env_clear()
            .env("HOME", &root)
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("PATH", "")
            .current_dir(&root)
            .output()
            .expect("native CLI child");
        assert!(
            output.status.success(),
            "{database}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(root.join(database).is_file(), "{database}");
    }
}
