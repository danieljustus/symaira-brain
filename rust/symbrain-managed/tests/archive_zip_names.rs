use std::io::{Cursor, Write};

use symbrain_managed::{Core, extract_binary};
use zip::write::SimpleFileOptions;

#[test]
fn zip_names_preserve_selection_and_reject_unsafe_or_duplicate_entries() {
    let core = Core {
        binary_name: "symtool".to_owned(),
        ..Core::default()
    };
    let cases: &[(&[&str], Option<&str>)] = &[
        (&["symtool"], None),
        (&["rélease/symtool"], None),
        (&["../symtool"], Some("unsafe")),
        (&["dir\\..\\symtool"], Some("unsafe")),
        (&["/symtool"], Some("unsafe")),
        (&["C:\\symtool"], Some("unsafe")),
        (&["symtool", "./symtool"], Some("duplicate")),
        (&["first/symtool", "second/symtool"], Some("ambiguous")),
    ];
    for &(names, expected_error) in cases {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for name in names {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .expect("write fixture entry");
            writer.write_all(b"verified payload").unwrap();
        }
        let bytes = writer.finish().unwrap().into_inner();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.zip");
        std::fs::write(&path, bytes).unwrap();

        let result = extract_binary(&path, &core, "linux", "amd64");
        if let Some(expected) = expected_error {
            let error = result.expect_err(&format!("{names:?} must be rejected"));
            assert!(
                error.to_string().contains(expected),
                "{names:?}: expected {expected:?}, got {error}"
            );
        } else {
            assert_eq!(
                result.unwrap_or_else(|error| panic!("{names:?}: {error}")),
                b"verified payload"
            );
        }
    }
}
