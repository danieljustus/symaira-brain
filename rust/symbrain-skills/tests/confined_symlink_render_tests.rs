#![cfg(any(unix, windows))]

use std::fs;
use std::path::{Path, PathBuf};

use symbrain_skills::{RenderMetadata, load_bundle, materialize, render_target};

fn link_directory(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(target, link).unwrap();
}

fn remove_directory_link(link: &Path) {
    #[cfg(unix)]
    fs::remove_file(link).unwrap();
    #[cfg(windows)]
    fs::remove_dir(link).unwrap();
}

fn source_at(parent: &Path) -> PathBuf {
    let source = parent.join("safe-skill");
    fs::create_dir_all(source.join("real/nested")).unwrap();
    fs::write(
        source.join("SKILL.md"),
        "---\nname: safe-skill\ndescription: confined assets\n---\nBody\n",
    )
    .unwrap();
    fs::write(source.join("real/nested/data.bin"), b"SAFE").unwrap();
    fs::write(source.join("real/note.md"), b"Resource markdown\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            source.join("real/nested/data.bin"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    source
}

fn rendered(bundle: &symbrain_skills::Bundle) -> symbrain_skills::Rendered {
    render_target(bundle, "opencode", &RenderMetadata::default()).unwrap()
}

#[test]
fn relative_and_absolute_in_root_directory_links_materialize_regular_files() {
    for absolute in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = source_at(temp.path());
        let target = if absolute {
            source.join("real")
        } else {
            PathBuf::from("real")
        };
        link_directory(&target, &source.join("linked"));
        let bundle = load_bundle(&source).unwrap();
        let output =
            materialize(&bundle, &rendered(&bundle), &temp.path().join("rendered")).unwrap();
        assert_eq!(
            fs::read(output.root.join("linked/nested/data.bin")).unwrap(),
            b"SAFE"
        );
        assert_eq!(
            fs::read(output.root.join("linked/note.md")).unwrap(),
            b"Resource markdown\n"
        );
        assert!(
            fs::symlink_metadata(output.root.join("linked"))
                .unwrap()
                .is_dir()
        );
        assert!(
            !fs::symlink_metadata(output.root.join("linked/nested/data.bin"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(output.root.join("linked/nested/data.bin"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o755
            );
        }
    }
}

#[test]
fn relative_link_chains_can_reach_an_absolute_in_root_directory() {
    let temp = tempfile::tempdir().unwrap();
    let source = source_at(temp.path());
    link_directory(&source.join("real"), &source.join("absolute"));
    link_directory(Path::new("absolute"), &source.join("linked"));
    let bundle = load_bundle(&source).unwrap();
    let output = materialize(&bundle, &rendered(&bundle), &temp.path().join("rendered")).unwrap();
    assert_eq!(
        fs::read(output.root.join("linked/nested/data.bin")).unwrap(),
        b"SAFE"
    );
}

#[test]
fn absolute_outside_directory_is_rejected_before_rendering() {
    let temp = tempfile::tempdir().unwrap();
    let source = source_at(temp.path());
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("data.bin"), b"EVIL").unwrap();
    link_directory(&outside, &source.join("linked"));
    assert!(load_bundle(&source).is_err());
}

#[test]
fn replacing_an_in_root_link_with_an_outside_target_preserves_old_output() {
    let temp = tempfile::tempdir().unwrap();
    let source = source_at(temp.path());
    link_directory(&source.join("real"), &source.join("linked"));
    let bundle = load_bundle(&source).unwrap();
    let render = rendered(&bundle);
    let destination = temp.path().join("rendered");
    let initial = materialize(&bundle, &render, &destination).unwrap();
    let outside = temp.path().join("outside");
    fs::create_dir_all(outside.join("nested")).unwrap();
    fs::write(outside.join("nested/data.bin"), b"EVIL").unwrap();
    fs::write(outside.join("note.md"), b"Resource markdown\n").unwrap();
    remove_directory_link(&source.join("linked"));
    link_directory(&outside, &source.join("linked"));
    assert!(materialize(&bundle, &render, &destination).is_err());
    assert_eq!(
        fs::read(initial.root.join("linked/nested/data.bin")).unwrap(),
        b"SAFE"
    );
}

#[test]
fn replacing_a_resolved_directory_with_an_outside_link_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let source = source_at(temp.path());
    link_directory(&source.join("real"), &source.join("linked"));
    let bundle = load_bundle(&source).unwrap();
    let render = rendered(&bundle);
    let outside = temp.path().join("outside");
    fs::create_dir_all(outside.join("nested")).unwrap();
    fs::write(outside.join("nested/data.bin"), b"EVIL").unwrap();
    fs::write(outside.join("note.md"), b"Resource markdown\n").unwrap();
    fs::rename(source.join("real"), source.join("previous-real")).unwrap();
    link_directory(&outside, &source.join("real"));
    assert!(materialize(&bundle, &render, &temp.path().join("rendered")).is_err());
}

#[test]
fn renamed_source_root_keeps_hashing_and_copying_the_retained_directory() {
    let temp = tempfile::tempdir().unwrap();
    let source = source_at(temp.path());
    link_directory(&source.join("real"), &source.join("linked"));
    let bundle = load_bundle(&source).unwrap();
    let render = rendered(&bundle);
    let initial = materialize(&bundle, &render, &temp.path().join("before")).unwrap();
    let rename = fs::rename(&source, temp.path().join("retained-source"));
    #[cfg(windows)]
    {
        // Cap-std deliberately denies FILE_SHARE_DELETE on directory handles:
        // do not weaken that protection to mimic Unix root replacement.
        let error = rename.unwrap_err();
        assert_eq!(error.raw_os_error(), Some(32));
        let result = materialize(&bundle, &render, &temp.path().join("after")).unwrap();
        assert_eq!(result.source_hash, initial.source_hash);
        assert_eq!(
            fs::read(result.root.join("linked/nested/data.bin")).unwrap(),
            b"SAFE"
        );
    }
    #[cfg(unix)]
    {
        rename.unwrap();
        fs::create_dir_all(source.join("real/nested")).unwrap();
        fs::write(source.join("real/nested/data.bin"), b"EVIL").unwrap();
        let result = materialize(&bundle, &render, &temp.path().join("after")).unwrap();
        assert_eq!(result.source_hash, initial.source_hash);
        assert_eq!(
            fs::read(result.root.join("linked/nested/data.bin")).unwrap(),
            b"SAFE"
        );
    }
}

#[test]
fn a_trusted_root_alias_accepts_canonical_in_root_directory_targets() {
    let temp = tempfile::tempdir().unwrap();
    let source = source_at(temp.path());
    link_directory(&source.join("real"), &source.join("linked"));
    let alias = temp.path().join("root-alias");
    link_directory(&source, &alias);
    let bundle = load_bundle(&alias).unwrap_or_else(|error| {
        panic!(
            "{error}; source={source:?}; alias={alias:?}; canonical_source={:?}; \
             canonical_alias={:?}; alias_target={:?}; resource_target={:?}",
            fs::canonicalize(&source),
            fs::canonicalize(&alias),
            fs::read_link(&alias),
            fs::read_link(source.join("linked")),
        )
    });
    let result = materialize(&bundle, &rendered(&bundle), &temp.path().join("rendered")).unwrap();
    assert_eq!(
        fs::read(result.root.join("linked/nested/data.bin")).unwrap(),
        b"SAFE"
    );
}

#[test]
fn parent_traversal_and_shared_text_prefixes_do_not_confer_root_authority() {
    for target_name in ["../outside", "../safe-skill-other"] {
        let temp = tempfile::tempdir().unwrap();
        let source = source_at(temp.path());
        let outside = source.join(target_name);
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("data.bin"), b"EVIL").unwrap();
        link_directory(&outside, &source.join("linked"));
        assert!(load_bundle(&source).is_err());
        remove_directory_link(&source.join("linked"));
        link_directory(&fs::canonicalize(&outside).unwrap(), &source.join("linked"));
        assert!(load_bundle(&source).is_err());
    }
}

#[test]
fn cyclic_directory_links_hit_a_finite_resolution_bound() {
    let temp = tempfile::tempdir().unwrap();
    let source = source_at(temp.path());
    link_directory(Path::new("b"), &source.join("a"));
    link_directory(Path::new("a"), &source.join("b"));
    let error = load_bundle(&source).unwrap_err();
    assert!(error.0.contains("maximum symlink resolution depth"));
}

#[test]
fn trusted_root_alias_preserves_parent_directory_alias_spellings() {
    let temp = tempfile::tempdir().unwrap();
    let physical_parent = temp.path().join("physical-parent");
    fs::create_dir(&physical_parent).unwrap();
    let parent_alias = temp.path().join("parent-alias");
    link_directory(&physical_parent, &parent_alias);
    let source = source_at(&parent_alias);
    link_directory(&source.join("real"), &source.join("linked"));
    let alias = parent_alias.join("root-alias");
    link_directory(&source, &alias);
    let bundle = load_bundle(&alias).unwrap();
    let output = materialize(&bundle, &rendered(&bundle), &temp.path().join("rendered")).unwrap();
    assert_eq!(
        fs::read(output.root.join("linked/nested/data.bin")).unwrap(),
        b"SAFE"
    );
}
