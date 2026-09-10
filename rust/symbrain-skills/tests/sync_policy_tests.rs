#[test]
fn sync_default_explicitly_skips_conflicts() {
    let library = tempfile::tempdir().expect("library");
    let home = tempfile::tempdir().expect("home");
    let source = library.path().join("demo");
    std::fs::create_dir_all(&source).expect("source directory");
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: demo\ndescription: test\n---\nbase\n",
    )
    .expect("source");
    let bundle = symbrain_skills::load_bundle(&source).expect("bundle");
    let rendered = symbrain_skills::render_target(
        &bundle,
        "opencode",
        &symbrain_skills::RenderMetadata::default(),
    )
    .expect("render");
    symbrain_skills::install::install_rendered(
        &bundle,
        &rendered,
        &symbrain_skills::install::InstallOptions {
            home_dir: home.path().to_path_buf(),
            ..Default::default()
        },
    )
    .expect("install");

    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: demo\ndescription: test\n---\nlibrary change\n",
    )
    .expect("library edit");
    std::fs::write(
        home.path().join(".config/opencode/skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: test\n---\nharness change\n",
    )
    .expect("harness edit");

    let results = symbrain_skills::install::sync(&symbrain_skills::install::SyncOptions {
        home_dir: home.path().to_path_buf(),
        library_dir: library.path().to_path_buf(),
        targets: vec!["opencode".to_owned()],
        ..Default::default()
    })
    .expect("sync");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].action, "skipped");
    assert!(results[0].error.contains("conflict"));
    assert!(
        String::from_utf8(
            std::fs::read(home.path().join(".config/opencode/skills/demo/SKILL.md"))
                .expect("installed skill"),
        )
        .expect("utf8")
        .contains("harness change")
    );
}
