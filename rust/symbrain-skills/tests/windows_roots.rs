#![cfg(windows)]

use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::Path;
use std::process::Command;

use symbrain_skills::install::{InstallOptions, install_rendered};
use symbrain_skills::{RenderMetadata, load_bundle, materialize, render_target};

fn rendered_source(root: &Path) -> (symbrain_skills::Bundle, symbrain_skills::Rendered) {
    fs::create_dir_all(root).expect("skill directory");
    fs::write(
        root.join("SKILL.md"),
        b"---\nname: windows-root\ndescription: native install test\n---\nBody.\n",
    )
    .expect("source skill");
    let bundle = load_bundle(root).expect("bundle");
    let rendered = render_target(&bundle, "opencode", &RenderMetadata::default()).expect("render");
    (bundle, rendered)
}

fn junction(link: &Path, target: &Path) {
    let output = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .expect("run mklink");
    assert!(
        output.status.success(),
        "mklink /J failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_ne!(
        fs::symlink_metadata(link)
            .expect("junction metadata")
            .file_attributes()
            & 0x400,
        0,
        "expected a Windows reparse point"
    );
}

#[test]
fn drive_root_materializes_and_installs_real_skill() {
    let temp = tempfile::tempdir().expect("root");
    let (bundle, rendered) = rendered_source(&temp.path().join("library/windows-root"));
    let output = temp.path().join("render");
    let tree = materialize(&bundle, &rendered, &output).expect("materialize on drive");
    assert_eq!(
        fs::read(tree.root.join("SKILL.md")).expect("rendered bytes"),
        rendered.skill_md
    );

    let home = temp.path().join("home");
    let result = install_rendered(
        &bundle,
        &rendered,
        &InstallOptions {
            home_dir: home.clone(),
            mode: "copy".to_owned(),
            ..Default::default()
        },
    )
    .expect("install on drive");
    assert_eq!(result.action, "installed");
    assert_eq!(
        fs::read(result.path.join("SKILL.md")).expect("installed bytes"),
        rendered.skill_md
    );
    assert!(result.path.join(".symskills.json").is_file());
    assert!(
        home.join(".local/share/symskills/base/opencode/windows-root/manifest.json")
            .is_file(),
        "base snapshot must be installed too"
    );
}

#[test]
fn junction_ancestors_are_refused_without_touching_outside() {
    let temp = tempfile::tempdir().expect("root");
    let (bundle, rendered) = rendered_source(&temp.path().join("library/windows-root"));
    let outside = tempfile::tempdir().expect("outside");
    let sentinel = outside.path().join("sentinel.txt");
    fs::write(&sentinel, b"outside\n").expect("sentinel");

    let render_link = temp.path().join("render-link");
    junction(&render_link, outside.path());
    assert!(
        materialize(&bundle, &rendered, &render_link.join("nested")).is_err(),
        "materialization through junction must be refused"
    );

    let render_final = temp.path().join("render-final");
    fs::create_dir_all(render_final.join("opencode")).expect("render parent");
    junction(&render_final.join("opencode/windows-root"), outside.path());
    assert!(
        materialize(&bundle, &rendered, &render_final).is_err(),
        "materialization onto a junction must be refused"
    );

    let home_link = temp.path().join("home-link");
    junction(&home_link, outside.path());
    assert!(
        install_rendered(
            &bundle,
            &rendered,
            &InstallOptions {
                home_dir: home_link,
                mode: "copy".to_owned(),
                ..Default::default()
            }
        )
        .is_err(),
        "install through junction must be refused"
    );
    let final_home = temp.path().join("home-final");
    let final_parent = final_home.join(".config/opencode/skills");
    fs::create_dir_all(&final_parent).expect("install parent");
    junction(&final_parent.join("windows-root"), outside.path());
    assert!(
        install_rendered(
            &bundle,
            &rendered,
            &InstallOptions {
                home_dir: final_home,
                mode: "copy".to_owned(),
                ..Default::default()
            }
        )
        .is_err(),
        "install onto a junction must be refused"
    );
    let mut entries = fs::read_dir(outside.path())
        .expect("outside entries")
        .map(|entry| entry.expect("outside entry").file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(entries, ["sentinel.txt"]);
    assert_eq!(
        fs::read(sentinel).expect("sentinel after refusal"),
        b"outside\n"
    );
}
