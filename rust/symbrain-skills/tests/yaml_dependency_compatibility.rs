//! Dependency migration controls, not acceptance of the open Go encoder residuals.

use serde_json::json;
use symbrain_skills::{RenderMetadata, load_bundle, parse_skill_md, render_target};

#[test]
fn yaml_dependency_preserves_scalar_types_and_rendered_documents() {
    for (yaml, expected) in [
        ("'2'", "2"),
        ("2", "2"),
        ("2.5", "2.5"),
        ("true", "true"),
        ("yes", "yes"),
        ("0123", "0123"),
        ("null", ""),
    ] {
        let input =
            format!("---\nname: yaml-compat\ndescription: test\nversion: {yaml}\n---\nBody.\n");
        let parsed = parse_skill_md(input.as_bytes()).expect("scalar frontmatter");
        assert_eq!(parsed.frontmatter.version, expected, "scalar {yaml}");
        assert_eq!(parsed.body, "Body.\n");
    }

    let input = concat!(
        "---\nname: yaml-compat\ndescription: Typed metadata\nversion: '2'\n",
        "metadata:\n",
        "  quoted: '2'\n  integer: 2\n  float: 2.5\n  boolean: true\n",
        "  absent: null\n  maximum: 18446744073709551615\n",
        "  nested: {text: string, integer: 2, boolean: true, absent: null}\n",
        "  controls: \"line\\n\\0\\r\\nend\"\n",
        "---\n\nOpaque body.\n"
    );
    let expected_metadata = json!({
        "quoted": "2", "integer": 2, "float": 2.5, "boolean": true,
        "absent": null, "maximum": u64::MAX,
        "nested": {"text": "string", "integer": 2, "boolean": true, "absent": null},
        "controls": "line\n\0\r\nend"
    });
    let parsed = parse_skill_md(input.as_bytes()).expect("typed frontmatter");
    assert_eq!(parsed.frontmatter.metadata, expected_metadata);

    // Parsing mixed sequences is covered without pretending that the separate
    // shared-encoder sequence-indentation residual (#835) has been repaired.
    let sequence = parse_skill_md(
        b"---\nname: yaml-compat\ndescription: test\nmetadata: {nested: [text, 2, true, null]}\n---\nBody.\n",
    )
    .expect("typed metadata sequence");
    assert_eq!(
        sequence.frontmatter.metadata["nested"],
        json!(["text", 2, true, null])
    );

    let root = tempfile::tempdir().expect("source root");
    std::fs::write(root.path().join("SKILL.md"), input).expect("source skill");
    let bundle = load_bundle(root.path()).expect("source bundle");
    let rendered =
        render_target(&bundle, "opencode", &RenderMetadata::default()).expect("rendered document");
    let reparsed = parse_skill_md(&rendered.skill_md).expect("valid rendered YAML");
    assert_eq!(reparsed.frontmatter.version, "2");
    assert_eq!(reparsed.frontmatter.metadata, expected_metadata);
    assert_eq!(reparsed.body, "Opaque body.\n");

    // Captured from main's serde_yaml_ng 0.10.0, not a new Go parity claim (#835).
    let legacy_bytes = concat!(
        "---\nname: yaml-compat\ndescription: Typed metadata\nversion: '2'\n",
        "compatibility: opencode\nmetadata:\n",
        "    absent: null\n    boolean: true\n",
        "    controls: \"line\\n\\0\\r\\nend\"\n",
        "    float: 2.5\n    integer: 2\n    maximum: 18446744073709551615\n",
        "    nested:\n        absent: null\n        boolean: true\n",
        "        integer: 2\n        text: string\n    quoted: '2'\n",
        "---\n\nOpaque body.\n"
    );
    assert_eq!(rendered.skill_md, legacy_bytes.as_bytes());

    for malformed in ["name: [unclosed", "name: one\nname: two", "name: *missing"] {
        let input = format!("---\n{malformed}\n---\nBody.\n");
        let error = parse_skill_md(input.as_bytes()).expect_err("invalid YAML rejected");
        assert!(error.0.starts_with("parse SKILL.md frontmatter:"));
    }
}
