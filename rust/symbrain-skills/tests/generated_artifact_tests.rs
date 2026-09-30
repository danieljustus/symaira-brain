//! #463: generated caches are reported by validation with their exact path.

use std::fs;

use symbrain_skills::{load_bundle, validate};

#[test]
fn generated_cache_resources_are_reported_and_clean_skills_are_not() {
    let root = tempfile::tempdir().expect("tempdir");
    let dir = root.path().join("cache-skill");
    fs::create_dir_all(dir.join("scripts")).expect("mkdir");
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: cache-skill\ndescription: test\n---\nBody\n",
    )
    .expect("skill");
    fs::write(dir.join("scripts/example.py"), "print('hi')\n").expect("script");

    let clean = validate(&load_bundle(&dir).expect("load"));
    assert!(
        clean
            .iter()
            .all(|issue| issue.code != "resource_generated_artifact"),
        "{clean:?}"
    );

    fs::create_dir_all(dir.join("scripts/__pycache__")).expect("mkdir");
    fs::write(
        dir.join("scripts/__pycache__/example.cpython-314.pyc"),
        b"\0bytecode",
    )
    .expect("pyc");
    fs::write(dir.join(".DS_Store"), b"x").expect("ds_store");
    let issues = validate(&load_bundle(&dir).expect("load"));
    let mut paths: Vec<_> = issues
        .iter()
        .filter(|issue| issue.code == "resource_generated_artifact")
        .inspect(|issue| assert_eq!(issue.severity, "warning"))
        .map(|issue| issue.path.as_str())
        .collect();
    paths.sort_unstable();
    assert_eq!(
        paths,
        [".DS_Store", "scripts/__pycache__/example.cpython-314.pyc"]
    );
}
