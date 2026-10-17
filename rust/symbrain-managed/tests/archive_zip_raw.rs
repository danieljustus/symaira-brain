use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use symbrain_managed::{Core, Installer, Platform, extract_binary};

#[path = "support/raw_zip.rs"]
mod raw_zip;
#[path = "support/server.rs"]
mod server;

#[test]
fn repeated_raw_zip_names_cannot_hide_entries_or_replace_installed_bytes() {
    let mut failures = Vec::new();
    for name in [b"symtool".as_slice(), b"documentation".as_slice()] {
        for (first, second) in [
            (0o100_755, 0o100_755),
            (0o120_777, 0o100_755),
            (0o100_755, 0o120_777),
            (0o040_755, 0o100_755),
            (0o100_755, 0o040_755),
            (0o010_600, 0o100_755),
            (0o100_755, 0o010_600),
        ] {
            for zip64 in [false, true] {
                let bytes = raw_zip::archive(
                    &[
                        (name, b"first", first),
                        (b"symtool", b"binary", 0o100_755),
                        (name, b"second", second),
                    ],
                    zip64,
                    b"self extracting prefix",
                );
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("fixture.zip");
                std::fs::write(&path, &bytes).unwrap();
                let core = Core {
                    version: "v1.2.3".into(),
                    repo: "example/tool".into(),
                    binary_name: "symtool".into(),
                    asset_prefix: "symtool".into(),
                    has_cosign: false,
                    ..Core::default()
                };
                match extract_binary(&path, &core, "windows", "amd64") {
                    Ok(data) => failures.push(format!(
                        "{name:?}/{first:o}/{second:o}/{zip64}: extracted {data:?}"
                    )),
                    Err(error) => assert!(
                        error.to_string().contains("duplicate raw zip entry"),
                        "{error}"
                    ),
                }
                let platform = Platform {
                    os: "windows",
                    arch: "amd64",
                };
                let asset = core.asset_name(platform.os, platform.arch);
                let mut core = core;
                core.sha256 =
                    BTreeMap::from([(asset.clone(), format!("{:x}", Sha256::digest(&bytes)))]);
                let prefix = format!("/{}/releases/download/{}/", core.repo, core.tag());
                let routes = BTreeMap::from([(format!("{prefix}{asset}"), bytes)]);
                let server = if zip64 {
                    server::TestServer::start_http10(routes)
                } else {
                    server::TestServer::start(routes)
                };
                let bin = temp.path().join("bin");
                std::fs::create_dir(&bin).unwrap();
                std::fs::write(bin.join("symtool"), b"existing").unwrap();
                let installer = Installer::with_base_url(&bin, false, &server.url).unwrap();
                if installer.install(&core, platform, &mut Vec::new()).is_ok() {
                    failures.push(format!("{name:?}/{first:o}/{second:o}/{zip64}: installed"));
                }
                if std::fs::read(bin.join("symtool")).unwrap() != b"existing" {
                    failures.push(format!(
                        "{name:?}/{first:o}/{second:o}/{zip64}: changed destination"
                    ));
                }
                if std::fs::read_dir(&bin).unwrap().count() != 1 {
                    failures.push(format!(
                        "{name:?}/{first:o}/{second:o}/{zip64}: published extra files"
                    ));
                }
                let requests = server.requests.lock().unwrap();
                let archives: Vec<_> = requests
                    .iter()
                    .filter(|path| {
                        std::path::Path::new(path)
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
                    })
                    .collect();
                assert_eq!(archives, [&format!("{prefix}{asset}")]);
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn independent_valid_zip_formats_preserve_selection() {
    for zip64 in [false, true] {
        for prefix in [b"".as_slice(), b"self extracting prefix".as_slice()] {
            for name in [
                b"symtool".as_slice(),
                "rélease/symtool".as_bytes(),
                b"r\x82lease/symtool".as_slice(),
            ] {
                let bytes = raw_zip::archive(
                    &[
                        (b"documentation", b"read me", 0o100_644),
                        (name, b"binary", 0o100_755),
                        (b"another-tool", b"other", 0o100_755),
                    ],
                    zip64,
                    prefix,
                );
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("fixture.zip");
                std::fs::write(&path, bytes).unwrap();
                let core = Core {
                    binary_name: "symtool".into(),
                    ..Core::default()
                };
                assert_eq!(
                    extract_binary(&path, &core, "linux", "amd64").unwrap(),
                    b"binary",
                    "{zip64}/{prefix:?}/{name:?}"
                );
            }
        }
    }
}

#[test]
fn duplicate_records_are_rejected_at_every_directory_position() {
    for names in [
        vec!["symtool", "symtool"],
        vec!["docs", "symtool", "symtool"],
        vec!["symtool", "docs", "docs"],
        vec!["a", "b", "symtool", "b"],
        vec!["a", "b", "b", "a", "symtool"],
    ] {
        let entries: Vec<_> = names
            .iter()
            .map(|name| (name.as_bytes(), b"payload".as_slice(), 0o100_755))
            .collect();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("fixture.zip");
        std::fs::write(&path, raw_zip::archive(&entries, false, b"")).unwrap();
        let core = Core {
            binary_name: "symtool".into(),
            ..Core::default()
        };
        let error = extract_binary(&path, &core, "linux", "amd64").unwrap_err();
        assert!(
            error.to_string().contains("duplicate raw zip entry"),
            "{names:?}: {error}"
        );
    }
}
