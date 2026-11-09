//! Logical `PWD` handling of relative database paths (#928, #936).
//!
//! A valid logical `PWD` (absolute and naming the current directory, in any
//! equivalent spelling) is kept, so a symlinked spelling is still refused by the
//! directory walk; an absent, relative or unrelated `PWD` falls back to the
//! physical working directory. Expected behavior was observed against the Go
//! process before its removal (#1003); scenarios the runner's privileges cannot
//! exercise are reported through `store_support::skip_privileged`, never passed
//! silently.

mod store_support;

use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;
use store_support::{CHILD_TIMEOUT, executed, isolate, output_within, skip_privileged, temp};

/// Every `PWD` spelling of the two links under test, plus the fall-back cases.
/// Trailing separators and dot segments name the same directory.
const SPELLINGS: [&str; 11] = [
    "symlink",
    "symlink-trailing-slash",
    "symlink-dot-segment",
    "symlink-dotdot-segment",
    "physical",
    "physical-trailing-slash",
    "physical-dot-segment",
    "physical-dotdot-segment",
    "wrong-directory",
    "relative",
    "unset",
];

const DATABASES: [&str; 2] = ["rel/sub/c.db", "c.db"];

fn pwd(spelling: &str, root: &Path) -> Option<String> {
    let (real, link) = (root.join("real"), root.join("link"));
    let text = |path: &Path| path.display().to_string();
    match spelling {
        "symlink" => Some(text(&link)),
        "symlink-trailing-slash" => Some(format!("{}/", text(&link))),
        "symlink-dot-segment" => Some(format!("{}/./", text(&link))),
        "symlink-dotdot-segment" => Some(format!("{}/../link", text(&link))),
        "physical" => Some(text(&real)),
        "physical-trailing-slash" => Some(format!("{}/", text(&real))),
        "physical-dot-segment" => Some(format!("{}/./", text(&real))),
        "physical-dotdot-segment" => Some(format!("{}/../real", text(&real))),
        "wrong-directory" => Some(text(root)),
        "relative" => Some("link".into()),
        _ => None,
    }
}

#[test]
fn relative_database_path_honours_only_pwd_naming_the_current_directory() {
    for args in [
        &["set", "hello", "--kind", "fact", "--staged"][..],
        &["delete", "absent"][..],
    ] {
        for spelling in SPELLINGS {
            for database in DATABASES {
                let verb = args[0];
                let scenario = format!("logical_pwd.{spelling}.{verb}.{database}");
                let (_guard, root) = temp();
                let real = root.join("real");
                let link = root.join("link");
                std::fs::create_dir(&real).unwrap();
                std::os::unix::fs::symlink(&real, &link).unwrap();
                let refused = spelling.starts_with("symlink");
                if refused && std::fs::symlink_metadata(&link).unwrap().uid() == 0 {
                    skip_privileged(&scenario, "root-owned links are trusted, not rejected");
                    continue;
                }
                let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
                command
                    .arg("memory")
                    .args(args)
                    .args(["--db", database])
                    .current_dir(&real);
                isolate(&mut command, &root);
                if let Some(value) = pwd(spelling, &root) {
                    command.env("PWD", value);
                }
                let output = output_within(&mut command, CHILD_TIMEOUT);
                let stderr = String::from_utf8(output.stderr).unwrap();
                if refused {
                    assert_eq!(output.status.code(), Some(1), "{scenario}: {stderr}");
                    assert!(output.stdout.is_empty(), "{scenario}");
                    assert_eq!(
                        stderr,
                        format!(
                            "symbrain memory {verb}: open memory database: failed to open sqlite database: failed to create database directory: mkdir {}: too many levels of symbolic links\n",
                            link.display()
                        ),
                        "{scenario}"
                    );
                    assert!(
                        !real.join("rel").exists() && !real.join("c.db").exists(),
                        "{scenario}: rejection must not create anything"
                    );
                } else {
                    assert!(real.join(database).is_file(), "{scenario}: {stderr}");
                    if verb == "set" {
                        assert!(output.status.success(), "{scenario}: {stderr}");
                        assert!(stderr.is_empty(), "{scenario}: {stderr}");
                    } else {
                        assert_eq!(output.status.code(), Some(1), "{scenario}");
                        assert_eq!(
                            stderr, "symbrain memory delete: memory not found: absent\n",
                            "{scenario}"
                        );
                    }
                }
                executed(&scenario);
            }
        }
    }
}
