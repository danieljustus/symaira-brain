//! Go joins `$PWD` with `.symbrain.toml` through `filepath.Join`, which cleans
//! the result lexically. The expected text below was observed from the real Go
//! CLI (`0541e64a`, corekit v0.18.2) with each of these `PWD` spellings.
#![cfg(unix)]

use std::fs;
use std::process::Command;

#[test]
fn project_config_path_is_lexically_cleaned_like_filepath_join() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join(".symbrain.toml"), "invalid = [").unwrap();
    let root_text = root.display().to_string();
    let expected = format!(
        "project config error: failed to parse {root_text}/project/.symbrain.toml: toml: line 1 (last key \"invalid\"): unexpected EOF; expected value"
    );
    for pwd in [
        format!("{root_text}/project"),
        format!("{root_text}//project"),
        format!("{root_text}/project/../project"),
        format!("{root_text}/./project"),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
        command
            .args(["setup", "--fix", "--json"])
            .env_clear()
            .current_dir(&project)
            .env("PWD", &pwd)
            .env("HOME", root.join("home"))
            .env("USERPROFILE", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("PATH", root.join("no-tools"))
            .env("SYMBRAIN_GO_BINARY", root.join("no-go"));
        if let Some(value) = std::env::var_os("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", value);
        }
        let output = command.output().unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(output.status.code(), Some(2), "PWD={pwd}: {stderr}");
        assert!(output.stdout.is_empty(), "PWD={pwd}");
        assert!(stderr.contains(&expected), "PWD={pwd}: {stderr}");
        assert!(
            !stderr.contains("//project") && !stderr.contains("/../"),
            "PWD={pwd}: {stderr}"
        );
    }
}
