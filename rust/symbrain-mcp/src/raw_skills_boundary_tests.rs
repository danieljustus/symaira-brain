//! Prepared full-syntax, metadata and exact Go-number acceptance regressions.
use super::*;
use crate::{Decoder, FrameError, Mode};
use std::io::Cursor;

fn body(arguments: &str, extra: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"skills_list","arguments":{arguments}{extra}}}}}"#
    )
}
fn each_transport(raw: &[u8], check: impl Fn(Result<Option<(crate::Request, Mode)>, FrameError>)) {
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
        check(Decoder::new(Cursor::new(wire)).read_request());
    }
}

#[test]
fn ignored_arguments_and_envelope_keep_full_go_depth_without_value128_fallback() {
    for count in [1, 100, 128, 1024, 9000, 9997] {
        let nested = format!("{}0{}", "[".repeat(count), "]".repeat(count));
        let raw = body(&format!(r#"{{"ignored":{nested}}}"#), "");
        each_transport(raw.as_bytes(), |found| {
            let request = found.unwrap().unwrap().0;
            assert!(
                raw_skills_params(request.params.as_deref())
                    .unwrap()
                    .is_ok()
            );
        });
        // Ignored envelope member instead of an argument: total depth count+1.
        let raw = body("{}", "").replacen('{', &format!(r#"{{"ignored":{nested},"#), 1);
        each_transport(raw.as_bytes(), |found| {
            assert!(found.is_ok());
        });
    }
}

#[test]
fn complete_syntax_precedes_typed_errors_and_retains_original_depth_character() {
    for open in ["[", "{"] {
        let child = if open == "[" {
            "[".repeat(9998)
        } else {
            "{\"n\":".repeat(9998)
        };
        let close = if open == "[" { "]" } else { "}" };
        let raw = body(
            &format!(r#"{{"ignored":{child}0{}}}"#, close.repeat(9998)),
            "",
        )
        .replace(
            r#""name":"skills_list""#,
            r#""name":4,"name":"skills_list""#,
        );
        each_transport(raw.as_bytes(), |found| match found.unwrap_err() {
            FrameError::Parse { message, .. } => assert_eq!(
                message,
                format!("invalid character '{open}' exceeded max depth")
            ),
            error => panic!("unexpected framing error: {error}"),
        });
    }
    let raw = body("{}", r#","_meta":[],"broken":01"#);
    each_transport(raw.as_bytes(), |found| {
        assert!(matches!(found, Err(FrameError::Parse { message, .. })
            if message == "invalid character '1' after object key:value pair"));
    });
}

#[test]
fn deep_metadata_is_valid_and_duplicate_numeric_first_error_is_preserved() {
    for count in [128, 1024, 9000, 9997] {
        let meta = format!(
            r#","_meta":{{"n":{}0{}}}"#,
            "[".repeat(count),
            "]".repeat(count)
        );
        let raw = body("{}", &meta);
        each_transport(raw.as_bytes(), |found| {
            let request = found.unwrap().unwrap().0;
            assert!(
                raw_skills_params(request.params.as_deref())
                    .unwrap()
                    .is_ok()
            );
        });
    }
    let raw = body(
        "{}",
        r#","_meta":{"n":-1e9999,"n":0,"a":[1e9999]},"_meta":{}"#,
    );
    let request = transport_request(raw.as_bytes()).unwrap().unwrap();
    assert_eq!(
        raw_skills_params(request.params.as_deref())
            .unwrap()
            .unwrap_err(),
        "Invalid params: json: cannot unmarshal number -1e9999 into Go struct field ._meta of type float64"
    );
}

#[test]
fn syntax_is_string_aware_and_metadata_go_long_decimal_is_finite() {
    let long = format!("0.{}1e100000", "0".repeat(10_000));
    let raw = body(
        "{}",
        &format!(
            r#","_meta":{{"long":{long},"negative":-{long},"zero":-0,"tiny":-1e-9999,"text":"[{{\\\"}}]"}}"#
        ),
    );
    each_transport(raw.as_bytes(), |found| {
        let request = found.unwrap().unwrap().0;
        assert!(
            raw_skills_params(request.params.as_deref())
                .unwrap()
                .is_ok()
        );
    });
    for raw in [
        b"{\"n\":+1}".as_slice(),
        b"[1,]",
        b"{\"n\":NaN}",
        b"\"\\u12x4\"",
        b"{}{}",
    ] {
        assert!(syntax::validate(raw).is_err());
    }
    let mut numbers = Vec::new();
    syntax::scan(br#"{"n":1,"n":[-0,0.1,1e-9999],"text":"0x1p2"}"#, |raw| {
        numbers.push(raw.to_vec());
    })
    .unwrap();
    assert_eq!(
        numbers,
        [
            b"1".to_vec(),
            b"-0".to_vec(),
            b"0.1".to_vec(),
            b"1e-9999".to_vec()
        ]
    );
}
