//! Go1.26.7 original-header copying and per-send URL Basic ownership.
//! Source-derived from net/http/client.go; SDK BSD notice is retained at
//! scripts/memory-sync-oracle/reference/GO-SDK-LICENSE. Runtime proof is pending.

use super::super::remote::Url;

pub(super) struct RedirectAuthorization {
    original: Option<Vec<u8>>,
    stripped: bool,
}

impl RedirectAuthorization {
    pub(super) fn new(token: &[u8]) -> Self {
        // The frozen client explicitly adds only a nonempty Bearer token.
        // Go's original-header copier is created before send clones Header
        // to generate URL Basic, so Basic never belongs to this copy state.
        let original = (!token.is_empty()).then(|| {
            let mut value = b"Bearer ".to_vec();
            value.extend_from_slice(token);
            value
        });
        Self {
            original,
            stripped: false,
        }
    }

    pub(super) fn for_request(&self, current: &Url) -> Option<Vec<u8>> {
        let copied = (!self.stripped).then(|| self.original.clone()).flatten();
        // Each send may synthesize Basic from its current resolved URL when
        // no explicit Authorization remains, including after sticky stripping.
        copied.or_else(|| current.basic())
    }

    pub(super) fn redirect(&mut self, initial: &Url, destination: &Url) {
        if !self.stripped
            && initial.host != destination.host
            && !should_copy(initial.hostname(), destination.hostname())
        {
            self.stripped = true;
        }
    }
}

fn should_copy(initial: &str, destination: &str) -> bool {
    // Go idnaASCII returns already-ASCII spelling unchanged. Unsupported
    // non-ASCII/zone URLs are still refused by Url::uri before an actual send.
    destination == initial
        || (!destination.contains([':', '%'])
            && destination
                .strip_suffix(initial)
                .is_some_and(|prefix| prefix.ends_with('.')))
}

#[cfg(test)]
#[path = "headers_tests.rs"]
mod tests;
