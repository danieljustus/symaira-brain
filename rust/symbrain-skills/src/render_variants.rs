//! Variant resolution for rendered Markdown bodies and resources.

use std::collections::BTreeMap;

use crate::model::{Bundle, SkillError};
use crate::render::VariantReport;
use crate::validation::is_render_blocking;
use crate::variant::{self, Options};

type VariantFiles = BTreeMap<String, Vec<u8>>;
type VariantResolution = (Vec<u8>, VariantFiles, Option<VariantReport>);

pub(crate) fn resolve_variants(
    bundle: &Bundle,
    target_name: &str,
    composed: &[u8],
) -> Result<VariantResolution, SkillError> {
    let options = Options {
        target: target_name.to_owned(),
        overrides: bundle
            .block_overrides
            .get(target_name)
            .cloned()
            .unwrap_or_default(),
        terms: bundle.manifest.terms.clone(),
    };
    let (body, body_result) = if let Ok(text) = std::str::from_utf8(composed) {
        let (result, problems) = variant::apply(text, &options);
        reject_blocking(&problems, "SKILL.md")?;
        (result.text.as_bytes().to_vec(), result)
    } else {
        if composed.windows(7).any(|part| part == b"{{term:")
            || composed.windows(10).any(|part| part == b"symskills:")
        {
            return Err(SkillError("invalid_utf8_variant_markdown: SKILL.md".into()));
        }
        (
            composed.to_vec(),
            variant::Result {
                source_bytes: composed.len(),
                ..Default::default()
            },
        )
    };
    let mut report = VariantReport {
        blocks: body_result.blocks.clone(),
        terms: body_result.terms.clone(),
        files: Vec::new(),
        source_bytes: body_result.source_bytes,
        replaced_bytes: body_result.replaced_bytes,
    };
    let mut files = BTreeMap::new();
    for (path, source) in &bundle.markdown {
        let changed = source.windows(7).any(|window| window == b"{{term:")
            || source.windows(10).any(|window| window == b"symskills:");
        if !changed {
            continue;
        }
        let text = String::from_utf8(source.clone())
            .map_err(|_| SkillError(format!("invalid_utf8_variant_markdown: {path}")))?;
        let (result, problems) = variant::apply(&text, &options);
        reject_blocking(&problems, path)?;
        report.source_bytes += result.source_bytes;
        report.replaced_bytes += result.replaced_bytes;
        report.blocks.extend(result.blocks);
        report.terms.extend(result.terms);
        if result.text.as_bytes() != source.as_slice() {
            report.files.push(path.clone());
            files.insert(path.clone(), result.text.into_bytes());
        }
    }
    report.blocks.sort();
    report.blocks.dedup();
    report.files.sort();
    let no_op = report.blocks.is_empty() && report.terms.is_empty() && report.files.is_empty();
    Ok((body, files, (!no_op).then_some(report)))
}

fn reject_blocking(problems: &[variant::Problem], path: &str) -> Result<(), SkillError> {
    if let Some(problem) = problems.iter().find(|problem| {
        problem.severity == variant::SEVERITY_ERROR && is_render_blocking(&problem.code)
    }) {
        return Err(SkillError(format!("{path}: {}", problem.message)));
    }
    Ok(())
}
