//! `symbrowse read`: port of the Go CLI handler plus the `domkit` rendering
//! pipeline it calls (HTML → markdown, outline, frontmatter, truncation).

use html5ever::{ParseOpts, parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use serde_json::{Map, Value, json};
use symbrowse_core::{
    error::ErrorCode,
    go_value::{go_struct, to_go_any},
    injection_boundary::Boundary,
    output::Envelope,
    yaml_str,
};
use time::OffsetDateTime;

use crate::app::{CliError, Ctx, Handled, daemon_request, response_error};

const DEFAULT_MAX_CHARS: usize = 15_000;
const NO_INPUT: u8 = 66;

#[derive(Clone, Debug)]
enum Node {
    Document(Vec<Node>),
    Element {
        tag: String,
        attrs: Vec<(String, String)>,
        children: Vec<Node>,
    },
    Text(String),
    Comment(String),
    Doctype(String),
}

fn convert(handle: &Handle) -> Node {
    let children =
        |handle: &Handle| -> Vec<Node> { handle.children.borrow().iter().map(convert).collect() };
    match &handle.data {
        NodeData::Document => Node::Document(children(handle)),
        NodeData::Text { contents } => Node::Text(contents.borrow().to_string()),
        NodeData::Comment { contents } => Node::Comment(contents.to_string()),
        NodeData::Doctype { name, .. } => Node::Doctype(name.to_string()),
        NodeData::Element {
            name,
            attrs,
            template_contents,
            ..
        } => {
            let mut kids = children(handle);
            if let Some(contents) = template_contents.borrow().as_ref() {
                kids.extend(contents.children.borrow().iter().map(convert));
            }
            Node::Element {
                tag: name.local.to_string(),
                attrs: attrs
                    .borrow()
                    .iter()
                    .map(|a| (a.name.local.to_string(), a.value.to_string()))
                    .collect(),
                children: kids,
            }
        }
        NodeData::ProcessingInstruction { .. } => Node::Comment(String::new()),
    }
}

fn parse(source: &str) -> Node {
    let dom = parse_document(RcDom::default(), ParseOpts::default())
        .from_utf8()
        .read_from(&mut source.as_bytes())
        .expect("reading from memory cannot fail");
    convert(&dom.document)
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn is_skipped(tag: &str) -> bool {
    matches!(
        tag,
        "script"
            | "style"
            | "noscript"
            | "template"
            | "head"
            | "title"
            | "meta"
            | "link"
            | "iframe"
    )
}

fn compress_whitespace(text: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in text.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            out.push(c);
            space = false;
        }
    }
    out
}

fn collect_text(node: &Node, out: &mut String) {
    match node {
        Node::Text(text) => out.push_str(text),
        Node::Element { tag, children, .. } => {
            if is_skipped(tag) {
                return;
            }
            for child in children {
                collect_text(child, out);
            }
        }
        Node::Document(children) => {
            for child in children {
                collect_text(child, out);
            }
        }
        _ => {}
    }
}

fn node_text(node: &Node) -> String {
    let mut raw = String::new();
    collect_text(node, &mut raw);
    compress_whitespace(&raw)
}

fn render_children(
    children: &[Node],
    out: &mut String,
    depth: usize,
    in_pre: bool,
    parent: Option<&str>,
) {
    for child in children {
        render_node(child, out, depth, in_pre, parent);
    }
}

fn render_node(node: &Node, out: &mut String, depth: usize, in_pre: bool, parent: Option<&str>) {
    match node {
        Node::Text(text) => {
            if in_pre {
                out.push_str(text);
            } else {
                out.push_str(&compress_whitespace(text));
            }
        }
        Node::Comment(_) | Node::Doctype(_) => {}
        Node::Document(children) => render_children(children, out, depth, in_pre, None),
        Node::Element {
            tag,
            attrs,
            children,
        } => {
            render_element(tag, attrs, children, out, depth, in_pre, parent);
        }
    }
}

fn render_element(
    tag: &str,
    attrs: &[(String, String)],
    children: &[Node],
    out: &mut String,
    depth: usize,
    in_pre: bool,
    parent: Option<&str>,
) {
    let tag_lower = tag.to_lowercase();
    let tag = tag_lower.as_str();
    if is_skipped(tag) {
        return;
    }
    let element = Node::Element {
        tag: tag.to_owned(),
        attrs: attrs.to_vec(),
        children: children.to_vec(),
    };
    match tag {
        "br" => {
            out.push('\n');
            return;
        }
        "img" => {
            let alt = attr(attrs, "alt").unwrap_or_default();
            let src = attr(attrs, "src").unwrap_or_default();
            if !src.is_empty() {
                out.push_str(&format!("![{alt}]({src})"));
            }
            return;
        }
        "a" => {
            let href = attr(attrs, "href").unwrap_or_default();
            let mut text = node_text(&element);
            if text.trim().is_empty() {
                text = href.to_owned();
            }
            if href.is_empty() || href.starts_with('#') {
                out.push_str(&text);
            } else {
                out.push_str(&format!("[{text}]({href})"));
            }
            return;
        }
        "strong" | "b" => {
            if !node_text(&element).trim().is_empty() {
                out.push_str("**");
                render_children(children, out, depth, in_pre, Some(tag));
                out.push_str("**");
            }
            return;
        }
        "em" | "i" => {
            if !node_text(&element).trim().is_empty() {
                out.push('*');
                render_children(children, out, depth, in_pre, Some(tag));
                out.push('*');
            }
            return;
        }
        "code" => {
            if parent.is_some_and(|p| p.eq_ignore_ascii_case("pre")) {
                render_children(children, out, depth, true, Some(tag));
                return;
            }
            let text = node_text(&element);
            if !text.trim().is_empty() {
                out.push('`');
                out.push_str(&text);
                out.push('`');
            }
            return;
        }
        _ => {}
    }
    let block = match tag {
        "h1" => Some("# "),
        "h2" => Some("## "),
        "h3" => Some("### "),
        "h4" => Some("#### "),
        "h5" => Some("##### "),
        "h6" => Some("###### "),
        "p" => Some(""),
        "pre" => Some("```\n"),
        "hr" => Some("---"),
        _ => None,
    };
    if let Some(marker) = block {
        out.push_str("\n\n");
        out.push_str(marker);
        if tag == "pre" {
            render_children(children, out, depth, true, Some(tag));
            out.push_str("\n```");
            return;
        }
        render_children(children, out, depth, in_pre, Some(tag));
        out.push_str("\n\n");
        return;
    }
    if matches!(tag, "ul" | "ol") {
        out.push_str("\n\n");
        let marker = if tag == "ul" { "- " } else { "1. " };
        let depth = depth + 1;
        for child in children {
            if let Node::Element {
                tag: child_tag,
                children: item,
                ..
            } = child
                && child_tag.eq_ignore_ascii_case("li")
            {
                out.push_str(&"  ".repeat(depth - 1));
                out.push_str(marker);
                render_children(item, out, depth, false, Some("li"));
                out.push('\n');
            }
        }
        return;
    }
    render_children(children, out, depth, in_pre, Some(tag));
}

fn markdown(root: &Node) -> String {
    let mut out = String::new();
    render_node(root, &mut out, 0, false, None);
    out.trim_matches(|c: char| c.is_whitespace()).to_owned()
}

#[derive(Clone)]
struct Heading {
    level: usize,
    text: String,
}

fn collect_headings(node: &Node, out: &mut Vec<Heading>) {
    let children: &[Node] = match node {
        Node::Document(children) => children,
        Node::Element { tag, children, .. } => {
            let bytes = tag.as_bytes();
            if bytes.len() == 2 && bytes[0] == b'h' && (b'1'..=b'6').contains(&bytes[1]) {
                let text = node_text(node).trim().to_owned();
                if !text.is_empty() {
                    out.push(Heading {
                        level: usize::from(bytes[1] - b'0'),
                        text,
                    });
                }
            }
            children
        }
        _ => return,
    };
    for child in children {
        collect_headings(child, out);
    }
}

fn lang_of(node: &Node) -> String {
    match node {
        Node::Element {
            tag,
            attrs,
            children,
        } => {
            if tag == "html"
                && let Some(lang) = attr(attrs, "lang")
                && !lang.is_empty()
            {
                return lang.to_owned();
            }
            children
                .iter()
                .map(lang_of)
                .find(|l| !l.is_empty())
                .unwrap_or_default()
        }
        Node::Document(children) => children
            .iter()
            .map(lang_of)
            .find(|l| !l.is_empty())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn first_ld_json(node: &Node) -> Option<String> {
    match node {
        Node::Element {
            tag,
            attrs,
            children,
        } => {
            if tag == "script"
                && attrs
                    .iter()
                    .any(|(k, v)| k == "type" && v == "application/ld+json")
                && let Some(Node::Text(text)) = children.first()
            {
                return Some(text.clone());
            }
            children.iter().find_map(first_ld_json)
        }
        Node::Document(children) => children.iter().find_map(first_ld_json),
        _ => None,
    }
}

fn schema_type(node: &Node) -> String {
    let Some(raw) = first_ld_json(node).filter(|raw| !raw.is_empty()) else {
        return String::new();
    };
    let Ok(Value::Object(data)) = serde_json::from_str::<Value>(&raw) else {
        return String::new();
    };
    for key in ["@type", "type"] {
        if let Some(Value::String(value)) = data.get(key) {
            return value.clone();
        }
    }
    String::new()
}

// --- selection -------------------------------------------------------------

fn split_selector(selector: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in selector.chars() {
        match quote {
            Some(q) => {
                current.push(c);
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    current.push(c);
                }
                ' ' | '\t' | '\n' | '\r' | '\u{c}' => {
                    if !current.is_empty() {
                        parts.push(std::mem::take(&mut current));
                    }
                }
                _ => current.push(c),
            },
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

fn matches_simple(tag: &str, attrs: &[(String, String)], part: &str) -> bool {
    match part.chars().next() {
        None => false,
        Some('.') => attr(attrs, "class")
            .unwrap_or_default()
            .split_whitespace()
            .any(|token| token == &part[1..]),
        Some('#') => attr(attrs, "id").unwrap_or_default() == &part[1..],
        Some('[') => {
            let inner = part.strip_prefix('[').unwrap_or(part);
            let inner = inner.strip_suffix(']').unwrap_or(inner);
            match inner.find('=') {
                None => attr(attrs, inner).is_some(),
                Some(index) => {
                    let key = &inner[..index];
                    let want = inner[index + 1..].trim_matches(|c| c == '"' || c == '\'');
                    attr(attrs, key).is_some_and(|value| value == want)
                }
            }
        }
        Some(_) => tag == part,
    }
}

fn matches_selector(node: &Node, selector: &str) -> bool {
    let Node::Element { tag, attrs, .. } = node else {
        return false;
    };
    let parts = split_selector(selector);
    !parts.is_empty() && parts.iter().all(|part| matches_simple(tag, attrs, part))
}

fn find_subtree(node: &Node, selector: &str) -> Option<Node> {
    if matches_selector(node, selector) {
        return Some(node.clone());
    }
    let children: &[Node] = match node {
        Node::Document(children) | Node::Element { children, .. } => children,
        _ => return None,
    };
    children
        .iter()
        .find_map(|child| find_subtree(child, selector))
}

fn remove_matching(node: &mut Node, selector: &str) {
    let children = match node {
        Node::Document(children) | Node::Element { children, .. } => children,
        _ => return,
    };
    children.retain(|child| !matches_selector(child, selector));
    for child in children.iter_mut() {
        remove_matching(child, selector);
    }
}

// --- html.Render -----------------------------------------------------------

fn escape_html(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\'' => out.push_str("&#39;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&#34;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
}

fn render_html(node: &Node, out: &mut String, parent: Option<&str>) {
    match node {
        Node::Document(children) => {
            for child in children {
                render_html(child, out, None);
            }
        }
        Node::Text(text) => {
            if parent.is_some_and(|p| {
                matches!(
                    p,
                    "iframe"
                        | "noembed"
                        | "noframes"
                        | "noscript"
                        | "plaintext"
                        | "script"
                        | "style"
                        | "xmp"
                )
            }) {
                out.push_str(text);
            } else {
                escape_html(text, out);
            }
        }
        Node::Comment(text) => {
            out.push_str("<!--");
            out.push_str(text);
            out.push_str("-->");
        }
        Node::Doctype(name) => {
            out.push_str("<!DOCTYPE ");
            out.push_str(name);
            out.push('>');
        }
        Node::Element {
            tag,
            attrs,
            children,
        } => {
            out.push('<');
            out.push_str(tag);
            for (key, value) in attrs {
                out.push(' ');
                out.push_str(key);
                out.push_str("=\"");
                escape_html(value, out);
                out.push('"');
            }
            let void = matches!(
                tag.as_str(),
                "area"
                    | "base"
                    | "br"
                    | "col"
                    | "embed"
                    | "hr"
                    | "img"
                    | "input"
                    | "keygen"
                    | "link"
                    | "meta"
                    | "param"
                    | "source"
                    | "track"
                    | "wbr"
            );
            if void {
                out.push('>');
                return;
            }
            out.push('>');
            if matches!(tag.as_str(), "pre" | "listing" | "textarea")
                && let Some(Node::Text(text)) = children.first()
                && text.starts_with('\n')
            {
                out.push('\n');
            }
            for child in children {
                render_html(child, out, Some(tag));
            }
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
        }
    }
}

// --- truncation / frontmatter ---------------------------------------------

fn truncate(text: &str, max: usize) -> (String, bool) {
    let total = text.chars().count();
    if total <= max {
        return (text.to_owned(), false);
    }
    let head_n = max / 2;
    let tail_n = max - max / 2;
    let head: String = text.chars().take(head_n).collect();
    let tail: String = if total <= tail_n {
        text.to_owned()
    } else {
        text.chars().skip(total - tail_n).collect()
    };
    let omitted = total - head.chars().count() - tail.chars().count();
    (
        format!("{head}\n\n… [truncated: {omitted} runes omitted] …\n\n{tail}"),
        true,
    )
}

struct Document {
    url: String,
    title: String,
    lang: String,
    fetched_at: String,
    tokens_est: usize,
    schema_type: String,
    markdown: String,
    raw: String,
    outline: Vec<Heading>,
    truncated: bool,
    char_count: usize,
    boundaries: Option<Boundary>,
}

impl Document {
    fn frontmatter(&self) -> String {
        let mut out = String::from("---\n");
        let line = |out: &mut String, key: &str, value: &str| {
            out.push_str(key);
            out.push_str(": ");
            out.push_str(&yaml_str::inline(value, false));
            out.push('\n');
        };
        if !self.title.is_empty() {
            line(&mut out, "title", &self.title);
        }
        line(&mut out, "url", &self.url);
        line(&mut out, "fetched_at", &self.fetched_at);
        if !self.lang.is_empty() {
            line(&mut out, "lang", &self.lang);
        }
        out.push_str(&format!("tokens_est: {}\n", self.tokens_est));
        if !self.schema_type.is_empty() {
            line(&mut out, "schema_type", &self.schema_type);
        }
        out.push_str("---\n\n");
        out
    }

    fn value(&self) -> Value {
        let boundary = self.boundaries.as_ref().map(|b| {
            go_struct(vec![
                ("nonce", "nonce", json!(b.nonce)),
                ("origin", "origin", json!(b.origin)),
                ("start", "start", json!(b.start)),
                ("end", "end", json!(b.end)),
            ])
        });
        let outline: Vec<Value> = self
            .outline
            .iter()
            .map(|h| {
                go_struct(vec![
                    ("level", "level", json!(h.level)),
                    ("text", "text", json!(h.text)),
                ])
            })
            .collect();
        let omit = |keep: bool, name: &'static str| if keep { name } else { "" };
        go_struct(vec![
            ("url", "url", json!(self.url)),
            ("", "finalurl", json!("")),
            (
                omit(!self.title.is_empty(), "title"),
                "title",
                json!(self.title),
            ),
            (
                omit(!self.lang.is_empty(), "lang"),
                "lang",
                json!(self.lang),
            ),
            ("fetched_at", "fetchedat", json!(self.fetched_at)),
            ("tokens_est", "tokensest", json!(self.tokens_est)),
            (
                omit(!self.schema_type.is_empty(), "schema_type"),
                "schematype",
                json!(self.schema_type),
            ),
            (
                omit(!self.markdown.is_empty(), "markdown"),
                "markdown",
                json!(self.markdown),
            ),
            (omit(!self.raw.is_empty(), "raw"), "raw", json!(self.raw)),
            (
                omit(!outline.is_empty(), "outline"),
                "outline",
                Value::Array(outline),
            ),
            ("truncated", "truncated", json!(self.truncated)),
            ("char_count", "charcount", json!(self.char_count)),
            (
                "meta",
                "meta",
                go_struct(vec![
                    ("status_code", "statuscode", json!(200)),
                    ("truncated", "truncated", json!(self.truncated)),
                ]),
            ),
            (
                omit(boundary.is_some(), "content_boundaries"),
                "contentboundaries",
                boundary.unwrap_or(Value::Null),
            ),
        ])
    }
}

fn render(
    html: &str,
    title: &str,
    url: &str,
    selector: &str,
    filter: &str,
    outline: bool,
    raw: bool,
) -> Result<Document, CliError> {
    let mut root = parse(html);
    let mut source = html.to_owned();
    if !selector.is_empty() || !filter.is_empty() {
        if !selector.is_empty() {
            root = find_subtree(&root, selector).ok_or_else(|| {
                CliError::plain(format!(
                    "selector {} did not match an element",
                    symbrowse_core::go_quote::quote(selector)
                ))
            })?;
        } else {
            remove_matching(&mut root, filter);
        }
        let mut rendered = String::new();
        render_html(&root, &mut rendered, None);
        source = rendered;
    }
    let char_count = source.chars().count();
    let now = OffsetDateTime::now_utc();
    let mut doc = Document {
        url: url.to_owned(),
        title: title.trim().to_owned(),
        lang: lang_of(&root),
        fetched_at: format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            now.year(),
            u8::from(now.month()),
            now.day(),
            now.hour(),
            now.minute(),
            now.second()
        ),
        tokens_est: char_count / 4,
        schema_type: schema_type(&root),
        markdown: String::new(),
        raw: String::new(),
        outline: Vec::new(),
        truncated: false,
        char_count,
        boundaries: None,
    };
    if raw {
        let (text, truncated) = truncate(&source, DEFAULT_MAX_CHARS);
        doc.raw = text;
        doc.truncated = truncated;
    } else if outline {
        collect_headings(&root, &mut doc.outline);
    } else {
        let (text, truncated) = truncate(&markdown(&root), DEFAULT_MAX_CHARS);
        doc.markdown = text;
        doc.truncated = truncated;
    }
    Ok(doc)
}

fn write_human(ctx: &mut Ctx<'_>, doc: &Document) {
    let mut body = doc.frontmatter();
    if !doc.outline.is_empty() {
        for heading in &doc.outline {
            body.push_str(&"#".repeat(heading.level));
            body.push(' ');
            body.push_str(&heading.text);
            body.push('\n');
        }
    } else if !doc.raw.is_empty() {
        body.push_str(&doc.raw);
    } else {
        body.push_str(&doc.markdown);
    }
    let mut output = body.trim_end_matches('\n').to_owned();
    if let Some(boundary) = &doc.boundaries {
        if let Some(start) = output.find("\n---\n\n") {
            let head = output[..start + 5].to_owned();
            let rest = output[start + 5..].to_owned();
            output = head + &boundary.wrap_text(&rest);
        } else {
            output = boundary.wrap_text(&output);
        }
    }
    ctx.println(&output);
}

pub fn read(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let url = args.first().cloned().unwrap_or_default();
    let engine_hint = ctx.flags.bool("engine-hint");
    let session = ctx.flags.string("session", "default");
    let max_tokens = if ctx.flags.changed("max-tokens") {
        let value = ctx.flags.int("max-tokens", 0);
        (value > 0).then_some(value)
    } else {
        None
    };
    let response = daemon_request(
        &session,
        "read",
        Some(json!({"url": url, "engine_hint": engine_hint})),
        max_tokens,
    )?;
    if !response.success && response.error.is_none() {
        return Err(CliError::plain("read request failed"));
    }
    if !response.success {
        return Err(response_error(&response));
    }
    let data = response.data.as_ref().map(to_go_any).unwrap_or(Value::Null);
    let field = |key: &str| {
        data.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let js_required = data.get("js_required").and_then(Value::as_bool);
    let js_reason = field("js_required_reason");
    let mut doc = render(
        &field("html"),
        &field("title"),
        &field("url"),
        &ctx.flags.string("selector", ""),
        &ctx.flags.string("filter", ""),
        ctx.flags.bool("outline"),
        ctx.flags.bool("raw"),
    )
    .map_err(|error| {
        let mut wrapped = CliError::wrapf(NO_INPUT, ErrorCode::InvalidArgs, error.message.clone());
        wrapped.cause = Some(error.message);
        wrapped
    })?;
    let page_url = field("url");
    if ctx.flags.bool("content-boundaries") && !page_url.is_empty() {
        doc.boundaries =
            Some(Boundary::new(page_url).map_err(|error| CliError::plain(error.to_string()))?);
    }
    if ctx.structured() {
        let value = match js_required {
            Some(required) => {
                // The hint travels as a sibling of the document: the document
                // is round-tripped through JSON into a map (sorted keys).
                let text = symbrowse_core::go_value::marshal_plain(&doc.value());
                let mut map: Map<String, Value> = match serde_json::from_str::<Value>(&text) {
                    Ok(Value::Object(map)) => map,
                    _ => Map::new(),
                };
                map.insert("js_required".into(), Value::Bool(required));
                if !js_reason.is_empty() {
                    map.insert(
                        "js_required_reason".into(),
                        Value::String(js_reason.clone()),
                    );
                }
                to_go_any(&Value::Object(map))
            }
            None => doc.value(),
        };
        return ctx.write_envelope(&Envelope::ok(value, Vec::new()));
    }
    write_human(ctx, &doc);
    if let Some(required) = js_required {
        let mut line = format!("js_required: {required}");
        if !js_reason.is_empty() {
            line.push_str(" — ");
            line.push_str(&js_reason);
        }
        ctx.println(&line);
    }
    Ok(())
}
