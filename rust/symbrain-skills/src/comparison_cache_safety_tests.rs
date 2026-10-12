//! Shared cache-operation confinement and non-authoritative sidecar failure.
use super::*;
use std::os::unix::fs::symlink;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn comparison_capabilities_confine_lock_file_and_sidecar_after_ancestor_swap() {
    // Exercise actual shared filesystem operations after capability capture
    // and parent validation, not a substitute for the complete native gate.
    for swap_root in [true, false] {
        let temp = tempfile::tempdir().expect("owned fixture");
        let cache = temp.path().join("cache");
        let worker_cache = cache.clone();
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("sentinel"), b"protected bytes").unwrap();
        let relative = Path::new("status-render/key");
        let (ready_tx, ready_rx) = mpsc::sync_channel(0);
        let (resume_tx, resume_rx) = mpsc::sync_channel(0);
        std::thread::scope(|scope| {
            let worker = scope.spawn(move || -> Result<(), SkillError> {
                let root = open_root(&worker_cache).map_err(|error| io_error(&error))?;
                let parent = relative.parent().unwrap();
                ensure_dir(&root, parent).map_err(|error| io_error(&error))?;
                ready_tx
                    .send(())
                    .map_err(|error| SkillError(error.to_string()))?;
                resume_rx
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|error| SkillError(error.to_string()))?;
                let _lock =
                    lock_destination(&root, parent, relative).map_err(|error| io_error(&error))?;
                write_file(
                    &root,
                    &relative.join("SKILL.md"),
                    b"derived bytes",
                    0o644,
                    None,
                )
                .map_err(|error| io_error(&error))?;
                write_comparison_sidecar(&root, &relative.with_extension("json"), b"{}\n")
            });
            // Drop the resume sender before joining on any setup failure.
            let setup = (|| -> io::Result<()> {
                ready_rx
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(io::Error::other)?;
                let replaced = if swap_root {
                    cache.clone()
                } else {
                    cache.join("status-render")
                };
                std::fs::rename(&replaced, temp.path().join("retained"))?;
                symlink(&outside, &replaced)?;
                resume_tx.send(()).map_err(io::Error::other)
            })();
            drop(resume_tx);
            let result = worker.join().expect("worker joined");
            setup.expect("synchronized ancestor replacement");
            assert_eq!(
                result.is_ok(),
                swap_root,
                "retained root versus replaced child"
            );
        });
        assert_eq!(
            std::fs::read(outside.join("sentinel")).unwrap(),
            b"protected bytes"
        );
        let names: Vec<_> = std::fs::read_dir(&outside)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("sentinel")]);
        if swap_root {
            let retained = temp.path().join("retained");
            assert_eq!(
                std::fs::read(retained.join(relative).join("SKILL.md")).unwrap(),
                b"derived bytes"
            );
            assert_eq!(
                std::fs::read(retained.join(relative.with_extension("json"))).unwrap(),
                b"{}\n"
            );
        }
    }
}

#[test]
fn sidecar_failure_reports_error_without_rolling_back_valid_derived_tree() {
    let temp = tempfile::tempdir().expect("owned fixture");
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    let original = b"---\nname: demo\ndescription: bounded fixture\n---\n\nbody\n";
    std::fs::write(source.join("SKILL.md"), original).unwrap();
    let bundle = crate::load_bundle(&source).unwrap();
    let rendered = crate::render::render_target(
        &bundle,
        "opencode",
        &crate::render::RenderMetadata::default(),
    )
    .unwrap();
    let cache = temp.path().join("cache");
    let first = cached_comparison(&bundle, &rendered, &cache).unwrap();
    let hint = first.root.with_extension("json");
    let sentinel = temp.path().join("sentinel");
    std::fs::write(&sentinel, b"protected bytes").unwrap();
    std::fs::remove_file(&hint).unwrap();
    symlink(&sentinel, &hint).unwrap();
    let mut changed = rendered.clone();
    changed.skill_md.extend_from_slice(b"changed\n");
    let error = cached_comparison(&bundle, &changed, &cache).unwrap_err();
    assert!(
        error
            .0
            .contains("comparison cache sidecar is not a regular file")
    );
    assert_eq!(
        std::fs::read(first.root.join("SKILL.md")).unwrap(),
        changed.skill_md
    );
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"protected bytes");
    assert_eq!(std::fs::read(source.join("SKILL.md")).unwrap(), original);
    assert_eq!(std::fs::read_link(&hint).unwrap(), sentinel);
    std::fs::remove_file(&hint).unwrap();
    let retry = cached_comparison(&bundle, &changed, &cache).unwrap();
    assert_ne!(retry.source_hash, first.source_hash);
    assert_eq!(
        std::fs::read(retry.root.join("SKILL.md")).unwrap(),
        changed.skill_md
    );
    let metadata = std::fs::symlink_metadata(&hint).unwrap();
    assert!(metadata.is_file() && !metadata.file_type().is_symlink());
    let hint: serde_json::Value = serde_json::from_slice(&std::fs::read(&hint).unwrap()).unwrap();
    assert_eq!(hint["target"], rendered.target);
    assert_eq!(hint["name"], rendered.name);
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"protected bytes");
    assert_eq!(std::fs::read(source.join("SKILL.md")).unwrap(), original);
}
