use std::{io::Cursor, path::PathBuf};

use super::{
    Batch, ImportError, Metadata, SessionImporter, SessionRef, ShellHistoryImporter, aider, bytes,
    curated, files, markdown, shell,
};

fn session() -> SessionRef {
    SessionRef {
        tool: b"aider".to_vec(),
        session_id: b"owned".to_vec(),
        path: PathBuf::from("owned"),
        modified_at: None,
        metadata: Metadata::new(),
    }
}

#[test]
fn scanner_full_buffer_requires_a_terminating_newline() {
    for (data, success) in [
        (vec![b'x'; 65535], true),
        (vec![b'x'; 65536], false),
        ([vec![b'x'; 65535], vec![b'\n']].concat(), true),
    ] {
        let mut scanned = Vec::new();
        let result = files::scan(&mut Cursor::new(data), 65536, |line| {
            scanned.push(line.len())
        });
        assert_eq!(result.is_ok(), success);
        assert_eq!(scanned.len(), usize::from(success));
    }
}

#[test]
fn scanner_crlf_and_final_empty_line_match_scanlines() {
    let mut lines = Vec::new();
    files::scan(&mut Cursor::new(b"a\r\n\n\r"), 65536, |line| {
        lines.push(line.to_vec())
    })
    .unwrap();
    assert_eq!(lines, [b"a".to_vec(), Vec::new(), Vec::new()]);
}

#[test]
fn aider_preserves_scanned_prefix_after_ignored_scanner_error() {
    let data = [
        b"**Assistant**:\n".to_vec(),
        vec![b'x'; 51],
        b"\n".to_vec(),
        vec![b'z'; 65536],
    ]
    .concat();
    let batch = aider::parse(&mut Cursor::new(data), &session());
    assert!(batch.error.is_none());
    assert_eq!(
        batch.rows.unwrap()[0].content,
        [vec![b'x'; 51], vec![b'\n']].concat()
    );
}

#[test]
fn aider_drops_role_line_content_and_flushes_at_headings() {
    let text = [
        b"**Assistant**: inline text is ignored\n".to_vec(),
        vec![0xff; 51],
        b"\n## next\n**Human**: ignored\n".to_vec(),
        vec![b'x'; 100],
    ]
    .concat();
    let batch = aider::parse(&mut Cursor::new(text), &session());
    let rows = batch.rows.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].content.len(), 52);
    assert_eq!(rows[0].content[0], 0xff);
}

#[test]
fn markdown_delimiters_are_source_specific() {
    let raw = b"---\nname: first\nname: ' second '\n---suffix\nbody\n";
    let (fm, body) = markdown::codex(raw);
    assert_eq!(bytes::value(&fm, b"name"), b"second");
    assert_eq!(body, b"suffix\nbody\n");
    let (body, fm, _, _) = curated::parse(&mut Cursor::new(raw)).unwrap();
    assert!(body.is_empty());
    assert!(fm.is_empty());
}

#[test]
fn applications_json_null_surrogates_and_invalid_bytes_keep_go_projection() {
    let raw = b"[null,\"z\",\"\\ud800\",\"\xff\xfe\",\"z\"]";
    assert_eq!(
        markdown::applications(raw),
        [
            b"z".to_vec(),
            "\u{fffd}".as_bytes().to_vec(),
            "\u{fffd}\u{fffd}".as_bytes().to_vec()
        ]
    );
}

#[test]
fn applications_fallback_keeps_spaces_inside_quotes() {
    assert_eq!(
        markdown::applications(b"[' a ',z]"),
        [b" a ".to_vec(), b"z".to_vec()]
    );
    assert_eq!(markdown::applications(b"[1]"), [b"1".to_vec()]);
}

#[test]
fn codex_list_frontmatter_resets_duplicate_keys() {
    let (fm, _) = markdown::codex(b"---\napplications:\n- z\n- a\napplications: x\n- y\n---\nbody");
    assert_eq!(bytes::value(&fm, b"applications"), b"x,y");
}

#[test]
fn go_json_escapes_html_and_each_bad_byte() {
    assert_eq!(
        bytes::json_quote(b"<\xff\xfe>&"),
        b"\"\\u003c\\ufffd\\ufffd\\u003e\\u0026\""
    );
}

#[test]
fn json_distinguishes_valid_replacement_from_each_invalid_byte() {
    assert_eq!(
        bytes::json_quote("\u{fffd}".as_bytes()),
        b"\"\xef\xbf\xbd\""
    );
    for raw in [b"\xff".as_slice(), b"\xc0", b"\x80", b"\xe2"] {
        assert_eq!(bytes::json_quote(raw), b"\"\\ufffd\"");
    }
    assert_eq!(
        bytes::json_quote(b"\xed\xa0\x80"),
        b"\"\\ufffd\\ufffd\\ufffd\""
    );
    assert_eq!(
        bytes::json_quote("a\u{2028}\u{2029}東京".as_bytes()),
        "\"a\\u2028\\u2029東京\"".as_bytes()
    );
    assert_eq!(
        bytes::json_quote(b"\0\x1f\x08\x0c\n\r\t\"\\"),
        b"\"\\u0000\\u001f\\b\\f\\n\\r\\t\\\"\\\\\""
    );
}

#[test]
fn embedded_json_retains_raw_key_order_and_literal_replacement() {
    let mut metadata = Metadata::new();
    bytes::set(&mut metadata, b"\xff", b"\xff".to_vec());
    bytes::set(
        &mut metadata,
        "\u{fffd}".as_bytes(),
        "\u{fffd}".as_bytes().to_vec(),
    );
    assert_eq!(
        bytes::json_map(&metadata),
        b"{\"\xef\xbf\xbd\":\"\xef\xbf\xbd\",\"\\ufffd\":\"\\ufffd\"}"
    );
}

#[cfg(unix)]
#[test]
fn basename_preserves_dot_components_without_changing_clean_or_join() {
    for (raw, base) in [
        (b"".as_slice(), b".".as_slice()),
        (b"////", b"/"),
        (b"/owned/vault/.//", b"."),
        (b"/owned/vault/..", b".."),
        (b"npm/", b"npm"),
        (b"/owned/\xff/", b"\xff"),
    ] {
        let path = files::from_bytes(raw.to_vec()).unwrap();
        assert_eq!(files::basename(&path).unwrap(), base);
    }
    let path = PathBuf::from("/owned/link/../vault/.");
    assert_eq!(files::clean(&path).unwrap(), PathBuf::from("/owned/vault"));
    assert_eq!(
        files::join(&path, b"../note").unwrap(),
        PathBuf::from("/owned/note")
    );
}

#[cfg(unix)]
#[test]
fn shell_tag_uses_the_original_final_command_component() {
    let importer = ShellHistoryImporter::new("owned".into(), true, Vec::new());
    for (command, tag) in [
        (b"npm/. install".as_slice(), b"".as_slice()),
        (b"npm/.. install", b""),
        (b"npm/ install", b"package-manager"),
    ] {
        let mut reference = session();
        bytes::set(&mut reference.metadata, b"command", command.to_vec());
        let batch = importer.import(&reference);
        assert!(batch.error.is_none());
        assert_eq!(bytes::value(&batch.rows.unwrap()[0].metadata, b"tag"), tag);
    }
}

#[test]
fn go_whitespace_does_not_include_bom_or_replace_invalid_content() {
    assert_eq!(bytes::trim(b" \xff\t"), b"\xff");
    assert_eq!(
        bytes::trim("\u{0085}\u{feff}\u{3000}".as_bytes()),
        "\u{feff}".as_bytes()
    );
    assert_eq!(bytes::fields("a\u{0085}b\u{2003}c".as_bytes()).len(), 3);
}

#[test]
fn curated_links_preserve_first_order_and_empty_trimmed_link() {
    assert_eq!(
        markdown::links(b"[[b]] [[a|display]] [[ b ]] [[  ]] [[b]] [[\xff]]"),
        [b"b".to_vec(), b"a".to_vec(), Vec::new(), vec![0xff]]
    );
}

#[test]
fn curated_scanner_failure_discards_complete_prefix() {
    let data = [b"valid prefix\n".to_vec(), vec![b'x'; 65536]].concat();
    assert!(curated::parse(&mut Cursor::new(data)).is_err());
}

#[test]
fn resource_filename_accepts_raw_name_and_rejects_changed_extension() {
    assert!(markdown::resource(b"2026-08-28T10-00-00-abcd-6h-\xff.md").is_some());
    for filename in [
        b"2026-08-28T10-00-00-abcd-6h-x.MD".as_slice(),
        b"2026-13-28T10-00-00-abcd-6h-x.md",
        b"2026-08-28T10-00-00--6h-x.md",
        b"2026-08-28T10-00-00-abcd-6h-.md",
    ] {
        assert!(markdown::resource(filename).is_none());
    }
}

#[test]
fn shell_bash_timestamp_is_not_joined_to_the_next_command() {
    let batch = shell::discover_lines(
        &mut Cursor::new(b"#1700000000\ngit status\n"),
        std::path::Path::new("owned"),
        &[],
        None,
    );
    let rows = batch.rows.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(bytes::value(&rows[0].metadata, b"command").is_empty());
    let importer = ShellHistoryImporter::new("owned".into(), true, Vec::new());
    assert!(importer.import(&rows[0]).rows.is_none());
}

#[test]
fn shell_ignores_duration_and_keeps_duplicate_timestamp_ids() {
    let data = b": 1700000000:0;git status\n: 1700000000:999;git diff\n: 1700000001:1;ls -la\n";
    let batch = shell::discover_lines(
        &mut Cursor::new(data),
        std::path::Path::new("owned"),
        &[Vec::new()],
        None,
    );
    let rows = batch.rows.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].session_id, rows[1].session_id);
}

#[test]
fn shell_scanner_error_retains_earlier_rows() {
    let data = [b": 1700000000:0;git status\n".to_vec(), vec![b'x'; 65536]].concat();
    let batch = shell::discover_lines(
        &mut Cursor::new(data),
        std::path::Path::new("owned"),
        &[],
        None,
    );
    assert_eq!(batch.rows.unwrap().len(), 1);
    assert!(matches!(
        batch.error,
        Some(ImportError::Message("bufio.Scanner: token too long"))
    ));
}

#[cfg(unix)]
#[test]
fn lexical_join_preserves_raw_names_and_cleans_before_kernel_lookup() {
    use std::os::unix::ffi::OsStrExt;
    let path = files::join(std::path::Path::new("/owned/link/../home"), b".codex/\xff").unwrap();
    assert_eq!(path.as_os_str().as_bytes(), b"/owned/home/.codex/\xff");
    assert_eq!(
        files::relative(
            std::path::Path::new("../base"),
            std::path::Path::new("../base/a")
        )
        .unwrap(),
        b"a"
    );
    assert!(
        files::relative(
            std::path::Path::new("../base"),
            std::path::Path::new("other")
        )
        .is_err()
    );
}

#[test]
fn batch_represents_nil_and_allocated_empty_separately() {
    let nil: Batch<SessionRef> = Batch::nil();
    let allocated: Batch<SessionRef> = Batch {
        rows: Some(Vec::new()),
        error: None,
    };
    assert!(nil.rows.is_none());
    assert!(allocated.rows.is_some_and(|rows| rows.is_empty()));
}
