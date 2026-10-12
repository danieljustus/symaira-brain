//! Prepared byte-owned presentation checks; source preparation did not run them.
use symbrain_skills::{GoText, parse_skill_md};

#[test]
fn invalid_bytes_and_literal_replacement_have_distinct_json_and_human_bytes() {
    for (raw, json) in [
        (b"\xff".as_slice(), "\"\\ufffd\""),
        (b"\xe2\x82", "\"\\ufffd\\ufffd\""),
        (b"\xc0\xaf", "\"\\ufffd\\ufffd\""),
        ("\u{fffd}".as_bytes(), "\"\u{fffd}\""),
    ] {
        let text = GoText::from_bytes(raw);
        assert_eq!(text.as_bytes(), raw);
        assert_eq!(text.len(), raw.len());
        assert_eq!(serde_json::to_string(&text).unwrap(), json);
    }
    let literal = GoText::from("\u{f000}\u{f1ff}");
    assert_eq!(literal.as_bytes(), "\u{f000}\u{f1ff}".as_bytes());
    assert_ne!(literal, GoText::from_bytes(b"\xff"));
}

#[test]
fn js_separators_html_and_quotes_are_escaped_at_json_boundary_only() {
    let raw = "<>&\u{2028}\u{2029}\"\\\n";
    let text = GoText::from(raw);
    assert_eq!(text.as_bytes(), raw.as_bytes());
    assert_eq!(
        serde_json::to_string(&text).unwrap(),
        "\"\\u003c\\u003e\\u0026\\u2028\\u2029\\\"\\\\\\n\""
    );
}

#[test]
fn crlf_frontmatter_decodes_only_header_and_retains_original_body_bytes() {
    for body in [
        b"body-\xff\r\n".as_slice(),
        b"body-\xe2\x82\r\n",
        b"body-\xc0\xaf\r\n",
        "body-\u{fffd}\r\n".as_bytes(),
    ] {
        let mut raw = b"---\r\nname: demo\r\ndescription: valid header\r\n---\r\n\r\n".to_vec();
        raw.extend_from_slice(body);
        let parsed = parse_skill_md(&raw).unwrap();
        assert_eq!(parsed.frontmatter.name, "demo");
        assert_eq!(parsed.body_line_offset, 5);
        let mut expected = body[..body.len() - 2].to_vec();
        expected.push(b'\n');
        assert_eq!(parsed.body.as_bytes(), expected);
        assert_eq!(parsed.body.len(), expected.len());
    }
    assert!(parse_skill_md(b"---\nname: \xff\n---\nbody\n").is_err());
}

#[cfg(unix)]
#[test]
fn native_path_does_not_lose_bytes_before_report_and_marker_serialization() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};
    let path = Path::new(OsStr::from_bytes(b"/owned/library-\xe2\x82"));
    let text = GoText::from_path(path);
    assert_eq!(text.as_bytes(), path.as_os_str().as_bytes());
    assert_eq!(text.json_token(), "\"/owned/library-\\ufffd\\ufffd\"");
    let marker = symbrain_skills::install::new_marker("opencode", "demo", path, "owned", false);
    let encoded = symbrain_skills::install::encode_marker(&marker).unwrap();
    let needle = b"\"rendered_at\": \"/owned/library-\\ufffd\\ufffd\"";
    assert!(encoded.windows(needle.len()).any(|part| part == needle));
}
