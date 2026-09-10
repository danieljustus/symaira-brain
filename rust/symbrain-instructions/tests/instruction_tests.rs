//! Executable parity and property-style tests for the instruction core.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use symbrain_instructions::{
    BEGIN_MARKER, END_MARKER, ESCAPE_SENTINEL, ESCAPED_BEGIN_MARKER, ESCAPED_END_MARKER,
    ESCAPED_ESCAPE_SENTINEL, MAX_SOURCE_FILE_BYTES, MAX_SOURCE_TOTAL_BYTES, Source, render,
    resolve_global_path,
};

#[derive(Debug, Deserialize)]
struct Oracle {
    schema_version: u32,
    rollback_baseline: String,
    go_toolchain: String,
    generator_sha256: String,
    provenance: Vec<Provenance>,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Provenance {
    path: String,
    sha256: String,
    rollback_sha256: String,
    rollback_present: bool,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    operation: String,
    #[serde(default)]
    existing: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    expected: String,
    #[serde(default)]
    global: String,
    #[serde(default)]
    project: String,
    #[serde(default)]
    global_present: bool,
    #[serde(default)]
    project_present: bool,
    #[serde(default)]
    expected_global_path: String,
    #[serde(default)]
    expected_project_path: String,
    #[serde(default)]
    xdg_config_home: String,
    #[serde(default)]
    expected_xdg_global_path: String,
    #[serde(default)]
    verdict: String,
    #[serde(default)]
    error: String,
    #[serde(default)]
    platform_evidence: String,
}

fn oracle() -> Oracle {
    serde_json::from_slice(include_bytes!("fixtures/oracle_expectations.json"))
        .expect("oracle fixture must be valid JSON")
}

fn decode(value: &str) -> Vec<u8> {
    let mut output = Vec::new();
    let mut accumulator = 0_u32;
    let mut bits = 0_u8;
    for byte in value.bytes() {
        if byte == b'=' {
            break;
        }
        let six = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'\n' | b'\r' => continue,
            _ => panic!("invalid base64 byte {byte}"),
        };
        accumulator = (accumulator << 6) | u32::from(six);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push(u8::try_from(accumulator >> bits).expect("base64 byte fits"));
            accumulator &= (1 << bits) - 1;
        }
    }
    output
}

fn case<'a>(suite: &'a Oracle, id: &str) -> &'a Case {
    suite
        .cases
        .iter()
        .find(|item| item.id == id)
        .unwrap_or_else(|| panic!("oracle case {id} is missing"))
}

#[test]
fn oracle_provenance_is_present_and_pinned() {
    let suite = oracle();
    assert_eq!(suite.schema_version, 1);
    assert_eq!(suite.rollback_baseline.len(), 40);
    assert_eq!(suite.go_toolchain, "go1.26.7");
    assert_eq!(suite.generator_sha256.len(), 64);
    assert!(
        suite
            .generator_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
    assert_eq!(suite.provenance.len(), 12);
    assert_eq!(
        suite
            .provenance
            .iter()
            .filter(|source| !source.rollback_present)
            .count(),
        3
    );
    for source in &suite.provenance {
        assert!(!source.path.is_empty());
        assert_eq!(source.sha256.len(), 64);
        assert!(source.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(source.rollback_sha256.len(), 64);
        assert!(
            source
                .rollback_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
    }
}

#[test]
fn oracle_render_cases_match_go() {
    let suite = oracle();
    for item in suite.cases.iter().filter(|item| item.operation == "render") {
        assert_eq!(
            render(&decode(&item.existing), &decode(&item.content)),
            decode(&item.expected),
            "render case {} differs from Go oracle",
            item.id
        );
    }
}

#[test]
fn oracle_source_cases_match_go() {
    let suite = oracle();
    for item in suite.cases.iter().filter(|item| item.operation == "source") {
        let temp = tempfile::tempdir().expect("temporary source root");
        let global = temp.path().join("global/instructions.md");
        let project = temp.path().join("project/.symbrain/instructions.md");
        if item.global_present {
            std::fs::create_dir_all(global.parent().expect("global parent")).expect("global dir");
            std::fs::write(&global, decode(&item.global)).expect("global instructions");
        }
        if item.project_present {
            std::fs::create_dir_all(project.parent().expect("project parent"))
                .expect("project dir");
            std::fs::write(&project, decode(&item.project)).expect("project instructions");
        }
        let source = Source::from_paths(global, Some(project));
        assert_eq!(
            source.content().expect("source read"),
            decode(&item.expected),
            "case {}",
            item.id
        );
    }
}

#[test]
fn oracle_source_verdict_cases_match_go() {
    let suite = oracle();
    for item in suite
        .cases
        .iter()
        .filter(|item| item.operation == "source_verdict")
    {
        assert_eq!(item.verdict, "error", "case {}", item.id);
        if item.platform_evidence == "unix_runtime" && !cfg!(unix) {
            // The fixture deliberately carries static Unix evidence when the
            // current runner cannot create a no-follow symlink case.
            continue;
        }
        let temp = tempfile::tempdir().expect("temporary verdict root");
        let global = temp.path().join("global");
        let project = temp.path().join("project");
        match item.id.as_str() {
            "source_symlink_rejected" => {
                #[cfg(unix)]
                {
                    let outside = temp.path().join("outside");
                    std::fs::write(&outside, b"outside").expect("outside source");
                    std::os::unix::fs::symlink(&outside, &global).expect("source symlink");
                }
            }
            "source_directory_rejected" => {
                std::fs::create_dir(&global).expect("source directory");
            }
            "source_per_file_limit_rejected" => {
                std::fs::write(
                    &global,
                    vec![0_u8; usize::try_from(MAX_SOURCE_FILE_BYTES).expect("limit") + 1],
                )
                .expect("oversized source");
            }
            "source_merged_limit_rejected" => {
                std::fs::write(
                    &global,
                    vec![0_u8; usize::try_from(MAX_SOURCE_FILE_BYTES).expect("limit")],
                )
                .expect("global source");
                std::fs::write(
                    &project,
                    vec![
                        0_u8;
                        usize::try_from(MAX_SOURCE_TOTAL_BYTES - MAX_SOURCE_FILE_BYTES + 1)
                            .expect("merged limit")
                    ],
                )
                .expect("project source");
            }
            id => panic!("unknown source verdict case {id}"),
        }
        let error = Source::from_paths(global, Some(project))
            .content()
            .expect_err("source verdict should fail");
        let normalized = error
            .to_string()
            .replace(temp.path().to_string_lossy().as_ref(), "<root>")
            .replace('\\', "/");
        assert_eq!(
            normalized, item.error,
            "case {} diagnostic differs from Go oracle",
            item.id
        );
    }
}
#[test]
fn renders_the_pinned_five_goldens() {
    let suite = oracle();
    for id in [
        "golden_fresh_file",
        "golden_existing_with_user_content",
        "golden_file_without_markers",
        "golden_crlf_file",
        "golden_no_trailing_newline",
    ] {
        let item = case(&suite, id);
        assert_eq!(
            render(&decode(&item.existing), &decode(&item.content)),
            decode(&item.expected),
            "golden {id}"
        );
    }
}

#[test]
fn second_render_is_byte_identical_for_deterministic_inputs() {
    let suite = oracle();
    for item in suite.cases.iter().filter(|item| item.operation == "render") {
        let first = render(&decode(&item.existing), &decode(&item.content));
        assert_eq!(
            render(&first, &decode(&item.content)),
            first,
            "case {}",
            item.id
        );
    }
}

#[test]
fn preserves_unmanaged_prefix_and_suffix_across_property_inputs() {
    let prefixes: [&[u8]; 6] = [
        b"",
        b"header",
        b"header\n",
        b"\xff\x00",
        b"# title\r\n",
        b"x\ny\n",
    ];
    let contents: [&[u8]; 4] = [b"", b"body", b"body\r\n", b"\xfe\x00\xff"];
    let suffixes: [&[u8]; 5] = [b"", b"\nfooter", b"\r\nfooter", b"\n\xff", b"\nend\n"];
    for prefix in prefixes {
        for content in contents {
            for suffix in suffixes {
                let mut existing = prefix.to_vec();
                existing.extend_from_slice(BEGIN_MARKER);
                existing.push(b'\n');
                existing.extend_from_slice(b"old");
                existing.extend_from_slice(END_MARKER);
                existing.extend_from_slice(suffix);
                let rendered = render(&existing, content);
                assert!(rendered.starts_with(prefix));
                assert!(rendered.ends_with(suffix));
                assert_eq!(render(&rendered, content), rendered);
            }
        }
    }
}

#[test]
fn malformed_and_multiple_boundaries_use_only_the_first_block() {
    let begin_only = b"before\n<!-- symbrain:begin -->\nold\n";
    assert_eq!(
        render(begin_only, b"new\n"),
        b"before\n<!-- symbrain:begin -->\nnew\n<!-- symbrain:end -->\n"
    );
    let multiple = b"<!-- symbrain:begin -->\nfirst\n<!-- symbrain:end -->\n<!-- symbrain:begin -->\nsecond\n<!-- symbrain:end -->\n";
    assert_eq!(
        render(multiple, b"final\n"),
        b"<!-- symbrain:begin -->\nfinal\n<!-- symbrain:end -->\n<!-- symbrain:begin -->\nsecond\n<!-- symbrain:end -->\n"
    );
    let end_before_begin = b"prefix <!-- symbrain:end -->\n<!-- symbrain:begin -->\nold";
    assert_eq!(
        render(end_before_begin, b"new"),
        b"prefix <!-- symbrain:end -->\n<!-- symbrain:begin -->\nnew<!-- symbrain:end -->\n"
    );
}

#[test]
fn preserves_non_utf8_bytes_like_go_strings() {
    let existing = b"before\xff\x00\n<!-- symbrain:begin -->\nold\n<!-- symbrain:end -->\n\xfe";
    let content = b"new\x80\x81";
    let expected =
        b"before\xff\x00\n<!-- symbrain:begin -->\nnew\x80\x81<!-- symbrain:end -->\n\xfe";
    assert_eq!(render(existing, content), expected);
}

#[test]
fn source_paths_follow_xdg_and_project_layout() {
    let home = Path::new("/oracle-home");
    let project = Path::new("/oracle-project");
    let source = Source::with_environment(Some(project), None, Some(home));
    assert_eq!(
        source.global_path,
        PathBuf::from("/oracle-home/.config/symbrain/instructions.md")
    );
    assert_eq!(
        source.project_path,
        Some(PathBuf::from("/oracle-project/.symbrain/instructions.md"))
    );
    let xdg = Path::new("/custom/config");
    assert_eq!(
        resolve_global_path(Some(xdg), Some(home)),
        PathBuf::from("/custom/config/symbrain/instructions.md")
    );
    let suite = oracle();
    let item = case(&suite, "source_paths");
    assert_eq!(
        source.project_path.unwrap().to_string_lossy(),
        item.expected_project_path
    );
    assert_eq!(
        source.global_path.to_string_lossy(),
        item.expected_global_path
    );
    assert_eq!(item.xdg_config_home, "/oracle-config");
    assert_eq!(
        resolve_global_path(Some(Path::new(&item.xdg_config_home)), Some(home)),
        PathBuf::from(&item.expected_xdg_global_path)
    );
}

#[test]
fn source_concatenates_without_inventing_a_separator() {
    let temp = tempfile::tempdir().expect("temporary source root");
    let global = temp.path().join("global");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&global).expect("global dir");
    std::fs::create_dir_all(&project).expect("project dir");
    let global_path = global.join("instructions.md");
    let project_path = project.join("instructions.md");
    std::fs::write(&global_path, b"global").expect("global file");
    std::fs::write(&project_path, b"project\xff").expect("project file");
    let source = Source::from_paths(global_path, Some(project_path));
    assert_eq!(source.content().expect("read source"), b"globalproject\xff");
}

#[test]
fn missing_source_files_are_empty_and_other_io_errors_are_returned() {
    let source = Source::from_paths(
        PathBuf::from("/definitely/missing/symbrain/instructions.md"),
        None,
    );
    assert_eq!(source.content().expect("missing files are ignored"), b"");

    let existing_parent = tempfile::tempdir().expect("existing source parent");
    let source = Source::from_paths(existing_parent.path().join("instructions.md"), None);
    assert_eq!(
        source
            .content()
            .expect("missing file below existing parent is ignored"),
        b""
    );
}

#[test]
fn reserved_markers_in_content_are_escaped_and_idempotent() {
    let content = format!(
        "literal {} and {}\n",
        String::from_utf8_lossy(BEGIN_MARKER),
        String::from_utf8_lossy(END_MARKER)
    );
    let first = render(b"prefix\n", content.as_bytes());
    assert!(
        first
            .windows(ESCAPED_BEGIN_MARKER.len())
            .any(|window| window == ESCAPED_BEGIN_MARKER)
    );
    assert!(
        first
            .windows(ESCAPED_END_MARKER.len())
            .any(|window| window == ESCAPED_END_MARKER)
    );
    assert_eq!(render(&first, content.as_bytes()), first);
}

#[test]
fn prefix_code_escape_is_injective_and_preserves_ordinary_bytes() {
    let double_begin = render(b"", [BEGIN_MARKER, BEGIN_MARKER].concat().as_slice());
    let existing_escape = render(b"", ESCAPED_BEGIN_MARKER);
    assert_ne!(double_begin, existing_escape);

    let content = [
        b"ordinary <!-- symbrain: ordinary -->\n".as_slice(),
        ESCAPE_SENTINEL,
        ESCAPED_BEGIN_MARKER,
        ESCAPED_END_MARKER,
        &[0xff, 0x00, 0xfe],
    ]
    .concat();
    let first = render(b"", &content);
    assert!(
        first
            .windows(ESCAPED_ESCAPE_SENTINEL.len())
            .any(|window| { window == ESCAPED_ESCAPE_SENTINEL })
    );
    assert!(
        first
            .windows(b"ordinary <!-- symbrain: ordinary -->".len())
            .any(|window| window == b"ordinary <!-- symbrain: ordinary -->")
    );
    assert!(first.windows(3).any(|window| window == [0xff, 0x00, 0xfe]));
    assert_eq!(render(&first, &content), first);
}

#[test]
fn source_rejects_directories_and_oversized_files() {
    let temp = tempfile::tempdir().expect("temporary source root");
    let directory = temp.path().join("directory");
    std::fs::create_dir(&directory).expect("directory");
    let directory_source = Source::from_paths(directory, None);
    assert!(directory_source.content().is_err());

    let oversized = temp.path().join("oversized");
    std::fs::write(
        &oversized,
        vec![0_u8; usize::try_from(MAX_SOURCE_FILE_BYTES).expect("test size") + 1],
    )
    .expect("oversized source");
    let oversized_source = Source::from_paths(oversized, None);
    assert!(oversized_source.content().is_err());
}

#[test]
fn source_rejects_oversized_merged_content() {
    let temp = tempfile::tempdir().expect("temporary source root");
    let global = temp.path().join("global");
    let project = temp.path().join("project");
    std::fs::write(
        &global,
        vec![0_u8; usize::try_from(MAX_SOURCE_FILE_BYTES).expect("test size")],
    )
    .expect("global source");
    std::fs::write(
        &project,
        vec![
            0_u8;
            usize::try_from(MAX_SOURCE_TOTAL_BYTES - MAX_SOURCE_FILE_BYTES + 1).expect("test size")
        ],
    )
    .expect("project source");
    let source = Source::from_paths(global, Some(project));
    assert!(source.content().is_err());
}

#[cfg(unix)]
#[test]
fn source_rejects_symlink_without_following_it() {
    let temp = tempfile::tempdir().expect("temporary source root");
    let outside = temp.path().join("outside");
    let link = temp.path().join("instructions");
    std::fs::write(&outside, b"outside").expect("outside source");
    std::os::unix::fs::symlink(&outside, &link).expect("symlink");
    let source = Source::from_paths(link, None);
    assert!(source.content().is_err());
}
