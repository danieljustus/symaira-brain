//! Prepared raw transport / ordered outer admission regressions.
use super::*;
use crate::{Decoder, Mode};
use std::io::Cursor;

fn call(params: &str) -> Result<RawSkillsCall, String> {
    let raw = RawValue::from_string(params.into()).unwrap();
    raw_skills_params(Some(&raw)).expect("known Skills route")
}

#[test]
fn typed_metadata_and_first_duplicate_name_error_prevent_handler_admission() {
    for (params, detail) in [
        (
            r#"{"name":"skills_install","arguments":{"name":"demo","dry_run":false},"_meta":[]}"#,
            "array into Go struct field ._meta of type map[string]interface {}",
        ),
        (
            r#"{"name":"skills_list","arguments":{},"_meta":{"token":1e9999}}"#,
            "number 1e9999 into Go struct field ._meta of type float64",
        ),
        (
            r#"{"name":4,"name":"skills_list","arguments":{}}"#,
            "number into Go struct field .name of type string",
        ),
        (
            r#"{"_meta":[],"_meta":{},"name":"skills_list"}"#,
            "array into Go struct field ._meta of type map[string]interface {}",
        ),
    ] {
        assert_eq!(
            call(params).unwrap_err(),
            format!("Invalid params: json: cannot unmarshal {detail}")
        );
    }
    let (name, args) =
        call(r#"{"name":"skills_list","name":null,"arguments":{"ignored":1e9999},"_meta":null}"#)
            .unwrap();
    assert_eq!(name, "skills_list");
    assert_eq!(args.unwrap().get(), r#"{"ignored":1e9999}"#);
    assert!(call(r#"{"name":"skills_list","_meta":{"nested":[{"token":1e9999}]}}"#).is_err());
    assert!(call(r#"{"name":"skills_list","_meta":{"\ud800":null,"number":1}}"#).is_ok());
}

#[test]
fn raw_strings_surrogate_outer_keys_and_ignored_numbers_reach_both_transports() {
    let samples = [
        b"\xff".as_slice(),
        b"\xe2\x82",
        b"\xc0\xaf",
        "\u{fffd}".as_bytes(),
    ];
    for sample in samples {
        let mut body = br#"{"jsonrpc":"2.0","id":3,"\ud800":null,"method":"tools/call","params":{"\ud800":false,"name":"skills_list","arguments":{"ignored":""#.to_vec();
        body.extend_from_slice(sample);
        body.extend_from_slice(br#"","unknown":1e9999}}}"#);
        for mode in [Mode::Line, Mode::Framed] {
            let mut data = if mode == Mode::Framed {
                format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes()
            } else {
                Vec::new()
            };
            data.extend_from_slice(&body);
            if mode == Mode::Line {
                data.push(b'\n');
            }
            let (request, found) = Decoder::new(Cursor::new(data))
                .read_request()
                .unwrap()
                .unwrap();
            assert_eq!(found, mode);
            let (name, arguments) = raw_skills_params(request.params.as_deref())
                .unwrap()
                .unwrap();
            assert_eq!(name, "skills_list");
            let arguments = arguments.unwrap();
            let expected = "\u{fffd}".repeat(if sample == b"\xe2\x82" || sample == b"\xc0\xaf" {
                2
            } else {
                1
            });
            assert!(arguments.get().contains(&expected));
            assert!(arguments.get().contains("1e9999"));
        }
        let ordinary = body.windows(11).position(|s| s == b"skills_list").unwrap();
        let mut other = body[..ordinary].to_vec();
        other.extend_from_slice(b"memory_list");
        other.extend_from_slice(&body[ordinary + 11..]);
        // Unknown outer surrogate keys keep the inherited non-Skills rejection.
        assert!(transport_request(&other).is_none());
        assert!(
            Decoder::new(Cursor::new([other.as_slice(), b"\n"].concat()))
                .read_request()
                .is_err()
        );
    }
}

#[test]
fn repair_keeps_handler_surrogate_escapes_and_go_whitespace_domain_separate() {
    let body = br#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"skills_history","arguments":{"\ud800":1e9999,"name":"\ud800"}}}"#;
    let request = transport_request(body).unwrap().unwrap();
    assert!(
        request
            .params
            .unwrap()
            .get()
            .contains(r#"{"\ud800":1e9999,"name":"\ud800"}"#)
    );
    for space in ['\u{0085}', '\u{2007}', '\u{2028}', '\u{3000}'] {
        let mut data = space.to_string().into_bytes();
        data.extend_from_slice(b"\xff");
        data.extend_from_slice(space.to_string().as_bytes());
        assert_eq!(trim_go_space(&data), b"\xff");
    }
    assert_eq!(trim_go_space(b"\xff \xfe"), b"\xff \xfe");
    assert!(syntax::validate(b"{\xff}").is_err());
    assert!(transport_request(b"{\xff}").is_none());
}
