//! Portable skill bundle models, validation, and harness-specific variants.

#![deny(unsafe_code)]
#![deny(missing_docs)]

mod encode;
pub mod install;
mod load;
pub mod materialize;
pub mod model;
pub mod render;
mod render_variants;
mod target;
mod validation;
pub mod variant;

pub use load::load_bundle;
pub use materialize::{Materialized, MaterializedFile, materialize, materialize_with_fault};
pub use model::{
    Bundle, Frontmatter, Issue, MAX_BODY_LENGTH, MAX_DESCRIPTION_LENGTH, MAX_FRONTMATTER_SIZE,
    MAX_INPUT_SIZE, MAX_NAME_LENGTH, MAX_RESOURCE_DEPTH, MAX_RESOURCE_ENTRIES, MAX_RESOURCE_SIZE,
    MAX_TOTAL_RESOURCE_BYTES, Manifest, ManifestSkill, ParsedSkill, Resource, SkillError,
    TargetConfig, parse_skill_md, validate_skill_name,
};
pub use render::{
    CapabilityGap, RenderMetadata, Rendered, VariantReport, default_targets, render_target,
};
pub use validation::{DEFAULT_TARGETS, is_render_blocking, validate, validate_with_targets};
