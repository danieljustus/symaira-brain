use std::fs;

use serde::Deserialize;
use symbrain_managed::{
    Core, Manifest, atomic_install, download_url, extract_binary, find_checksum, verify_checksum,
};

#[derive(Deserialize)]
struct Oracle {
    manifest: Manifest,
    core_cases: Vec<CoreCase>,
    checksum_data: String,
    checksum: String,
    checksum_text: String,
    checksum_asset: String,
    archive_cases: Vec<ArchiveCase>,
}

#[derive(Deserialize)]
struct CoreCase {
    id: String,
    core: Core,
    goos: String,
    goarch: String,
    asset_name: String,
    asset_name_alt: String,
    checksum_asset_name: String,
    tag: String,
    certificate_identity: String,
    binary_path: String,
    supports_platform: bool,
}

#[derive(Deserialize)]
struct ArchiveCase {
    id: String,
    format: String,
    archive_hex: String,
    #[serde(default)]
    expected_hex: String,
    #[serde(default)]
    error_contains: String,
}

fn oracle() -> Oracle {
    serde_json::from_slice(include_bytes!("fixtures/oracle_expectations.json"))
        .expect("parse managed oracle")
}

fn decode_hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0, "hex length");
    (0..value.len())
        .step_by(2)
        .map(|index| {
            let text = &value[index..index + 2];
            u8::from_str_radix(text, 16).expect("hex byte")
        })
        .collect()
}

/// Version without a leading `v`, as it appears inside release asset names.
fn bare(version: &str) -> &str {
    version.strip_prefix('v').unwrap_or(version)
}

/// Compares the shipped manifest with the sealed Go-era oracle manifest by
/// structure and policy, deliberately NOT by pin values (#997).
///
/// The sealed fixture cannot change without a re-seal, while the shipped
/// `assets/manifest.json` is refreshed whenever a core releases. Everything that
/// is policy must still match exactly (core set, repo, binary and asset prefix,
/// cosign, platform and arch rules, optional flag, one checksum per archive and
/// the archive naming scheme); only the version and the sha256 digests may move.
fn assert_embedded_manifest_matches_sealed_structure(shipped: &Manifest, sealed: &Manifest) {
    assert_eq!(shipped.schema_version, sealed.schema_version);
    assert_eq!(
        shipped.cores.keys().collect::<Vec<_>>(),
        sealed.cores.keys().collect::<Vec<_>>(),
        "core set"
    );
    for (name, new) in &shipped.cores {
        let old = &sealed.cores[name];
        let policy = |core: &Core| Core {
            version: String::new(),
            sha256: std::collections::BTreeMap::default(),
            ..core.clone()
        };
        assert_eq!(policy(new), policy(old), "{name} policy");
        assert_eq!(
            bare(&new.version).split('.').count(),
            bare(&old.version).split('.').count(),
            "{name} version shape"
        );
        // Same archives, only re-versioned: map each sealed asset name onto the
        // shipped version and require exactly the shipped asset set.
        let repinned: std::collections::BTreeSet<String> = old
            .sha256
            .keys()
            .map(|asset| asset.replace(bare(&old.version), bare(&new.version)))
            .collect();
        let pinned: std::collections::BTreeSet<String> = new.sha256.keys().cloned().collect();
        assert_eq!(pinned, repinned, "{name} pinned archives");
        for (asset, digest) in &new.sha256 {
            assert!(
                digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
                "{name} {asset}: malformed sha256 {digest:?}"
            );
        }
    }
}

#[test]
fn embedded_manifest_and_core_naming_match_go() {
    let oracle = oracle();
    assert_embedded_manifest_matches_sealed_structure(
        &Manifest::load_embedded().expect("manifest"),
        &oracle.manifest,
    );
    for case in oracle.core_cases {
        let core = case.core;
        assert_eq!(
            core.asset_name(&case.goos, &case.goarch),
            case.asset_name,
            "{} asset",
            case.id
        );
        assert_eq!(
            core.asset_name_alt(&case.goos, &case.goarch),
            case.asset_name_alt,
            "{} alt",
            case.id
        );
        assert_eq!(
            core.checksum_asset_name(),
            case.checksum_asset_name,
            "{} checksum asset",
            case.id
        );
        assert_eq!(core.tag(), case.tag, "{} tag", case.id);
        assert_eq!(
            core.certificate_identity(),
            case.certificate_identity,
            "{} identity",
            case.id
        );
        assert_eq!(
            core.binary_path_in_archive(&case.goos, &case.goarch),
            case.binary_path,
            "{} binary path",
            case.id
        );
        assert_eq!(
            core.supports_platform(&case.goos),
            case.supports_platform,
            "{} platform",
            case.id
        );
    }
}

#[test]
fn checksum_contract_matches_go() {
    let oracle = oracle();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("payload");
    fs::write(&path, oracle.checksum_data).expect("write");
    verify_checksum(&path, &format!("  {}  \n", oracle.checksum)).expect("checksum");
    assert_eq!(
        find_checksum(&oracle.checksum_text, &oracle.checksum_asset).expect("find"),
        oracle.checksum
    );
}

#[test]
fn archive_selection_and_security_errors_match_go() {
    let oracle = oracle();
    let core = Core {
        version: "v1.2.3".to_string(),
        repo: "example/tool".to_string(),
        binary_name: "symtool".to_string(),
        asset_prefix: "symaira-tool".to_string(),
        ..Core::default()
    };
    for case in oracle.archive_cases {
        let dir = tempfile::tempdir().expect("tempdir");
        let extension = if case.format == "zip" {
            "fixture.zip"
        } else {
            "fixture.tar.gz"
        };
        let path = dir.path().join(extension);
        fs::write(&path, decode_hex(&case.archive_hex)).expect("write archive");
        let result = extract_binary(&path, &core, "linux", "amd64");
        if case.error_contains.is_empty() {
            assert_eq!(
                result.unwrap_or_else(|error| panic!("{}: {error}", case.id)),
                decode_hex(&case.expected_hex),
                "{} data",
                case.id
            );
        } else {
            let error = result.expect_err(&format!("{} should fail", case.id));
            assert!(
                error.to_string().contains(&case.error_contains),
                "{} error {:?} missing {:?}",
                case.id,
                error.to_string(),
                case.error_contains
            );
        }
    }
}

#[test]
fn atomic_install_replaces_bytes_and_sets_executable_mode() {
    let dir = tempfile::tempdir().expect("tempdir");
    atomic_install(dir.path(), "symtool", b"old").expect("first install");
    atomic_install(dir.path(), "symtool", b"new").expect("replace install");
    let path = dir.path().join("symtool");
    assert_eq!(fs::read(&path).expect("read"), b"new");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).expect("metadata").permissions().mode() & 0o777,
            0o755
        );
    }
}

#[test]
fn release_url_is_tag_pinned() {
    assert_eq!(
        download_url("https://github.com", "owner/repo", "v1.2.3", "asset.tar.gz"),
        "https://github.com/owner/repo/releases/download/v1.2.3/asset.tar.gz"
    );
}
