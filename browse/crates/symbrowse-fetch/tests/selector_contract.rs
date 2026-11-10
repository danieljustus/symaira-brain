//! FETCH-007 selector family: the bounded CSS subset accepted by
//! `dom::select`. No sealed selector fixture exists (the Go `goquery`
//! oracle was removed), so these Rust behavior tests are the reference.
use symbrowse_fetch::dom::{self, Node};

const PAGE: &[u8] = br#"<html><body>
<header id="top" class="bar wide"><a href="/home" data-kind="nav-main" lang="en-GB">Home</a></header>
<main class="content">
  <section class="intro first"><p class="lead">one</p><p>two</p></section>
  <section class="outro"><div><p class="lead deep">three</p></div><p>four</p></section>
</main>
<footer><a href="https://example.test/x.pdf" data-kind="ext">PDF</a></footer>
</body></html>"#;

fn texts(selector: &str) -> Vec<String> {
    let tree = dom::parse(PAGE).unwrap();
    dom::select(&tree.root, selector)
        .unwrap_or_else(|error| panic!("{selector}: {error}"))
        .iter()
        .map(|node| {
            dom::text_content(std::slice::from_ref(node))
                .trim()
                .to_owned()
        })
        .collect()
}

fn error(selector: &str) -> String {
    let tree = dom::parse(PAGE).unwrap();
    dom::select(&tree.root, selector)
        .err()
        .unwrap_or_else(|| panic!("{selector} must not match"))
        .to_string()
}

#[test]
fn simple_selectors_match_in_document_order() {
    assert_eq!(texts("p"), ["one", "two", "three", "four"]);
    assert_eq!(texts("#top"), ["Home"]);
    assert_eq!(texts(".lead"), ["one", "three"]);
    assert_eq!(texts("p.lead.deep"), ["three"]);
    assert_eq!(texts("header.bar.wide#top"), ["Home"]);
    assert_eq!(texts("SECTION.outro"), ["three four"]);
}

#[test]
fn combinators_distinguish_child_from_descendant() {
    assert_eq!(texts("main p"), ["one", "two", "three", "four"]);
    assert_eq!(texts("section > p"), ["one", "two", "four"]);
    assert_eq!(texts("section>p"), ["one", "two", "four"]);
    assert_eq!(texts("section p.lead"), ["one", "three"]);
    assert_eq!(texts("main > section > div > p"), ["three"]);
    assert_eq!(
        error("main > p"),
        "selector \"main > p\" matched no elements"
    );
}

#[test]
fn attribute_operators_follow_css_semantics() {
    assert_eq!(texts("a[href]"), ["Home", "PDF"]);
    assert_eq!(texts("a[data-kind=ext]"), ["PDF"]);
    assert_eq!(texts("a[data-kind=\"ext\"]"), ["PDF"]);
    assert_eq!(texts("a[data-kind^=nav]"), ["Home"]);
    assert_eq!(texts("a[href$='.pdf']"), ["PDF"]);
    assert_eq!(texts("a[href*=example]"), ["PDF"]);
    assert_eq!(texts("a[lang|=en]"), ["Home"]);
    assert_eq!(texts("section[class~=first]"), ["one two"]);
    assert_eq!(texts("a[data-kind!=ext]"), ["Home"]);
    assert_eq!(texts("a[href][data-kind=ext]"), ["PDF"]);
}

#[test]
fn comma_groups_return_each_node_once_in_document_order() {
    assert_eq!(texts("p.lead, header"), ["Home", "one", "three"]);
    assert_eq!(texts("p, p.lead"), ["one", "two", "three", "four"]);
}

#[test]
fn no_match_and_malformed_selectors_report_the_selector() {
    assert_eq!(error("table"), "selector \"table\" matched no elements");
    assert_eq!(error(""), "selector \"\" matched no elements");
    for malformed in ["p >", "> p", "p > > a", ","] {
        assert_eq!(
            error(malformed),
            format!("selector {malformed:?} matched no elements")
        );
    }
    assert_eq!(
        error(".lead.missing"),
        "selector \".lead.missing\" matched no elements"
    );
    assert_eq!(error("a[href"), "selector \"a[href\" matched no elements");
}

#[test]
fn selector_size_is_bounded() {
    let deep = vec!["div"; 65].join(" ");
    assert_eq!(
        error(&deep),
        format!("selector {deep:?} matched no elements")
    );
    let wide = vec!["p"; 33].join(",");
    assert_eq!(
        error(&wide),
        format!("selector {wide:?} matched no elements")
    );
}

#[test]
fn selection_returns_clones_that_leave_the_tree_untouched() {
    let tree = dom::parse(PAGE).unwrap();
    let before = dom::serialize(&tree.root);
    let nodes = dom::select(&tree.root, "section").unwrap();
    assert!(
        nodes
            .iter()
            .all(|node| matches!(node, Node::Element { .. }))
    );
    assert_eq!(dom::serialize(&tree.root), before);
}
