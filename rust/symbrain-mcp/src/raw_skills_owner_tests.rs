//! Structural error ownership; all transport tests are pending compilation.
use super::owner::{Owner, diagnostic_owner};
use super::{syntax, transport_request};
use crate::{Decoder, FrameError, Mode};
use std::io::Cursor;

fn check(raw: &[u8], owner: Owner, expected: Option<&str>) {
    assert_eq!(diagnostic_owner(raw), owner, "{raw:?}");
    let expected = expected.map_or_else(
        || {
            serde_json::from_slice::<serde_json::Value>(raw)
                .unwrap_err()
                .to_string()
        },
        str::to_owned,
    );
    if owner == Owner::KnownSkills {
        assert_eq!(syntax::validate(raw).unwrap_err(), expected);
        assert!(matches!(transport_request(raw), Some(Err(error)) if error == expected));
    } else {
        assert!(transport_request(raw).is_none());
    }
    for mode in [Mode::Line, Mode::Framed] {
        let mut wire = if mode == Mode::Framed {
            format!("Content-Length: {}\r\n\r\n", raw.len()).into_bytes()
        } else {
            Vec::new()
        };
        wire.extend_from_slice(raw);
        if mode == Mode::Line {
            wire.push(b'\n');
        }
        match Decoder::new(Cursor::new(wire)).read_request().unwrap_err() {
            FrameError::Parse {
                mode: actual,
                message,
            } => {
                assert_eq!(actual, mode);
                assert_eq!(message, expected, "{raw:?}");
            }
            error => panic!("not a parse refusal: {error}"),
        }
    }
}

#[test]
fn malformed_known_skills_keep_original_messages_without_admission() {
    // These three rows were incorrectly labelled non-Skills by ddd6ee7a.
    for (raw, error) in [
        (br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list","arguments":{}}"#.as_slice(), "unexpected end of JSON input"),
        (br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":4,"name":"skills_list","_meta":[],"x":01}}"#, "invalid character '1' after object key:value pair"),
        (br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list"},"broken":"\u12x4"}"#, "invalid character 'x' in \\u hexadecimal character escape"),
    ] {
        check(raw, Owner::KnownSkills, Some(error));
    }
    for raw in [
        br#"{"JSONRPC":"2.0","m\u0065thod":"tools/call","param\u017f":{"na\u006de":"skills_list","arguments":01}}"#.as_slice(),
        br#"{"jsonrpc":"2.0","method":"tools/call","method":null,"params":{"name":"skills_list","name":null,"arguments":01}}"#,
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"arguments":01,"name":"skills_list"}}"#,
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"unknown","arguments":01,"NAME":"skills_list"}}"#,
        br#"{"jsonrpc":"2.0","params":{"name":"skills_list","arguments":01},"method":"ping","method":"tools/call"}"#,
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list","arguments":{"decoy":{"name":"other"},"bad":01}}}"#,
    ] {
        check(raw, Owner::KnownSkills, Some("invalid character '1' after object key:value pair"));
    }
}

#[test]
fn actual_later_overrides_decoys_and_unknown_tools_keep_inherited_errors() {
    for raw in [
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_unknown","arguments":01}}"#.as_slice(),
        br#"{"jsonrpc":"2.0","method":"ping","params":{"arguments":{"name":"skills_list"},"bad":01}}"#,
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list","arguments":01,"name":"memory_list"}}"#,
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list","arguments":{"bad":01}},"METHOD":"ping"}"#,
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list","arguments":[1,]},"p\u0061rams":{"name":"memory_list"}}"#,
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list","arguments":{"n" 1}},"jsonrpc":"1.0"}"#,
    ] {
        check(raw, Owner::Other, None);
    }
    for raw in [
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list","arguments":01},"params":null}"#.as_slice(),
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list","arguments":01},"params":{}}"#,
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"arguments":{"name":"skills_list"},"x":01}}"#,
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list","arguments":01,"na"#,
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list","arguments":01},"method":"to"#,
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list","arguments":01},"method":"\q"}"#,
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list","arguments":[1},"name":"other"}}"#,
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list"}}{"method":"ping"}"#,
        br#"{"ignored":{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list"}},"x":01}"#,
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list"},"method":"#,
    ] {
        check(raw, Owner::Indeterminate, None);
    }
}

#[test]
fn deep_rejected_values_cannot_hide_later_routing_overrides() {
    let nested = format!("{}0{}", "[".repeat(10_001), "]".repeat(10_001));
    for (suffix, owner) in [
        ("", Owner::KnownSkills),
        (r#", "method":"ping""#, Owner::Other),
    ] {
        let raw = format!(
            r#"{{"jsonrpc":"2.0","method":"tools/call","params":{{"name":"skills_list","arguments":{nested}}}{suffix}}}"#
        );
        let expected =
            (owner == Owner::KnownSkills).then_some("invalid character '[' exceeded max depth");
        check(raw.as_bytes(), owner, expected);
    }
}

#[test]
fn physical_truncation_stays_io_and_other_line_owners_keep_whole_line_utf8() {
    let raw =
        br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"skills_list","arguments":-"#;
    let mut wire = format!("Content-Length: {}\r\n\r\n", raw.len() + 1).into_bytes();
    wire.extend_from_slice(raw);
    assert!(matches!(
        Decoder::new(Cursor::new(wire)).read_request(),
        Err(FrameError::Io(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof
            && error.to_string() == "read body: unexpected EOF"
    ));
    let raw = b"  {\"jsonrpc\":\"2.0\",\"method\":\"ping\",\"ignored\":\"\xff\",\"x\":01}";
    let mut line = raw.to_vec();
    line.push(b'\n');
    let expected = String::from_utf8(line.clone()).unwrap_err().to_string();
    assert!(matches!(
        Decoder::new(Cursor::new(line)).read_request(),
        Err(FrameError::Parse { mode: Mode::Line, message }) if message == expected
    ));
    let expected = serde_json::from_slice::<serde_json::Value>(raw)
        .unwrap_err()
        .to_string();
    let mut wire = format!("Content-Length: {}\r\n\r\n", raw.len()).into_bytes();
    wire.extend_from_slice(raw);
    assert!(matches!(
        Decoder::new(Cursor::new(wire)).read_request(),
        Err(FrameError::Parse { mode: Mode::Framed, message }) if message == expected
    ));
}

#[test]
fn every_known_name_and_duplicate_null_type_rules_are_diagnostic_only() {
    for name in super::SKILLS_TOOL_NAMES {
        let raw = format!(
            r#"{{"jsonrpc":"2.0","jsonrpc":null,"method":"tools/call","method":4,"params":{{"name":"{name}","name":[],"arguments":{{"text":"\"method\":\"ping\",\"name\":\"other\"","bad":01}}}}}}"#
        );
        check(
            raw.as_bytes(),
            Owner::KnownSkills,
            Some("invalid character '1' after object key:value pair"),
        );
    }
    check(
        br#"{"jsonrpc":"2.0","method":"ping","params":{"text":"\"name\":\"skills_list\"","x":01}}"#,
        Owner::Other,
        None,
    );
    check(
        br#"{"method":"tools/call","params":{"name":"skills_list","arguments":01}}"#,
        Owner::Indeterminate,
        None,
    );
}
