//! Real Go (`BurntSushi/toml` v1.6.0 via configkit v0.18.2) parse outcomes.
use super::parse;

/// `(name, input, Some(Go message) | None when Go accepted the document)`.
const CORPUS: &[(&str, &[u8], Option<&str>)] = &[
    ("eq-eq", b"\x74\x68\x69\x73\x20\x69\x73\x20\x3d\x20\x3d\x20\x6e\x6f\x74\x20\x74\x6f\x6d\x6c\x0a", Some("toml: line 1: expected '.' or '=', but got 'i' instead")),
    ("novalue", b"\x61\x20\x3d\x20\x0a", Some("toml: line 1 (last key \"a\"): expected value but found '\\n' instead")),
    ("novalue-eof", b"\x61\x20\x3d", Some("toml: line 1 (last key \"a\"): unexpected EOF; expected value")),
    ("barekey", b"\x61\x0a", Some("toml: line 1: expected '.' or '=', but got '\\n' instead")),
    ("unterm-str", b"\x61\x20\x3d\x20\x22\x75\x6e\x74\x65\x72\x6d\x69\x6e\x61\x74\x65\x64\x0a", Some("toml: line 1 (last key \"a\"): strings cannot contain newlines")),
    ("unterm-str-eof", b"\x61\x20\x3d\x20\x22\x78", Some("toml: line 1 (last key \"a\"): unexpected EOF; expected '\"'")),
    ("dupkey", b"\x61\x20\x3d\x20\x31\x0a\x61\x20\x3d\x20\x32\x0a", Some("toml: line 2 (last key \"a\"): Key 'a' has already been defined.")),
    ("tbl-unclosed", b"\x5b\x74\x62\x6c\x0a", Some("toml: line 2: expected '.' or ']' to end table name, but got '\\n' instead")),
    ("dup-table", b"\x5b\x61\x5d\x0a\x5b\x61\x5d\x0a", Some("toml: line 2: Key 'a' has already been defined.")),
    ("arr-unclosed", b"\x69\x6e\x76\x61\x6c\x69\x64\x20\x3d\x20\x5b\x0a", Some("toml: line 1 (last key \"invalid\"): unexpected EOF; expected value")),
    ("arr-unclosed2", b"\x61\x20\x3d\x20\x5b\x31\x2c\x20\x32\x0a", Some("toml: line 1 (last key \"a\"): expected a comma (',') or array terminator (']'), but got end of file")),
    ("lead0", b"\x61\x20\x3d\x20\x30\x31\x0a", Some("toml: line 1 (last key \"a\"): Invalid integer \"01\": cannot have leading zeroes")),
    ("tru", b"\x61\x20\x3d\x20\x74\x72\x75\x0a", Some("toml: line 1 (last key \"a\"): expected value but found \"tru\" instead")),
    ("empty-tbl", b"\x5b\x5d\x0a", Some("toml: line 1: unexpected end of table name (table names cannot be empty)")),
    ("sq-unterm", b"\x61\x20\x3d\x20\x27\x78\x0a", Some("toml: line 1 (last key \"a\"): strings cannot contain newlines")),
    ("noKey", b"\x3d\x20\x31\x0a", Some("toml: line 1: unexpected '=': key name appears blank")),
    ("dotted-conflict", b"\x61\x2e\x62\x20\x3d\x20\x31\x0a\x61\x20\x3d\x20\x32\x0a", None),
    ("two-on-line", b"\x61\x20\x3d\x20\x31\x20\x62\x20\x3d\x20\x32\x0a", Some("toml: line 1: expected a top-level item to end with a newline, comment, or EOF, but got 'b' instead")),
    ("inline-trailing", b"\x61\x20\x3d\x20\x7b\x78\x3d\x31\x2c\x7d\x0a", None),
    ("bad-escape", b"\x61\x20\x3d\x20\x22\x5c\x71\x22\x0a", Some("toml: line 1 (last key \"a\"): invalid escape in string '\\q'")),
    ("badutf", b"\x61\x20\x3d\x20\x22\xff\x22\x0a", Some("toml: line 1 (last key \"a\"): invalid UTF-8 byte: 0xff")),
    ("badutf2", b"\xff\xff\x20\x3d\x20\x31\x0a", Some("toml: line 1: invalid UTF-8 byte: 0xff")),
    ("ctrl", b"\x61\x20\x3d\x20\x22\x01\x22\x0a", Some("toml: line 1 (last key \"a\"): TOML files cannot contain control characters: '0x01'")),
    ("aot-conflict", b"\x5b\x5b\x61\x5d\x5d\x0a\x5b\x61\x5d\x0a", Some("toml: line 2: Key 'a' has already been defined.")),
    ("bad-date", b"\x61\x20\x3d\x20\x31\x39\x37\x39\x2d\x31\x33\x2d\x30\x31\x0a", Some("toml: line 1 (last key \"a\"): invalid datetime: \"1979-13-01\"")),
    ("qkey-unterm", b"\x22\x75\x6e\x74\x65\x72\x6d\x69\x6e\x61\x74\x65\x64\x20\x3d\x20\x31\x0a", Some("toml: line 1: strings cannot contain newlines")),
    ("underscore", b"\x61\x20\x3d\x20\x31\x5f\x0a", Some("toml: line 1 (last key \"a\"): Invalid integer \"1_\": underscores must be surrounded by digits")),
    ("plus", b"\x61\x20\x3d\x20\x2b\x0a", Some("toml: line 2 (last key \"a\"): expected a digit but got '\\n'")),
    ("dot-end", b"\x5b\x61\x2e\x5d\x0a", Some("toml: line 1: unexpected end of table name (table names cannot be empty)")),
    ("bom8", b"\xef\xbb\xbf\x3d\x20\x31\x0a", Some("toml: line 1: unexpected '=': key name appears blank")),
    ("bom16le", b"\xff\xfe\x61\x00\x20\x00\x3d\x00\x20\x00\x31\x00\x0a\x00", Some("toml: line 1: files cannot contain NULL bytes; probably using UTF-16; TOML files must be UTF-8")),
    ("bom16be", b"\xfe\xff\x00\x61\x00\x20\x00\x3d\x00\x20\x00\x31\x00\x0a", Some("toml: line 1: files cannot contain NULL bytes; probably using UTF-16; TOML files must be UTF-8")),
    ("multi-line-2", b"\x6f\x6b\x20\x3d\x20\x31\x0a\x62\x61\x64\x20\x62\x61\x64\x20\x3d\x20\x32\x0a", Some("toml: line 2: expected '.' or '=', but got 'b' instead")),
    ("line3", b"\x0a\x0a\x78\x20\x3d\x20\x3d\x20\x31\x0a", Some("toml: line 3 (last key \"x\"): expected value but found '=' instead")),
    ("crlf", b"\x61\x20\x3d\x20\x31\x0d\x0a\x62\x20\x3d\x20\x3d\x20\x32\x0d\x0a", Some("toml: line 2 (last key \"b\"): expected value but found '=' instead")),
    ("nested-key", b"\x5b\x6d\x6f\x64\x75\x6c\x65\x73\x5d\x0a\x62\x72\x6f\x77\x73\x65\x20\x3d\x20\x3d\x20\x74\x72\x75\x65\x0a", Some("toml: line 2 (last key \"modules.browse\"): expected value but found '=' instead")),
    ("tbl-key", b"\x5b\x61\x5d\x0a\x62\x20\x3d\x20\x0a", Some("toml: line 2 (last key \"a.b\"): expected value but found '\\n' instead")),
    ("arr-mixed-ok", b"\x61\x20\x3d\x20\x5b\x31\x2c\x20\x22\x78\x22\x5d\x0a", None),
    ("int-overflow", b"\x61\x20\x3d\x20\x39\x39\x39\x39\x39\x39\x39\x39\x39\x39\x39\x39\x39\x39\x39\x39\x39\x39\x39\x39\x0a", Some("toml: line 1 (last key \"a\"): 99999999999999999999 is out of range for int64")),
    ("float-bad", b"\x61\x20\x3d\x20\x31\x2e\x0a", Some("toml: line 1 (last key \"a\"): Invalid float \"1.\": '.' must be followed by one or more digits")),
    ("hex-bad", b"\x61\x20\x3d\x20\x30\x78\x5a\x5a\x0a", Some("toml: line 1 (last key \"a\"): not a hexadecimal number: '0xZ'")),
    ("quoted-bad-key", b"\x22\x61\x22\x62\x20\x3d\x20\x31\x0a", Some("toml: line 1: expected '.' or '=', but got 'b' instead")),
    ("nul", b"\x61\x20\x3d\x20\x31\x00\x0a", Some("toml: line 1: files cannot contain NULL bytes; probably using UTF-16; TOML files must be UTF-8")),
    ("cr-alone", b"\x61\x20\x3d\x20\x31\x0d\x62\x20\x3d\x20\x32\x0a", Some("toml: line 1 (last key \"a\"): TOML files cannot contain control characters: '0x0d'")),
    ("unterm-ml", b"\x61\x20\x3d\x20\x22\x22\x22\x78\x0a", Some("toml: line 1 (last key \"a\"): unexpected EOF; expected '\"\"\"'")),
    ("tab-key", b"\x61\x09\x3d\x20\x0a", Some("toml: line 1 (last key \"a\"): expected value but found '\\n' instead")),
    ("inline-newline", b"\x61\x20\x3d\x20\x7b\x78\x3d\x0a\x31\x7d\x0a", Some("toml: line 1 (last key \"a.x\"): expected value but found '\\n' instead")),
    ("key-str-after-val", b"\x61\x20\x3d\x20\x22\x78\x22\x20\x22\x79\x22\x0a", Some("toml: line 1: expected a top-level item to end with a newline, comment, or EOF, but got '\"' instead")),
    ("tbl-then-val", b"\x5b\x61\x5d\x20\x62\x0a", Some("toml: line 1 (last key \"a\"): expected a top-level item to end with a newline, comment, or EOF, but got 'b' instead")),
    ("hash-only", b"\x23\x20\x63\x0a", None),
    ("time-1.1", b"\x61\x20\x3d\x20\x31\x32\x3a\x33\x30\x0a", None),
    ("inline-multi-1.1", b"\x61\x20\x3d\x20\x7b\x78\x3d\x31\x2c\x0a\x20\x79\x3d\x32\x7d\x0a", None),
    ("esc-e", b"\x61\x20\x3d\x20\x22\x5c\x65\x22\x0a", None),
    ("esc-x", b"\x61\x20\x3d\x20\x22\x5c\x78\x34\x31\x22\x0a", None),
    ("ctx-nokey", b"\x5b\x61\x5d\x0a\x62\x20\x63\x20\x3d\x20\x31\x0a", Some("toml: line 2 (last key \"a\"): expected '.' or '=', but got 'c' instead")),
    ("ctx-novalue", b"\x5b\x61\x2e\x62\x5d\x0a\x63\x20\x3d\x20\x3d\x20\x31\x0a", Some("toml: line 2 (last key \"a.b.c\"): expected value but found '=' instead")),
    ("ctx-dup", b"\x5b\x61\x5d\x0a\x62\x20\x3d\x20\x31\x0a\x62\x20\x3d\x20\x32\x0a", Some("toml: line 3 (last key \"a.b\"): Key 'a.b' has already been defined.")),
    ("ctx-duptbl", b"\x5b\x61\x2e\x62\x5d\x0a\x5b\x61\x2e\x62\x5d\x0a", Some("toml: line 2 (last key \"a\"): Key 'a.b' has already been defined.")),
    ("ctx-two", b"\x5b\x61\x5d\x0a\x62\x20\x3d\x20\x31\x20\x63\x20\x3d\x20\x32\x0a", Some("toml: line 2 (last key \"a\"): expected a top-level item to end with a newline, comment, or EOF, but got 'c' instead")),
    ("ctx-hdr-unclosed", b"\x5b\x61\x5d\x0a\x5b\x62\x0a", Some("toml: line 3 (last key \"a\"): expected '.' or ']' to end table name, but got '\\n' instead")),
    ("dup-dotted", b"\x61\x2e\x62\x20\x3d\x20\x31\x0a\x61\x2e\x62\x20\x3d\x20\x32\x0a", Some("toml: line 2 (last key \"a.b\"): Key 'a.b' has already been defined.")),
    ("quoted", b"\x22\x61\x20\x62\x22\x20\x3d\x20\x3d\x20\x31\x0a", Some("toml: line 1 (last key \"a b\"): expected value but found '=' instead")),
    ("aot", b"\x5b\x5b\x61\x5d\x5d\x0a\x62\x20\x3d\x20\x3d\x20\x31\x0a", Some("toml: line 2 (last key \"a.b\"): expected value but found '=' instead")),
    ("hdr-junk", b"\x5b\x61\x5d\x20\x23\x20\x63\x0a\x62\x20\x63\x0a", Some("toml: line 2 (last key \"a\"): expected '.' or '=', but got 'c' instead")),
    ("hdr-eof", b"\x5b\x61", Some("toml: line 1: expected '.' or ']' to end table name, but got '\\x00' instead")),
    ("key-eof", b"\x61", Some("toml: line 1: unexpected EOF; expected key separator '='")),
    ("val-eof-sp", b"\x61\x20\x3d\x20", Some("toml: line 1 (last key \"a\"): unexpected EOF; expected value")),
    ("bad-char-key", b"\x40\x20\x3d\x20\x31\x0a", Some("toml: line 1: expected '.' or '=', but got '@' instead")),
    ("str-nl-ml", b"\x61\x20\x3d\x20\x22\x78\x0a\x79\x22\x0a", Some("toml: line 1 (last key \"a\"): strings cannot contain newlines")),
    ("lit-eof", b"\x61\x20\x3d\x20\x27\x78", Some("toml: line 1 (last key \"a\"): unexpected EOF; expected \"'\"")),
    ("trail-comma-arr-eof", b"\x61\x20\x3d\x20\x5b\x31\x2c\x0a", Some("toml: line 1 (last key \"a\"): unexpected EOF; expected value")),
    ("inline-unclosed", b"\x61\x20\x3d\x20\x7b\x78\x20\x3d\x20\x31\x0a", Some("toml: line 1 (last key \"a.x\"): expected a comma or an inline table terminator '}', but got end of file instead")),
    ("inline-eof", b"\x61\x20\x3d\x20\x7b\x78\x20\x3d\x20\x31", Some("toml: line 1 (last key \"a.x\"): expected a comma or an inline table terminator '}', but got end of file instead")),
    ("dup-arr", b"\x61\x20\x3d\x20\x31\x0a\x5b\x61\x5d\x0a", Some("toml: line 2: Key 'a' has already been defined.")),
    ("tbl-key-dup", b"\x5b\x61\x5d\x0a\x5b\x61\x2e\x62\x5d\x0a\x63\x3d\x31\x0a\x5b\x61\x2e\x62\x5d\x0a", Some("toml: line 4 (last key \"a\"): Key 'a.b' has already been defined.")),
    ("val-bracket", b"\x61\x20\x3d\x20\x5d\x0a", Some("toml: line 1 (last key \"a\"): expected value but found ']' instead")),
    ("val-comma", b"\x61\x20\x3d\x20\x2c\x0a", Some("toml: line 1 (last key \"a\"): expected value but found ',' instead")),
    ("val-comment", b"\x61\x20\x3d\x20\x23\x20\x78\x0a", Some("toml: line 1 (last key \"a\"): expected value but found '#' instead")),
    ("val-brace", b"\x61\x20\x3d\x20\x7d\x0a", Some("toml: line 1 (last key \"a\"): expected value but found '}' instead")),
    ("float-lead", b"\x61\x20\x3d\x20\x2e\x35\x0a", Some("toml: line 1 (last key \"a\"): floats must start with a digit, not '.'")),
    ("int-spaces", b"\x61\x20\x3d\x20\x31\x20\x32\x0a", Some("toml: line 1: expected a top-level item to end with a newline, comment, or EOF, but got '2' instead")),
    ("tab-lead", b"\x09\x09\x78\x20\x79\x20\x3d\x20\x31\x0a", Some("toml: line 1: expected '.' or '=', but got 'y' instead")),
    ("num-trailing", b"\x61\x20\x3d\x20\x31\x32\x61\x62\x63\x0a", Some("toml: line 1: expected a top-level item to end with a newline, comment, or EOF, but got 'a' instead")),
    ("bool-cap", b"\x61\x20\x3d\x20\x54\x72\x75\x65\x0a", Some("toml: line 1 (last key \"a\"): expected value but found \"True\" instead")),
    ("uni-val", b"\x61\x20\x3d\x20\xc3\xa9\x0a", Some("toml: line 1 (last key \"a\"): expected value but found \"é\" instead")),
    ("uni-key", b"\xc3\xa9\x20\xc3\xa9\x20\x3d\x20\x31\x0a", Some("toml: line 1: expected '.' or '=', but got 'é' instead")),
];

/// Shapes whose Go lexer wording was not reproduced; `toml_edit` text is kept.
const UNMATCHED: &[&str] = &[
    // BurntSushi accepts a dotted key later redefined as a scalar; toml_edit rejects it.
    "dotted-conflict",
    // BurntSushi rejects a newline inside an inline table value; toml_edit (TOML 1.1) accepts it.
    "inline-newline",
];

#[test]
fn corpus_matches_the_recorded_go_outcomes() {
    let mut mismatches = Vec::new();
    for (name, input, expected) in CORPUS {
        let actual = parse(input).err();
        if actual.as_deref() != *expected {
            mismatches.push(format!("{name}: go={expected:?} rust={actual:?}"));
        }
    }
    let unexpected: Vec<&String> = mismatches
        .iter()
        .filter(|line| {
            !UNMATCHED
                .iter()
                .any(|name| line.starts_with(&format!("{name}:")))
        })
        .collect();
    assert!(
        unexpected.is_empty(),
        "{} mismatches:\n{}",
        unexpected.len(),
        unexpected
            .iter()
            .map(|l| l.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
