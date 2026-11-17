use std::io::Write;

use symbrain_managed::{Core, extract_binary};

type Entry = (&'static str, tar::EntryType, &'static [u8]);

fn tar_gz(entries: &[Entry]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    for &(name, kind, data) in entries {
        let mut header = tar::Header::new_ustar();
        header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
        header.set_entry_type(kind);
        header.set_mode(0o755);
        header.set_size(data.len() as u64);
        header.set_cksum();
        builder.append(&header, data).unwrap();
    }
    let mut encoder = builder.into_inner().unwrap();
    encoder.flush().unwrap();
    encoder.finish().unwrap()
}

#[test]
fn tar_root_directory_entry_is_skipped_but_unsafe_paths_still_fail() {
    use tar::EntryType::{Directory, Regular};
    let core = Core {
        binary_name: "symtool".to_owned(),
        ..Core::default()
    };
    let cases: &[(&[Entry], Option<&str>)] = &[
        // symcockpit 0.12.1 darwin_universal layout
        (
            &[
                ("./", Directory, b""),
                ("./LICENSE", Regular, b"license"),
                ("./symtool", Regular, b"payload"),
            ],
            None,
        ),
        (
            &[(".", Directory, b""), ("symtool", Regular, b"payload")],
            None,
        ),
        (
            &[("../", Directory, b""), ("symtool", Regular, b"payload")],
            Some("unsafe"),
        ),
        (
            &[("./../", Directory, b""), ("symtool", Regular, b"payload")],
            Some("unsafe"),
        ),
        (
            &[("./", Directory, b""), ("../symtool", Regular, b"payload")],
            Some("unsafe"),
        ),
    ];
    for &(entries, expected_error) in cases {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.tar.gz");
        std::fs::write(&path, tar_gz(entries)).unwrap();
        let result = extract_binary(&path, &core, "darwin", "arm64");
        let names: Vec<_> = entries.iter().map(|entry| entry.0).collect();
        match expected_error {
            Some(expected) => {
                let error = result.expect_err(&format!("{names:?} must be rejected"));
                assert!(error.to_string().contains(expected), "{names:?}: {error}");
            }
            None => assert_eq!(
                result.unwrap_or_else(|error| panic!("{names:?}: {error}")),
                b"payload"
            ),
        }
    }
}
