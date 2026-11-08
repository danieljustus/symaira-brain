//! `symbrain skills import <dir> [--batch] [--update]`.

// Short report strings: building them with `format!` pieces is clearer than `write!`.
#![allow(clippy::format_push_string)]

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

use serde::Serialize;
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_skills::import::{BatchResult, ImportOptions, import_skill, import_skills};

use super::env::Env;
use super::opargs::{self, Spec};
use super::{cleaned, finish, json_line, ops};

const SPEC: Spec = Spec {
    verb: "import",
    bools: &["batch", "update"],
    values: &["library"],
    shorts: &[],
    usage: "Usage: symbrain skills import <skill-dir> [--batch] [--update] [--library DIR] [--json]\n\n\
Import a skill directory (containing SKILL.md) into the library. With --batch,\n\
import every immediate subdirectory that holds a SKILL.md. Each skill is\n\
versioned in its own git repository; --update re-imports over an existing\n\
library skill and records a new commit. Existing skills are never overwritten\n\
without --update.\n",
};

#[derive(Serialize)]
struct Single<'a> {
    name: &'a str,
    path: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    vcs_warning: &'a str,
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
    let Some(source) = parsed.positional.first() else {
        return ops::usage(stderr, "import", "skill-dir is required");
    };
    let env = Env::load();
    let library = parsed.value("library").map_or_else(
        || cleaned(&env.cfg.library_dir),
        |dir| Path::new(dir).to_path_buf(),
    );
    let source = env.project_dir.join(source);
    let options = ImportOptions {
        vcs_enabled: env.cfg.vcs_enabled,
        update: parsed.flag("update"),
    };
    if parsed.flag("batch") {
        return batch(&env, &source, &library, options, stdout, stderr, format);
    }
    let result = import_skill(&source, &library, options);
    let error = result
        .as_ref()
        .err()
        .map_or_else(String::new, |e| e.0.clone());
    let name = result.as_ref().map(|r| r.name.clone()).unwrap_or_default();
    ops::record(&env, "import", &name, "", &library, &error);
    let imported = match result {
        Ok(imported) => imported,
        Err(error) => return ops::fail(stderr, "import", format!("import skill: {error}")),
    };
    if !imported.vcs_warning.is_empty() {
        let _ = writeln!(stderr, "warning: {}", imported.vcs_warning);
    }
    let output = match format {
        OutputFormat::Json => json_line(&Single {
            name: &imported.name,
            path: imported.path.to_string_lossy().into_owned(),
            vcs_warning: &imported.vcs_warning,
        }),
        OutputFormat::Table => {
            let verb = if imported.updated {
                "Updated"
            } else {
                "Imported"
            };
            format!("{verb} {} to {}\n", imported.name, imported.path.display()).into_bytes()
        }
    };
    finish("import", &output, stdout, stderr)
}

fn batch(
    env: &Env,
    source: &Path,
    library: &Path,
    options: ImportOptions,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let results = import_skills(source, library, options);
    for item in &results {
        ops::record(env, "import", &item.name, "", library, &item.error);
        if !item.vcs_warning.is_empty() {
            let _ = writeln!(stderr, "warning: {}", item.vcs_warning);
        }
    }
    let failed = results
        .iter()
        .filter(|item| item.status == "failed")
        .count();
    let output = match format {
        OutputFormat::Json => json_line(&results),
        OutputFormat::Table => batch_table(&results),
    };
    let code = finish("import", &output, stdout, stderr);
    if code == exit::OK && failed > 0 {
        return exit::GENERIC;
    }
    code
}

fn batch_table(results: &[BatchResult]) -> Vec<u8> {
    if results.is_empty() {
        return b"No skills found (looked for subdirectories containing SKILL.md).\n".to_vec();
    }
    let mut text = String::new();
    let mut imported = 0;
    let mut failed = 0;
    for item in results {
        if item.status == "failed" {
            failed += 1;
            text.push_str(&format!("Failed {}: {}\n", item.name, item.error));
        } else {
            imported += 1;
            text.push_str(&format!("Imported {} to {}\n", item.name, item.path));
        }
    }
    text.push_str(&format!(
        "\nSummary: {imported} imported, {failed} failed\n"
    ));
    text.into_bytes()
}
