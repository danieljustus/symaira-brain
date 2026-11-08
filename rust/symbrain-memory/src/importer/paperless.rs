//! Paperless-ngx importer (ported for #761 from
//! `importer/paperless/paperless.go` at v0.12.0): documents of a Paperless
//! server. The server URL and API token are explicit arguments (the Go
//! `PAPERLESS_URL` / `PAPERLESS_TOKEN` environment fallback is applied by the
//! caller through [`Paperless::from_env`]); HTTP goes through an injected
//! [`HttpGet`]. Content previews are cut on a character boundary.

use super::http::{HttpGet, PlainHttp};
use super::runner::truncate_bytes;
use super::{ImportResult, ImportedFact, PrivacyLevel, SessionImporter, SessionRef};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Imports documents from Paperless-ngx.
pub struct Paperless<'a> {
    base_url: String,
    token: String,
    tag: String,
    correspondent: String,
    max_content: usize,
    http: Box<dyn HttpGet + 'a>,
}

#[derive(Deserialize, Default)]
struct Named {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize, Default)]
struct Doc {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    created_date: String,
    #[serde(default)]
    added: String,
    #[serde(default)]
    correspondent: Option<Named>,
    #[serde(default)]
    tags: Option<Vec<Named>>,
    #[serde(default)]
    file_type: String,
    #[serde(default)]
    page_count: i64,
}

#[derive(Deserialize, Default)]
struct Page {
    #[serde(default)]
    results: Option<Vec<Doc>>,
    #[serde(default)]
    next: Option<String>,
}

fn day(raw: &str) -> Option<DateTime<Utc>> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()
        .map(|d| d.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc())
}

impl<'a> Paperless<'a> {
    /// `max_content == 0` means 1000 bytes. Uses the plain-HTTP client.
    #[must_use]
    pub fn new(
        base_url: &str,
        token: &str,
        tag: &str,
        correspondent: &str,
        max_content: usize,
    ) -> Self {
        Self::with_http(
            base_url,
            token,
            tag,
            correspondent,
            max_content,
            PlainHttp::default(),
        )
    }

    /// As [`Paperless::new`] with an injected HTTP client.
    #[must_use]
    pub fn with_http(
        base_url: &str,
        token: &str,
        tag: &str,
        correspondent: &str,
        max_content: usize,
        http: impl HttpGet + 'a,
    ) -> Self {
        Self {
            base_url: base_url.to_owned(),
            token: token.to_owned(),
            tag: tag.to_owned(),
            correspondent: correspondent.to_owned(),
            max_content: if max_content == 0 { 1000 } else { max_content },
            http: Box::new(http),
        }
    }

    /// Fills an empty URL/token from `PAPERLESS_URL` / `PAPERLESS_TOKEN`.
    #[must_use]
    pub fn from_env(mut self) -> Self {
        for (slot, var) in [
            (&mut self.base_url, "PAPERLESS_URL"),
            (&mut self.token, "PAPERLESS_TOKEN"),
        ] {
            if slot.is_empty() {
                *slot = std::env::var(var).unwrap_or_default();
            }
        }
        self
    }

    fn api<T: serde::de::DeserializeOwned + Default>(&self, url: &str) -> Result<T, String> {
        let token = format!("Token {}", self.token);
        let response = self
            .http
            .get(
                url,
                &[("Authorization", &token), ("Accept", "application/json")],
            )
            .map_err(|e| format!("API request failed: {e}"))?;
        if response.status != 200 {
            return Err(format!(
                "API returned status {}: {}",
                response.status,
                String::from_utf8_lossy(&response.body)
            ));
        }
        super::parse_object(&response.body)
    }
}

fn tags_json(doc: &Doc) -> Option<String> {
    let tags = doc.tags.as_ref().filter(|t| !t.is_empty())?;
    let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
    serde_json::to_string(&names).ok()
}

impl SessionImporter for Paperless<'_> {
    fn name(&self) -> &'static str {
        "paperless"
    }
    fn category(&self) -> Option<&str> {
        Some("documents")
    }
    fn privacy_level(&self) -> Option<PrivacyLevel> {
        Some(PrivacyLevel::Confidential)
    }
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        if self.base_url.is_empty() || self.token.is_empty() {
            return Err("PAPERLESS_URL and PAPERLESS_TOKEN must be set".into());
        }
        let mut url = format!(
            "{}/api/documents/?format=json&ordering=-created_date",
            self.base_url
        );
        if since > crate::sync_state::zero_time() {
            url.push_str(&format!("&created_date__gte={}", since.format("%Y-%m-%d")));
        }
        if !self.tag.is_empty() {
            url.push_str(&format!("&tags__name={}", self.tag));
        }
        if !self.correspondent.is_empty() {
            url.push_str(&format!("&correspondent__name={}", self.correspondent));
        }
        let mut documents = Vec::new();
        let mut next = Some(url);
        while let Some(url) = next.filter(|u| !u.is_empty()) {
            let page: Page = self.api(&url)?;
            documents.extend(page.results.unwrap_or_default());
            next = page.next;
        }
        Ok(documents
            .into_iter()
            .map(|doc| {
                let when = day(&doc.created_date)
                    .or_else(|| {
                        DateTime::parse_from_rfc3339(&doc.added)
                            .ok()
                            .map(|t| t.with_timezone(&Utc))
                    })
                    .unwrap_or_else(crate::sync_state::zero_time);
                let mut metadata = BTreeMap::from([
                    ("title".to_owned(), doc.title.clone()),
                    ("file_type".to_owned(), doc.file_type.clone()),
                ]);
                if let Some(c) = doc.correspondent.as_ref().filter(|c| !c.name.is_empty()) {
                    metadata.insert("correspondent".into(), c.name.clone());
                }
                if let Some(tags) = tags_json(&doc) {
                    metadata.insert("tags".into(), tags);
                }
                SessionRef {
                    tool: "paperless".into(),
                    session_id: doc.id.to_string(),
                    path: format!("{}/documents/{}/", self.base_url, doc.id),
                    modified_at: when,
                    metadata,
                }
            })
            .collect())
    }
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let doc: Doc = self.api(&format!(
            "{}/api/documents/{}/?format=json",
            self.base_url, session.session_id
        ))?;
        let content = if doc.content.len() > self.max_content {
            format!("{}...", truncate_bytes(&doc.content, self.max_content))
        } else {
            doc.content.clone()
        };
        let correspondent = doc
            .correspondent
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let mut text = format!("Document: {}", doc.title);
        if !correspondent.is_empty() {
            text.push_str(&format!(" from {correspondent}"));
        }
        if !content.is_empty() {
            text.push_str("\n\n");
            text.push_str(&content);
        }
        let mut metadata = BTreeMap::from([
            ("document_id".to_owned(), doc.id.to_string()),
            ("title".to_owned(), doc.title.clone()),
            ("source".to_owned(), "paperless".to_owned()),
            ("file_type".to_owned(), doc.file_type.clone()),
        ]);
        let date = day(&doc.created_date);
        if let Some(d) = date {
            metadata.insert("document_date".into(), d.format("%Y-%m-%d").to_string());
        }
        if !correspondent.is_empty() {
            metadata.insert("correspondent".into(), correspondent);
        }
        if let Some(tags) = tags_json(&doc) {
            metadata.insert("tags".into(), tags);
        }
        if doc.page_count > 0 {
            metadata.insert("page_count".into(), doc.page_count.to_string());
        }
        Ok(vec![ImportedFact {
            content: text,
            source: "paperless".into(),
            session_id: session.session_id.clone(),
            timestamp: date.unwrap_or_else(crate::sync_state::zero_time),
            metadata,
            evidence: Vec::new(),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::http::fake::FakeHttp;
    use chrono::TimeZone;

    const DOC: &str = r#"{"id":12,"title":"Invoice","content":"0123456789ABCDEF","created_date":"2026-03-05","added":"2026-03-06T08:00:00Z","correspondent":{"id":1,"name":"ACME"},"tags":[{"id":1,"name":"tax"},{"id":2,"name":"2026"}],"file_type":"pdf","page_count":3}"#;

    #[test]
    fn discovers_across_pages_with_filters() {
        let page1 = format!(
            r#"{{"count":2,"results":[{DOC}],"next":"http://paper.test/api/documents/?page=2"}}"#
        );
        let page2 = r#"{"count":2,"results":[{"id":13,"title":"Note","created_date":"bad","added":"2026-04-01T09:00:00Z","correspondent":null,"tags":null,"file_type":"txt"}],"next":null}"#.to_owned();
        let http = FakeHttp::new(vec![
            ("page=2", 200, page2),
            ("api/documents/?format", 200, page1),
        ]);
        let importer = Paperless::with_http("http://paper.test", "secret", "tax", "ACME", 0, &http);
        let since = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
        let sessions = importer.discover_sessions(since).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "12");
        assert_eq!(sessions[0].path, "http://paper.test/documents/12/");
        assert_eq!(
            sessions[0].modified_at,
            Utc.with_ymd_and_hms(2026, 3, 5, 0, 0, 0).unwrap()
        );
        assert_eq!(sessions[0].metadata["tags"], "[\"tax\",\"2026\"]");
        assert_eq!(sessions[0].metadata["correspondent"], "ACME");
        assert_eq!(
            sessions[1].modified_at,
            Utc.with_ymd_and_hms(2026, 4, 1, 9, 0, 0).unwrap(),
            "falls back to `added`"
        );
        assert!(!sessions[1].metadata.contains_key("tags"));
        let calls = http.calls.borrow();
        assert_eq!(
            calls[0].0,
            "http://paper.test/api/documents/?format=json&ordering=-created_date&created_date__gte=2026-03-01&tags__name=tax&correspondent__name=ACME"
        );
        assert!(
            calls[0]
                .1
                .contains(&("Authorization".into(), "Token secret".into()))
        );
    }

    #[test]
    fn imports_a_document() {
        let http = FakeHttp::new(vec![("/api/documents/12/", 200, DOC.to_owned())]);
        let importer = Paperless::with_http("http://paper.test", "t", "", "", 10, &http);
        let session = SessionRef {
            session_id: "12".into(),
            ..SessionRef::default()
        };
        let fact = &importer.import_session(&session).unwrap()[0];
        assert_eq!(fact.content, "Document: Invoice from ACME\n\n0123456789...");
        assert_eq!(
            fact.timestamp,
            Utc.with_ymd_and_hms(2026, 3, 5, 0, 0, 0).unwrap()
        );
        assert_eq!(fact.metadata["document_date"], "2026-03-05");
        assert_eq!(fact.metadata["page_count"], "3");
        assert_eq!(fact.metadata["document_id"], "12");
        assert_eq!(
            (importer.category(), importer.privacy_level()),
            (Some("documents"), Some(PrivacyLevel::Confidential))
        );
    }

    #[test]
    fn credentials_and_http_errors() {
        let http = FakeHttp::new(vec![("api/documents", 403, "denied".into())]);
        let no_token = Paperless::with_http("http://p", "", "", "", 0, &http);
        assert_eq!(
            no_token.discover_sessions(Utc::now()).unwrap_err(),
            "PAPERLESS_URL and PAPERLESS_TOKEN must be set"
        );
        let importer = Paperless::with_http("http://p", "t", "", "", 0, &http);
        assert_eq!(
            importer.discover_sessions(Utc::now()).unwrap_err(),
            "API returned status 403: denied"
        );
    }
}
