//! `symbrain skills render [skill] [--profile P]`.

// Short report strings: building them with `format!` pieces is clearer than `write!`.
#![allow(clippy::format_push_string)]

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use serde::Serialize;
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_skills::{Frontmatter, Rendered, load_bundle, materialize, render_target};

use super::env::Env;
use super::opargs::{self, Spec};
use super::{cleaned, finish, json_line, ops};

const SPEC: Spec = Spec {
    verb: "render",
    bools: &["explain", "ignore-capabilities"],
    values: &["target", "output", "profile"],
    shorts: &[('o', "output")],
    usage: "Usage: symbrain skills render [skill-dir|library-skill] [flags]\n\n\
Flags:\n\
  --profile NAME         render every skill of a context profile\n\
  --target all|a,b       harness targets (default all enabled)\n\
  --explain              report the harness-specific blocks resolved\n\
  --ignore-capabilities  render despite unsupported target capabilities\n\
  -o, --output DIR       render output directory\n\n\
Render a skill (or a context profile) for the selected harness targets into the\n\
render directory (or -o DIR) without installing it.\n",
};

#[derive(Serialize)]
struct Item<'a> {
    target: &'a str,
    name: &'a str,
    path: String,
    frontmatter: &'a Frontmatter,
    skill_md: String,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    warnings: &'a [String],
}

pub(super) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let parsed = match opargs::parse(&SPEC, args, stdout, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let env = Env::load();
    let targets = match ops::targets_of(parsed.value("target"), &env) {
        Ok(targets) => targets,
        Err(message) => return ops::usage(stderr, "render", message),
    };
    if parsed.value("profile").is_none() && parsed.positional.is_empty() {
        return ops::usage(stderr, "render", "skill-dir or --profile is required");
    }
    let jobs = match ops::jobs(&parsed, &env, parsed.flag("ignore-capabilities")) {
        Ok(jobs) => jobs,
        Err(message) => return ops::fail(stderr, "render", message),
    };
    let out_root = parsed.value("output").map_or_else(
        || cleaned(&env.cfg.render_dir),
        |dir| env.project_dir.join(dir),
    );
    let mut lines = String::new();
    let mut items: Vec<(Rendered, PathBuf)> = Vec::new();
    for job in &jobs {
        let bundle = match load_bundle(&job.path) {
            Ok(bundle) => bundle,
            Err(error) => return ops::fail(stderr, "render", format!("load skill: {error}")),
        };
        for target in &targets {
            let rendered = match render_target(&bundle, target, &job.metadata) {
                Ok(rendered) => rendered,
                Err(error) => {
                    return ops::fail(stderr, "render", format!("target {target}: {error}"));
                }
            };
            let tree = match materialize(&bundle, &rendered, &out_root) {
                Ok(tree) => tree,
                Err(error) => return ops::fail(stderr, "render", format!("write render: {error}")),
            };
            ops::record(&env, "render", &rendered.name, target, &out_root, "");
            for warning in &rendered.warnings {
                let _ = writeln!(stderr, "warning: {warning}");
            }
            lines.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                rendered.target,
                rendered.name,
                rendered.profile,
                tree.root.display()
            ));
            if parsed.flag("explain") {
                lines.push_str(&explain(&rendered));
            }
            items.push((rendered, tree.root));
        }
    }
    let output = match format {
        OutputFormat::Json => {
            let rows: Vec<Item<'_>> = items
                .iter()
                .map(|(rendered, root)| Item {
                    target: &rendered.target,
                    name: &rendered.name,
                    path: root.to_string_lossy().into_owned(),
                    frontmatter: &rendered.frontmatter,
                    skill_md: String::from_utf8_lossy(&rendered.skill_md).into_owned(),
                    warnings: &rendered.warnings,
                })
                .collect();
            json_line(&rows)
        }
        OutputFormat::Table => lines.into_bytes(),
    };
    let code = finish("render", &output, stdout, stderr);
    if code == exit::OK { exit::OK } else { code }
}

fn explain(rendered: &Rendered) -> String {
    match &rendered.variants {
        None => "  no harness-specific variants; canonical text rendered verbatim\n".to_owned(),
        Some(report) => {
            let mut text = String::new();
            for block in &report.blocks {
                text.push_str(&format!("  block {block}\n"));
            }
            for (term, value) in &report.terms {
                text.push_str(&format!("  term {term} -> {value}\n"));
            }
            for file in &report.files {
                text.push_str(&format!("  file {file}\n"));
            }
            text
        }
    }
}
