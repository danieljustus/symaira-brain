//! Frozen numeric loader normalization is separate from lossless string emission.
use serde_json::json;
use symbrain_skills::parse_skill_md;

#[test]
fn numeric_loader_parity_retains_approved_strings() {
    let raw = b"---\nname: probe\ndescription: \"\\n\\nX\"\nmetadata:\n  values: [-0.0, 1.0, 1e6, 1e-10]\n  nested: {\"<<\": \"\\n\"}\n---\nbody\0\xff\r\n";
    let parsed = parse_skill_md(raw).unwrap();
    assert_eq!(parsed.frontmatter.description, "\n\nX");
    assert_eq!(
        parsed.frontmatter.metadata["values"],
        json!([0, 1, 1e6, 1e-10])
    );
    assert!(parsed.frontmatter.metadata["values"][0].is_i64());
    assert!(parsed.frontmatter.metadata["values"][1].is_i64());
    assert!(parsed.frontmatter.metadata["values"][2].is_f64());
    assert_eq!(parsed.frontmatter.metadata["nested"]["<<"], "\n");
    assert_eq!(parsed.body.as_bytes(), b"body\0\xff\n");
}
