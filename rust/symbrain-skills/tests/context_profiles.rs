//! Shared profile resolution preserves inheritance, precedence and cycles.
use std::fs;
use symbrain_skills::context_profile::{list, resolve};
use tempfile::tempdir;

#[test]
fn global_inheritance_is_overridden_by_project_links_and_sorted() {
    let owned = tempdir().unwrap();
    let global = owned.path().join("global");
    let project = owned.path().join("project");
    let local = project.join(".symskills/profiles");
    let library = owned.path().join("library");
    for path in [&global, &local, &library.join("one"), &library.join("two")] {
        fs::create_dir_all(path).unwrap();
    }
    fs::write(
        global.join("base.toml"),
        "name = 'base'\n[links.z]\nskill = 'one'\n",
    )
    .unwrap();
    fs::write(
        global.join("work.toml"),
        "name = 'work'\ninherits = ['base']\n[links.a]\nskill = 'one'\n",
    )
    .unwrap();
    fs::write(
        local.join("work.toml"),
        "name = 'work'\n[links.z]\nskill = 'two'\nalias = 'local'\n",
    )
    .unwrap();
    let (rows, issues) = resolve(&library, &global, Some(&project), "work").unwrap();
    assert!(issues.is_empty());
    assert_eq!(
        rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
        ["a", "z"]
    );
    assert_eq!(rows[0].source, "global");
    assert_eq!(rows[1].source, "project");
    assert_eq!(rows[1].skill, "two");
    assert_eq!(rows[1].alias, "local");
    let refs = list(&global, Some(&project)).unwrap();
    assert_eq!(
        refs.iter().find(|row| row.name == "work").unwrap().source,
        "project"
    );
}

#[test]
fn cycles_and_wrong_field_types_fail_before_any_render_or_install() {
    let owned = tempdir().unwrap();
    fs::write(
        owned.path().join("cycle.toml"),
        "name = 'cycle'\ninherits = ['cycle']\n",
    )
    .unwrap();
    assert!(
        resolve(owned.path(), owned.path(), None, "cycle")
            .unwrap_err()
            .0
            .contains("cycle")
    );
    fs::write(
        owned.path().join("wrong.toml"),
        "name = 'wrong'\ninherits = 4\n",
    )
    .unwrap();
    assert!(resolve(owned.path(), owned.path(), None, "wrong").is_err());
}
