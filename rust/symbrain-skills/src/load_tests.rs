use std::cell::Cell;
use std::fs;
use std::path::Path;

use ambient_authority::ambient_authority;
use cap_std::fs::Dir;
use tempfile::tempdir;

use super::{ReadBudget, collect_bounded_entries, read_limited, read_limited_expected};

#[test]
fn enumeration_stops_before_collecting_over_limit() {
    let observed = Cell::new(0);
    let entries = std::iter::from_fn(|| {
        observed.set(observed.get() + 1);
        Some(Ok::<_, std::io::Error>(observed.get()))
    });
    let error = collect_bounded_entries(entries, 2, "test", "entry cap")
        .expect_err("an unbounded iterator must be stopped");
    assert_eq!(error.0, "entry cap");
    assert_eq!(observed.get(), 3, "only one overflow entry is inspected");
    assert_eq!(
        collect_bounded_entries([Ok(1), Ok(2)], 2, "test", "entry cap").unwrap(),
        [1, 2]
    );
}

#[test]
fn small_reads_do_not_reserve_the_full_public_limit() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("small"), b"tiny").unwrap();
    let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
    let bytes = read_limited(&root, Path::new("small"), "small", crate::MAX_INPUT_SIZE).unwrap();
    assert_eq!(bytes, b"tiny");
    assert_eq!(bytes.capacity(), 4);
}

#[test]
fn rejected_headers_still_consume_the_actual_read_budget() {
    let temp = tempdir().unwrap();
    let mut document = b"---\n".to_vec();
    document.resize(crate::MAX_FRONTMATTER_SIZE + 5, b'a');
    fs::write(temp.path().join("bad"), &document).unwrap();
    fs::write(temp.path().join("next"), b"x").unwrap();
    let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
    let mut budget = ReadBudget::new(document.len() as u64, "test inputs");
    let error =
        super::read_skill_document(&root, Path::new("bad"), "bad", Some(&mut budget)).unwrap_err();
    assert!(error.to_string().contains("frontmatter exceeds maximum"));
    assert_eq!(
        budget.remaining, 0,
        "rejected reads must not reset the budget"
    );
    assert!(
        super::read_skill_document(&root, Path::new("next"), "next", Some(&mut budget))
            .unwrap_err()
            .to_string()
            .contains("exceeds maximum total size")
    );
}

#[test]
fn actual_read_budget_accepts_exact_total_then_rejects_one_over() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("a"), b"12").unwrap();
    fs::write(temp.path().join("b"), b"34").unwrap();
    fs::write(temp.path().join("c"), b"5").unwrap();
    let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
    let mut budget = ReadBudget::new(4, "test inputs");
    for name in ["a", "b"] {
        assert_eq!(
            read_limited_expected(&root, Path::new(name), name, 4, 2, Some(&mut budget))
                .unwrap()
                .len(),
            2
        );
    }
    assert_eq!(budget.remaining, 0);
    assert_eq!(
        read_limited_expected(&root, Path::new("c"), "c", 4, 1, Some(&mut budget))
            .unwrap_err()
            .0,
        "test inputs exceeds maximum total size of 4 bytes"
    );
}

#[test]
fn resource_growth_after_inventory_is_rejected() {
    let temp = tempdir().unwrap();
    let file = temp.path().join("resource");
    fs::write(&file, b"tiny").unwrap();
    let inventoried = fs::metadata(&file).unwrap().len();
    fs::write(&file, b"larger").unwrap();
    let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
    assert!(
        read_limited_expected(
            &root,
            Path::new("resource"),
            "resource",
            16,
            inventoried,
            None
        )
        .unwrap_err()
        .0
        .contains("changed since inventory")
    );
}

#[test]
fn repeated_bundle_reads_and_clones_share_the_loading_budget() {
    let temp = tempdir().unwrap();
    let document = b"---\nname: bounded\ndescription: test\n---\nbody\n";
    fs::write(temp.path().join("SKILL.md"), document).unwrap();
    fs::write(temp.path().join("data.bin"), b"1234").unwrap();
    let budget = std::sync::Arc::new(std::sync::Mutex::new(ReadBudget::new(
        document.len() as u64 + 4,
        "test operation",
    )));
    let bundle =
        super::load_bundle_with_budget(temp.path(), std::sync::Arc::clone(&budget)).unwrap();
    let cloned = bundle.clone();
    assert_eq!(
        super::read_bundle_bytes(&bundle, Path::new("data.bin"), "data.bin", 4).unwrap(),
        b"1234"
    );
    assert_eq!(budget.lock().unwrap().remaining, 0);
    assert!(
        super::read_bundle_bytes(&cloned, Path::new("data.bin"), "data.bin", 4)
            .unwrap_err()
            .0
            .contains("test operation exceeds maximum total size")
    );
    assert_eq!(budget.lock().unwrap().remaining, 0);
}

#[test]
fn a_second_bundle_cannot_reset_the_operation_budget() {
    let temp = tempdir().unwrap();
    let document = b"---\nname: bounded\ndescription: test\n---\nbody\n";
    for name in ["a", "b"] {
        fs::create_dir(temp.path().join(name)).unwrap();
        fs::write(temp.path().join(name).join("SKILL.md"), document).unwrap();
    }
    let loader = super::BundleLoader {
        budget: std::sync::Arc::new(std::sync::Mutex::new(ReadBudget::new(
            document.len() as u64,
            "test operation",
        ))),
    };
    loader.load(&temp.path().join("a")).unwrap();
    assert!(
        loader
            .clone()
            .load(&temp.path().join("b"))
            .unwrap_err()
            .0
            .contains("test operation exceeds maximum total size")
    );
}

#[test]
fn growth_at_exhausted_budget_does_not_read_a_probe_byte() {
    use std::io::{Read, Seek, Write};
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("data"), b"1234").unwrap();
    let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
    let mut file = super::open_read_with(&root, Path::new("data"), "data", false).unwrap();
    let mut bytes = [0; 4];
    file.read_exact(&mut bytes).unwrap();
    let mut budget = ReadBudget::new(4, "test inputs");
    budget.remaining = 0;
    fs::OpenOptions::new()
        .append(true)
        .open(temp.path().join("data"))
        .unwrap()
        .write_all(b"5")
        .unwrap();
    let error =
        super::check_read_end(&file, "data", 16, 4, None, Some(4), Some(&budget)).unwrap_err();
    assert!(matches!(error, super::InputReadError::Budget(_)));
    assert_eq!(
        file.stream_position().unwrap(),
        4,
        "no over-budget probe was consumed"
    );
    assert_eq!(budget.remaining, 0);
}
