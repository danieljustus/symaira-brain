use std::env;
use std::path::{Path, PathBuf};

use serde_json::Value;
use symbrain_skills::{load_bundle, validate};

#[path = "common/mod.rs"]
mod common;

fn relative_root_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/skills-oracle/fixtures/relative-root")
}

#[test]
fn dot_root_keeps_go_absolute_path_and_validation_diagnostic() {
    let oracle: Value = serde_json::from_slice(&common::skills_oracle()).expect("Go oracle");
    let expected = &oracle["relative_root"];
    assert_eq!(expected["root_is_absolute"], true);

    let original = env::current_dir().expect("current directory");
    env::set_current_dir(relative_root_fixture()).expect("enter relative-root fixture");
    let loaded = load_bundle(Path::new("."));
    env::set_current_dir(original).expect("restore current directory");
    let bundle = loaded.expect("load bundle with relative dot root");

    assert!(
        bundle.root.is_absolute(),
        "root should be absolute: {:?}",
        bundle.root
    );
    assert_eq!(
        serde_json::to_value(validate(&bundle)).expect("serialize Rust validation"),
        expected["issues"],
        "relative-root validation should match Go's exact diagnostic"
    );
}
