//! Prepared Go scanner EOF-state and both-transport admission regressions.
use super::*;
use crate::{Decoder, FrameError, Mode};
use std::io::Cursor;

fn each_transport(raw: &[u8], expected: &str) {
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
        let error = Decoder::new(Cursor::new(wire)).read_request().unwrap_err();
        assert_eq!(error.mode(), mode);
        assert!(error.is_parse());
        match error {
            FrameError::Parse { message, .. } => assert_eq!(message, expected),
            error => panic!("unexpected framing error: {error}"),
        }
    }
}

#[test]
fn required_token_eof_uses_active_go_state_before_generic_end() {
    let prefix = br#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"skills_list","_meta":{"n":"#;
    let groups: &[(&[&[u8]], &str)] = &[
        (&[b"-"], "in numeric literal"),
        (&[b"0.", b"1."], "after decimal point in numeric literal"),
        (
            &[b"1e", b"1E", b"1e+", b"1e-", b"-0E+"],
            "in exponent of numeric literal",
        ),
        (&[b"t"], "in literal true (expecting 'r')"),
        (&[b"tr"], "in literal true (expecting 'u')"),
        (&[b"tru"], "in literal true (expecting 'e')"),
        (&[b"f"], "in literal false (expecting 'a')"),
        (&[b"fa"], "in literal false (expecting 'l')"),
        (&[b"fal"], "in literal false (expecting 's')"),
        (&[b"fals"], "in literal false (expecting 'e')"),
        (&[b"n"], "in literal null (expecting 'u')"),
        (&[b"nu"], "in literal null (expecting 'l')"),
        (&[b"nul"], "in literal null (expecting 'l')"),
        (&[br#""abc\"#], "in string escape code"),
        (
            &[br#""\u"#, br#""\u0"#, br#""\u01"#, br#""\u012"#],
            "in \\u hexadecimal character escape",
        ),
    ];
    for &(tokens, context) in groups {
        for token in tokens {
            let expected = format!("invalid character ' ' {context}");
            assert_eq!(syntax::validate(token).unwrap_err(), expected);
            let raw = [prefix.as_slice(), *token].concat();
            each_transport(&raw, &expected);
            // An earlier typed error cannot obscure the later syntax failure.
            let raw = String::from_utf8(raw).unwrap().replace(
                r#""name":"skills_list""#,
                r#""name":4,"name":"skills_list""#,
            );
            each_transport(raw.as_bytes(), &expected);
        }
    }
}

#[test]
fn incomplete_containers_and_ordinary_strings_keep_generic_eof() {
    for raw in [
        b"".as_slice(),
        b" ",
        b"{",
        b"[",
        br#"{"k""#,
        br#"{"k":"#,
        br#"[1,"#,
        br#""abc"#,
        br#""\u1234"#,
        br#"{"k":1"#,
        br#"{"k":true"#,
        br#"{"k":"v""#,
    ] {
        assert_eq!(
            syntax::validate(raw).unwrap_err(),
            "unexpected end of JSON input"
        );
    }
    for raw in [
        b"0".as_slice(),
        b"-1",
        b"1.0",
        b"1e+2",
        b"true",
        b"false",
        b"null",
        br#""abc""#,
        br#""\u1234""#,
    ] {
        assert!(syntax::validate(raw).is_ok());
    }
    // An actual byte has its own quote/context; EOF synthesis is not a rewrite.
    assert_eq!(
        syntax::validate(b"1eX").unwrap_err(),
        "invalid character 'X' in exponent of numeric literal"
    );
    assert_eq!(
        syntax::validate(b"\"\\\n").unwrap_err(),
        "invalid character '\\n' in string escape code"
    );
}

#[test]
fn original_independent_eof_wires_keep_full_parse_message_in_both_modes() {
    macro_rules! original {
        ($name:literal, $expected:literal) => {
            for (wire, mode) in [
                (
                    include_bytes!(concat!(
                        "../../../scripts/skills-native-oracle/eof-review-inputs/",
                        $name,
                        ".line"
                    ))
                    .as_slice(),
                    Mode::Line,
                ),
                (
                    include_bytes!(concat!(
                        "../../../scripts/skills-native-oracle/eof-review-inputs/",
                        $name,
                        ".framed"
                    ))
                    .as_slice(),
                    Mode::Framed,
                ),
            ] {
                let error = Decoder::new(Cursor::new(wire)).read_request().unwrap_err();
                assert_eq!(error.mode(), mode);
                match error {
                    FrameError::Parse { message, .. } => assert_eq!(message, $expected),
                    error => panic!("unexpected framing error: {error}"),
                }
            }
        };
    }
    original!("minus", "invalid character ' ' in numeric literal");
    original!(
        "dot",
        "invalid character ' ' after decimal point in numeric literal"
    );
    original!(
        "exponent",
        "invalid character ' ' in exponent of numeric literal"
    );
    original!(
        "exp-sign",
        "invalid character ' ' in exponent of numeric literal"
    );
    original!(
        "true",
        "invalid character ' ' in literal true (expecting 'e')"
    );
    original!(
        "false",
        "invalid character ' ' in literal false (expecting 'e')"
    );
    original!(
        "null",
        "invalid character ' ' in literal null (expecting 'l')"
    );
    original!("escape", "invalid character ' ' in string escape code");
    original!(
        "unicode",
        "invalid character ' ' in \\u hexadecimal character escape"
    );
}
