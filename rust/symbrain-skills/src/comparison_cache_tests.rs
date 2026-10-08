//! Comparison-cache validation, atomic repair and output confinement.
use super::*;

#[test]
fn comparison_cache_validates_repairs_and_preserves_atomic_boundaries() {
    let temp = tempfile::tempdir().expect("owned fixture");
    let source = temp.path().join("source");
    std::fs::create_dir(&source).expect("source directory");
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: demo\ndescription: bounded fixture\n---\n\nbody\n",
    )
    .expect("source document");
    let bundle = crate::load_bundle(&source).expect("load fixture");
    let rendered = crate::render::render_target(
        &bundle,
        "opencode",
        &crate::render::RenderMetadata::default(),
    )
    .expect("render fixture");
    let cache = temp.path().join("cache");
    let first = cached_comparison(&bundle, &rendered, &cache).expect("first render");
    let relative = first.root.strip_prefix(&cache).expect("confined render");
    let hint = cache.join(relative.with_extension("json"));
    assert!(hint.is_file());
    assert_concurrent_cache(&bundle, &rendered, &cache, &first);
    std::fs::write(first.root.join("poison"), b"must be removed").expect("poison cache");
    std::fs::write(&hint, b"untrusted hint").expect("poison hint");
    let repaired = cached_comparison(&bundle, &rendered, &cache).expect("validate full tree");
    assert_eq!(first.files, repaired.files);
    assert!(!first.root.join("poison").exists());
    let before = std::fs::read(first.root.join(".symskills.json")).expect("old marker");
    let mut changed = rendered.clone();
    changed.skill_md.extend_from_slice(b"changed\n");
    assert!(
        materialize_inner(
            &bundle,
            &changed,
            &cache,
            Some("swap-install"),
            Some((relative, "changed-hash", b"{}\n"))
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(first.root.join(".symskills.json")).expect("restored marker"),
        before
    );
    assert_eq!(
        std::fs::read(first.root.join("SKILL.md")).expect("restored render"),
        rendered.skill_md
    );
    assert!(
        materialize_inner(
            &bundle,
            &rendered,
            &cache,
            None,
            Some((Path::new("../escape"), "hash", b"{}\n"))
        )
        .is_err()
    );
    assert!(!temp.path().join("escape").exists());
    let sentinel = temp.path().join("sentinel");
    std::fs::write(&sentinel, b"preserve outside inode").expect("outside sentinel");
    std::fs::remove_file(&hint).expect("remove old hint");
    std::fs::hard_link(&sentinel, &hint).expect("hard-linked hint");
    cached_comparison(&bundle, &rendered, &cache).expect("replace link atomically");
    assert_eq!(
        std::fs::read(&sentinel).expect("outside inode"),
        b"preserve outside inode"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let lock = cache.join("status-render").join(format!(
            ".symskills-lock-{}",
            first.root.file_name().expect("key").to_string_lossy()
        ));
        assert_eq!(
            std::fs::metadata(lock)
                .expect("lock mode")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        std::fs::remove_file(&hint).expect("remove hint");
        symlink(&sentinel, &hint).expect("symlink hint");
        assert!(cached_comparison(&bundle, &rendered, &cache).is_err());
        assert_eq!(
            std::fs::read(&sentinel).expect("outside sentinel"),
            b"preserve outside inode"
        );
    }
}

fn assert_concurrent_cache(
    bundle: &Bundle,
    rendered: &Rendered,
    cache: &Path,
    first: &Materialized,
) {
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let writers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    cached_comparison(bundle, rendered, cache).expect("concurrent comparison")
                })
            })
            .collect();
        for writer in writers {
            let result = writer.join().expect("comparison writer joined");
            assert_eq!(result.files, first.files);
            assert_eq!(result.source_hash, first.source_hash);
        }
    });
    for entry in std::fs::read_dir(cache.join("status-render")).expect("comparison directory") {
        let name = entry.expect("comparison entry").file_name();
        let name = name.to_string_lossy();
        assert!(
            !name.starts_with(".symskills-stage-")
                && !name.starts_with(".symskills-backup-")
                && !name.starts_with(".symskills-sidecar-")
        );
    }
}
