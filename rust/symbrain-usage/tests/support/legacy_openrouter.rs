//! Frozen key-only evidence applies to the explicitly unavailable credits endpoint.
use super::usage;
use std::collections::BTreeMap;

pub fn transport(responses: BTreeMap<String, usage::Response>) -> usage::FixtureTransport {
    usage::FixtureTransport::with_sequences(
        responses
            .into_iter()
            .map(|(id, response)| {
                let mut replies = Vec::new();
                if id == "openrouter" {
                    replies.push(Ok(usage::Response {
                        status: 404,
                        body: b"{}".to_vec(),
                        headers: BTreeMap::new(),
                    }));
                }
                replies.push(Ok(response));
                (id, replies)
            })
            .collect(),
    )
}

/// Assert the new prelude independently, then compare the unchanged historical walk.
pub fn requests(transport: &usage::FixtureTransport) -> Vec<usage::Request> {
    let mut requests = transport.requests();
    if let Some(index) = requests.iter().position(|r| r.provider_id == "openrouter") {
        let prelude = requests.remove(index);
        let legacy = requests
            .iter()
            .find(|r| r.provider_id == "openrouter")
            .expect("legacy request after 404");
        assert_eq!(
            prelude.url.strip_suffix("/credits"),
            legacy.url.strip_suffix("/auth/key")
        );
        assert!(prelude.url.ends_with("/credits"));
        assert!(legacy.url.ends_with("/auth/key"));
        assert_eq!(prelude.method, "GET");
        assert_eq!(prelude.headers, legacy.headers);
        assert_eq!(prelude.body, legacy.body);
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.provider_id == "openrouter")
                .count(),
            1
        );
    }
    requests
}
