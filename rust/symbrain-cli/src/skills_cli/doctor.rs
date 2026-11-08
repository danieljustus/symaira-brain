//! `symbrain skills doctor` (Go `cmdSkillsDoctor`).

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use symbrain_core::output::OutputFormat;
use symbrain_skills::gotext::join;

use super::env::Env;
use super::flags::{self, Verb};
use super::{finish, json_line, or_dash, serialize_path};

#[derive(Debug, Serialize)]
struct Vcs {
    enabled: bool,
}

#[derive(Debug, Serialize)]
struct Config<'a> {
    #[serde(serialize_with = "serialize_path")]
    library_dir: &'a Path,
    #[serde(serialize_with = "serialize_path")]
    render_dir: &'a Path,
    #[serde(serialize_with = "serialize_path")]
    cache_dir: &'a Path,
    #[serde(serialize_with = "serialize_path")]
    profiles_dir: &'a Path,
    #[serde(serialize_with = "serialize_path")]
    base_dir: &'a Path,
    #[serde(rename = "Targets")]
    targets: Option<()>,
    vcs: Vcs,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_field_names)]
struct Target {
    target: String,
    #[serde(serialize_with = "serialize_path")]
    user: PathBuf,
    #[serde(serialize_with = "serialize_path")]
    project: PathBuf,
}

#[derive(Debug, Serialize)]
struct Report<'a> {
    config: Config<'a>,
    #[serde(serialize_with = "serialize_path")]
    config_path: &'a Path,
    #[serde(serialize_with = "serialize_path")]
    log_path: &'a Path,
    #[serde(serialize_with = "serialize_path")]
    profiles_dir: &'a Path,
    #[serde(serialize_with = "serialize_path")]
    project_dir: &'a Path,
    targets: Vec<Target>,
}

/// Registry skill-root suffixes below the home (user) and project directory.
fn roots(target: &str) -> Option<(&'static str, &'static str)> {
    Some(match target {
        "claude" => (".claude/skills", ".claude/skills"),
        "opencode" => (".config/opencode/skills", ".opencode/skills"),
        "codex" => (".agents/skills", ".agents/skills"),
        "antigravity" => (".gemini/config/skills", ".agents/skills"),
        "hermes" => (".hermes/skills/symaira", ".hermes/skills"),
        "openclaw" => (".openclaw/skills", ".agents/skills"),
        _ => return None,
    })
}

pub(super) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    if let Err(code) = flags::parse(Verb::Doctor, args, stderr) {
        return code;
    }
    let env = Env::load();
    let targets = symbrain_skills::enabled_target_names(&env.disabled_targets)
        .into_iter()
        .filter_map(|target| {
            let (user, project) = roots(&target)?;
            Some(Target {
                user: join(&env.home_dir, user),
                project: join(&env.project_dir, project),
                target,
            })
        })
        .collect();
    let cfg = &env.cfg;
    let report = Report {
        config: Config {
            library_dir: &cfg.library_dir,
            render_dir: &cfg.render_dir,
            cache_dir: &cfg.cache_dir,
            profiles_dir: &cfg.profiles_dir,
            base_dir: &cfg.base_dir,
            targets: None,
            vcs: Vcs {
                enabled: cfg.vcs_enabled,
            },
        },
        config_path: &env.config_path,
        log_path: &env.log_path,
        profiles_dir: &cfg.profiles_dir,
        project_dir: &env.project_dir,
        targets,
    };
    let output = match format {
        OutputFormat::Json => json_line(&report),
        OutputFormat::Table => {
            let vcs = cfg.vcs_enabled.to_string();
            let mut out = Vec::new();
            for (name, value) in [
                ("config", env.config_path.as_os_str()),
                ("library", cfg.library_dir.as_os_str()),
                ("rendered", cfg.render_dir.as_os_str()),
                ("cache", cfg.cache_dir.as_os_str()),
                ("base", cfg.base_dir.as_os_str()),
                ("profiles", cfg.profiles_dir.as_os_str()),
                ("log", env.log_path.as_os_str()),
                ("project", env.project_dir.as_os_str()),
                ("versioning", OsStr::new(&vcs)),
            ] {
                out.extend_from_slice(format!("{name:<11} ").as_bytes());
                let bytes = value.as_encoded_bytes();
                if std::str::from_utf8(bytes).is_ok_and(|text| or_dash(text) == "-") {
                    out.push(b'-');
                } else {
                    out.extend_from_slice(bytes);
                }
                out.push(b'\n');
            }
            out
        }
    };
    finish("doctor", &output, stdout, stderr)
}
