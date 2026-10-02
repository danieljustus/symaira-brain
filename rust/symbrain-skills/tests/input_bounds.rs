use std::fs;
use std::path::Path;

use symbrain_skills::{
    MAX_FRONTMATTER_SIZE, MAX_INPUT_SIZE, MAX_RESOURCE_DEPTH, MAX_RESOURCE_ENTRIES,
    MAX_RESOURCE_SIZE, MAX_TOTAL_RESOURCE_BYTES, load_bundle, parse_skill_md,
};

fn skill(root: &Path) {
    fs::write(
        root.join("SKILL.md"),
        b"---\nname: bounded\ndescription: test\n---\nbody\n",
    )
    .unwrap();
}

fn document(header_size: usize, crlf: bool) -> Vec<u8> {
    let prefix = "name: bounded\n# ";
    let header = format!("{prefix}{}", "a".repeat(header_size - prefix.len()));
    let value = format!("---\n{header}\n---\nbody\n");
    if crlf {
        value.replace('\n', "\r\n").into_bytes()
    } else {
        value.into_bytes()
    }
}

#[test]
fn frontmatter_lf_and_crlf_exact_boundary_and_one_over() {
    for crlf in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        for size in [MAX_FRONTMATTER_SIZE, MAX_FRONTMATTER_SIZE + 1] {
            let raw = document(size, crlf);
            fs::write(temp.path().join("SKILL.md"), &raw).unwrap();
            if size == MAX_FRONTMATTER_SIZE {
                assert_eq!(parse_skill_md(&raw).unwrap().frontmatter.name, "bounded");
                assert!(load_bundle(temp.path()).is_ok());
            } else {
                assert!(
                    parse_skill_md(&raw)
                        .unwrap_err()
                        .0
                        .contains("frontmatter exceeds maximum")
                );
                assert!(
                    load_bundle(temp.path())
                        .unwrap_err()
                        .0
                        .contains("frontmatter exceeds maximum")
                );
            }
        }
    }
}

#[test]
fn oversized_control_files_rejected_before_parse() {
    for name in ["SKILL.md", "symskills.toml"] {
        let temp = tempfile::tempdir().unwrap();
        skill(temp.path());
        fs::File::create(temp.path().join(name))
            .unwrap()
            .set_len(MAX_INPUT_SIZE + 1)
            .unwrap();
        assert!(
            load_bundle(temp.path())
                .unwrap_err()
                .0
                .contains("exceeds maximum input size")
        );
    }
}

#[test]
fn resource_depth_exact_boundary_and_one_over() {
    let temp = tempfile::tempdir().unwrap();
    skill(temp.path());
    let mut directory = temp.path().to_path_buf();
    for _ in 0..MAX_RESOURCE_DEPTH {
        directory = directory.join("d");
        fs::create_dir(&directory).unwrap();
    }
    fs::write(directory.join("ok.txt"), b"ok").unwrap();
    assert!(load_bundle(temp.path()).is_ok());
    fs::create_dir(directory.join("d")).unwrap();
    assert!(
        load_bundle(temp.path())
            .unwrap_err()
            .0
            .contains("maximum depth")
    );
}

#[test]
fn resource_entries_exact_boundary_and_one_over() {
    let temp = tempfile::tempdir().unwrap();
    skill(temp.path());
    for index in 1..MAX_RESOURCE_ENTRIES {
        fs::write(temp.path().join(format!("f{index:05}")), []).unwrap();
    }
    assert_eq!(
        load_bundle(temp.path()).unwrap().resources.len(),
        MAX_RESOURCE_ENTRIES - 1
    );
    fs::write(temp.path().join("one-over"), []).unwrap();
    assert!(
        load_bundle(temp.path())
            .unwrap_err()
            .0
            .contains("maximum entry count")
    );
}

#[test]
fn aggregate_resource_bytes_exact_boundary_and_one_over() {
    let temp = tempfile::tempdir().unwrap();
    skill(temp.path());
    let mut remaining = MAX_TOTAL_RESOURCE_BYTES;
    let mut index = 0;
    while remaining > 0 {
        let size = remaining.min(MAX_RESOURCE_SIZE);
        fs::File::create(temp.path().join(format!("data{index}.bin")))
            .unwrap()
            .set_len(size)
            .unwrap();
        remaining -= size;
        index += 1;
    }
    assert!(load_bundle(temp.path()).is_ok());
    fs::write(temp.path().join("one-over.bin"), [0]).unwrap();
    assert!(
        load_bundle(temp.path())
            .unwrap_err()
            .0
            .contains("maximum total size")
    );
}

#[cfg(unix)]
#[test]
fn fifo_control_resources_and_library_never_block() {
    for name in [
        "SKILL.md",
        "symskills.toml",
        "data.txt",
        "overlays/claude/blocks/block.md",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("library");
        let root = library.join("bounded");
        fs::create_dir_all(&root).unwrap();
        skill(&root);
        let file = root.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        if file.exists() {
            fs::remove_file(&file).unwrap();
        }
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&file)
                .status()
                .unwrap()
                .success()
        );
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let error = load_bundle(&root).unwrap_err().0;
            let (_, issues) = symbrain_skills::library::list_library(&library);
            sender.send((error, issues)).unwrap();
        });
        let (error, issues) = receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("special inputs must fail without waiting for a FIFO writer");
        assert!(error.contains("must be a regular file"), "{error}");
        if name == "SKILL.md" {
            assert_eq!(issues[0].code, "skill_input_rejected");
        }
    }
}

#[test]
fn library_rejections_and_normal_order_are_stable() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["z", "a"] {
        let root = temp.path().join(name);
        fs::create_dir(&root).unwrap();
        skill(&root);
    }
    let (entries, issues) = symbrain_skills::library::list_library(temp.path());
    assert!(issues.is_empty());
    assert!(entries[0].path.ends_with('a'));
    let root = temp.path().join("a");
    fs::write(
        root.join("SKILL.md"),
        document(MAX_FRONTMATTER_SIZE + 1, false),
    )
    .unwrap();
    let (_, issues) = symbrain_skills::library::list_library(temp.path());
    assert_eq!(issues[0].code, "skill_input_rejected");
}
