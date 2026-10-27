//! The pull half of `memory sync`: `GET /api/sync/changes` page by page
//! (`plainPull`), with the shipped wording for every failure.
//!
//! All pages are fetched before anything is written. The requests only depend
//! on the pagination cursors, never on the database, so applying afterwards is
//! equivalent to the shipped interleaving: when page `k` fails, pages `1..k`
//! are applied and the failure is reported, exactly what the shipped client
//! leaves behind. A page that has no proven reading hands the whole invocation
//! to the shipped client before any local write; its `GET`s are repeated there
//! (they are read-only).

use super::sync_decode::{self, Decoded, Page};
use super::sync_http::{self, Fetched, ReadStop};
use chrono::{DateTime, Utc};
use std::time::Instant;
use symbrain_memory::zero_time;

pub(super) const PAGE_LIMIT: usize = 500;
/// More pages than this are left to the shipped client.
const MAX_PAGES: usize = 64;
/// Success replies this native path can accept stay below this size.
const SUCCESS_BODY_LIMIT: usize = 1 << 20;
/// `io.LimitReader(resp.Body, 1<<16)` of the shipped error decoder.
const ERROR_BODY_LIMIT: usize = 1 << 16;

pub(super) struct Source<'a> {
    pub remote: &'a str,
    pub base: &'a str,
    pub token: Option<&'a str>,
}

pub(super) enum Pull {
    /// Every page, the last one without a pagination cursor.
    Pages(Vec<Page>),
    /// The pages before a failing request, and the text after `symbrain memory sync: `.
    Failed { pages: Vec<Page>, message: String },
}

/// `url.QueryEscape`.
fn query_escape(value: &str) -> String {
    use std::fmt::Write as _;
    let mut escaped = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                escaped.push(char::from(byte));
            }
            b' ' => escaped.push('+'),
            other => {
                let _ = write!(escaped, "%{other:02X}");
            }
        }
    }
    escaped
}

/// `Client.Changes`: the pagination cursor wins over `since`; the keys are
/// encoded in sorted order (`cursor`, `limit`, `since`).
fn page_url(base: &str, since: DateTime<Utc>, next: Option<&str>) -> String {
    let url = format!("{base}/api/sync/changes");
    if let Some(next) = next {
        return format!("{url}?cursor={}&limit={PAGE_LIMIT}", query_escape(next));
    }
    let url = format!("{url}?limit={PAGE_LIMIT}");
    if since == zero_time() {
        return url;
    }
    // `url.Values.Encode` escapes the colons of the RFC 3339 form.
    format!("{url}&since={}", since.format("%Y-%m-%dT%H%%3A%M%%3A%SZ"))
}

/// `None` hands the invocation to the shipped client.
pub(super) fn pull_pages(
    source: &Source<'_>,
    since: DateTime<Utc>,
    deadline: Instant,
) -> Option<Pull> {
    let mut pages: Vec<Page> = Vec::new();
    let mut next: Option<String> = None;
    loop {
        if pages.len() >= MAX_PAGES {
            return None;
        }
        let url = page_url(source.base, since, next.as_deref());
        match fetch(source, &url, deadline)? {
            Ok(page) => {
                let more = (!page.next_cursor.is_empty()).then(|| page.next_cursor.clone());
                pages.push(page);
                match more {
                    Some(cursor) => next = Some(cursor),
                    None => return Some(Pull::Pages(pages)),
                }
            }
            Err(message) => return Some(Pull::Failed { pages, message }),
        }
    }
}

/// One request: `Some(Ok(page))`, `Some(Err(message))` for a failure with a
/// proven wording, `None` to hand off.
fn fetch(source: &Source<'_>, url: &str, deadline: Instant) -> Option<Result<Page, String>> {
    let failed = |inner: String| Err(format!("pull from {}: {inner}", source.remote));
    let transport =
        |cause: &str| failed(format!("request {}: Get \"{url}\": {cause}", source.base));
    let body_limit = SUCCESS_BODY_LIMIT.max(ERROR_BODY_LIMIT);
    match sync_http::get(url, source.token, deadline, body_limit) {
        Fetched::Handoff => None,
        Fetched::Transport(cause) => Some(transport(&cause)),
        Fetched::Reply {
            status,
            body,
            stopped,
        } if status > 300 => {
            let body = &body[..body.len().min(ERROR_BODY_LIMIT)];
            // A stalled or cut body is decoded as far as it was read.
            let _ = stopped;
            let (message, code) = sync_decode::api_error(body)?;
            Some(failed(if code.is_empty() {
                format!("remote returned status {status}: {message}")
            } else {
                format!("remote returned {code} ({status}): {message}")
            }))
        }
        Fetched::Reply { body, stopped, .. } => {
            if let Some(stop) = stopped {
                let cause = match stop {
                    ReadStop::Deadline => "context deadline exceeded",
                    ReadStop::UnexpectedEof => "unexpected EOF",
                };
                return Some(failed(format!("decode remote response: {cause}")));
            }
            if body.len() >= SUCCESS_BODY_LIMIT {
                return None;
            }
            match sync_decode::changes(&body) {
                Decoded::Value(page) => Some(Ok(page)),
                Decoded::Failure(message) => {
                    Some(failed(format!("decode remote response: {message}")))
                }
                Decoded::Handoff => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_escape_follows_the_shipped_rules() {
        assert_eq!(query_escape("AZaz09-_.~"), "AZaz09-_.~");
        assert_eq!(query_escape("a b+c/d=e&f:g"), "a+b%2Bc%2Fd%3De%26f%3Ag");
        assert_eq!(query_escape("ä€\u{0}"), "%C3%A4%E2%82%AC%00");
    }

    #[test]
    fn urls_sort_their_keys_and_drop_since_after_the_first_page() {
        let base = "http://127.0.0.1:9";
        assert_eq!(
            page_url(base, zero_time(), None),
            "http://127.0.0.1:9/api/sync/changes?limit=500"
        );
        let since = symbrain_memory::parse_cursor("2026-10-26 01:02:03 +0000 UTC").unwrap();
        assert_eq!(
            page_url(base, since, None),
            "http://127.0.0.1:9/api/sync/changes?limit=500&since=2026-10-26T01%3A02%3A03Z"
        );
        assert_eq!(
            page_url(base, since, Some("c 1")),
            "http://127.0.0.1:9/api/sync/changes?cursor=c+1&limit=500"
        );
    }
}
