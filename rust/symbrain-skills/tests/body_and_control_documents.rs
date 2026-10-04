//! Prepared real owned file/loader/render checks; not executed during source work.
use std::fs;
use symbrain_skills::{RenderMetadata, load_bundle, materialize, render_target};

#[test]
fn ordinary_raw_markdown_body_is_preserved_through_render_and_materialization() {
    let root = tempfile::tempdir().unwrap();
    let bundle_root = root.path().join("bundle");
    fs::create_dir(&bundle_root).unwrap();
    for raw in [
        b"\xff".as_slice(),
        b"\xe2\x82",
        b"\xc0\xaf",
        "\u{fffd}".as_bytes(),
    ] {
        let mut input = b"---\nname: demo\ndescription: valid header\n---\nbody-".to_vec();
        input.extend_from_slice(raw);
        input.push(b'\n');
        fs::write(bundle_root.join("SKILL.md"), input).unwrap();
        let bundle = load_bundle(&bundle_root).unwrap();
        let item = render_target(&bundle, "opencode", &RenderMetadata::default()).unwrap();
        let mut suffix = b"body-".to_vec();
        suffix.extend_from_slice(raw);
        suffix.push(b'\n');
        assert!(item.skill_md.ends_with(&suffix));
        materialize(&bundle, &item, &root.path().join("rendered")).unwrap();
        assert_eq!(
            fs::read(root.path().join("rendered/opencode/demo/SKILL.md")).unwrap(),
            item.skill_md
        );
    }
}

#[cfg(unix)]
#[test]
fn control_links_are_refused_but_resource_links_keep_their_confined_owner() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let document = b"---\nname: demo\ndescription: source\n---\nbody\n";
    fs::write(root.path().join("document.md"), document).unwrap();
    symlink("document.md", root.path().join("SKILL.md")).unwrap();
    let error = load_bundle(root.path()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("read SKILL.md: openat SKILL.md: too many levels of symbolic links")
    );
    fs::remove_file(root.path().join("SKILL.md")).unwrap();
    fs::write(root.path().join("SKILL.md"), document).unwrap();
    fs::write(
        root.path().join("manifest.toml"),
        b"[skill]\nname = \"demo\"\n",
    )
    .unwrap();
    symlink("manifest.toml", root.path().join("symskills.toml")).unwrap();
    assert!(
        load_bundle(root.path()).unwrap_err().to_string().contains(
            "read symskills.toml: openat symskills.toml: too many levels of symbolic links"
        )
    );
    fs::remove_file(root.path().join("symskills.toml")).unwrap();
    fs::create_dir(root.path().join("assets")).unwrap();
    fs::write(root.path().join("assets/notes.md"), b"confined resource\n").unwrap();
    symlink("assets", root.path().join("linked-assets")).unwrap();
    assert!(
        load_bundle(root.path()).is_ok(),
        "confined resource-link policy is separate"
    );
}
