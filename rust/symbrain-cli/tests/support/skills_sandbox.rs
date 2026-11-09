//! Disposable HOME/XDG sandbox shared by the `symbrain skills` operation tests.

#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

#[path = "../../../test-support/coverage.rs"]
pub mod coverage;

pub struct Sandbox {
    _dir: TempDir,
    pub root: PathBuf,
}

pub struct Run {
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl Run {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.out).unwrap_or_else(|e| panic!("json {e}: {}", self.out))
    }
}

impl Sandbox {
    pub fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for sub in ["home", "config", "data", "cache", "project", "src"] {
            fs::create_dir_all(root.join(sub)).unwrap();
        }
        Self { _dir: dir, root }
    }

    pub fn skills(&self, args: &[&str]) -> Run {
        self.skills_with(&[], args)
    }

    /// Runs `symbrain skills` with extra environment variables.
    pub fn skills_with(&self, env: &[(&str, &str)], args: &[&str]) -> Run {
        let output = Command::new(env!("CARGO_BIN_EXE_symbrain"))
            .env_clear()
            .envs(std::env::var_os("LLVM_PROFILE_FILE").map(|path| ("LLVM_PROFILE_FILE", path)))
            .envs(coverage::profile_environment())
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("PATH", "/usr/bin:/bin:/opt/homebrew/bin")
            .envs(env.iter().copied())
            .current_dir(self.root.join("project"))
            .arg("skills")
            .args(args)
            .output()
            .unwrap();
        Run {
            code: output.status.code().unwrap(),
            out: String::from_utf8_lossy(&output.stdout).into_owned(),
            err: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// Writes the global symskills config file.
    pub fn global_config(&self, toml: &str) {
        let dir = self.root.join("config/symskills");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.toml"), toml).unwrap();
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    pub fn library(&self, relative: &str) -> PathBuf {
        self.root
            .join("data/symbrain/skills/library")
            .join(relative)
    }

    pub fn source(&self, name: &str, body: &str) -> PathBuf {
        let dir = self.root.join("src").join(name);
        fs::create_dir_all(dir.join("references")).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Test skill\ncategory: dev\n---\n{body}\n"),
        )
        .unwrap();
        fs::write(dir.join("references/r.md"), "ref\n").unwrap();
        dir
    }
}

pub fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}
