//! Google Calendar importer (ported for #761 from
//! `importer/calendar/calendar.go` at v0.12.0).
//!
//! The OAuth token comes from a token file, `GOOGLE_CALENDAR_TOKEN` (a file
//! path, as in Go) or a `symvault get google/calendar-token --print` lookup
//! through the injected [`Runner`]. API calls use an injected [`HttpGet`]; the
//! default [`PlainHttp`] has no TLS, so reaching `googleapis.com` needs a
//! caller-provided TLS client (see `http.rs`). Descriptions are cut on a
//! character boundary.

use super::http::{HttpGet, PlainHttp};
use super::runner::{Runner, SystemRunner, truncate_bytes};
use super::{ImportResult, ImportedFact, PrivacyLevel, SessionImporter, SessionRef};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Imports events of one calendar.
pub struct Calendar<'a> {
    calendar_id: String,
    include_desc: bool,
    token_path: String,
    /// `GOOGLE_CALENDAR_TOKEN`, read once by [`Calendar::new`].
    env_token_path: String,
    include_days: i64,
    api_base: String,
    http: Box<dyn HttpGet + 'a>,
    vault: Box<dyn Runner + 'a>,
    now: &'a dyn Fn() -> DateTime<Utc>,
}

#[derive(Deserialize, Default)]
struct Token {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    token_type: String,
}

#[derive(Deserialize, Default)]
struct When {
    #[serde(default, rename = "dateTime")]
    date_time: String,
    #[serde(default)]
    date: String,
}

#[derive(Deserialize, Default)]
struct Attendee {
    #[serde(default)]
    email: String,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize, Default)]
struct Organizer {
    #[serde(default)]
    email: String,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize, Default)]
struct Event {
    #[serde(default)]
    id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    start: When,
    #[serde(default)]
    end: When,
    #[serde(default)]
    attendees: Option<Vec<Attendee>>,
    #[serde(default)]
    organizer: Organizer,
    #[serde(default)]
    status: String,
    #[serde(default, rename = "recurringEventId")]
    recurring_id: String,
}

#[derive(Deserialize, Default)]
struct Listing {
    #[serde(default)]
    items: Option<Vec<Event>>,
    #[serde(default, rename = "nextPageToken")]
    next_page_token: String,
}

fn parse_when(when: &When) -> Option<DateTime<Utc>> {
    if !when.date_time.is_empty() {
        return DateTime::parse_from_rfc3339(&when.date_time)
            .ok()
            .map(|t| t.with_timezone(&Utc));
    }
    NaiveDate::parse_from_str(&when.date, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|d| d.and_utc())
}

impl<'a> Calendar<'a> {
    /// Empty `calendar_id` means `primary`; `include_days == 0` means 7.
    #[must_use]
    pub fn new(calendar_id: &str, token_path: &str, include_desc: bool, include_days: i64) -> Self {
        Self::with_io(
            calendar_id,
            token_path,
            include_desc,
            include_days,
            PlainHttp::default(),
            SystemRunner,
            &Utc::now,
        )
    }

    /// As [`Calendar::new`] with injected HTTP client, vault runner and clock.
    #[must_use]
    pub fn with_io(
        calendar_id: &str,
        token_path: &str,
        include_desc: bool,
        include_days: i64,
        http: impl HttpGet + 'a,
        vault: impl Runner + 'a,
        now: &'a dyn Fn() -> DateTime<Utc>,
    ) -> Self {
        Self {
            calendar_id: if calendar_id.is_empty() {
                "primary".into()
            } else {
                calendar_id.to_owned()
            },
            include_desc,
            token_path: token_path.to_owned(),
            env_token_path: String::new(),
            include_days: if include_days <= 0 { 7 } else { include_days },
            api_base: "https://www.googleapis.com/calendar/v3".into(),
            http: Box::new(http),
            vault: Box::new(vault),
            now,
        }
    }

    /// Redirects API calls (a local fake server in tests).
    #[must_use]
    pub fn with_api_base(mut self, base: &str) -> Self {
        base.clone_into(&mut self.api_base);
        self
    }

    fn load_token(&self) -> Result<Token, String> {
        let mut path = self.token_path.clone();
        if path.is_empty() {
            path.clone_from(&self.env_token_path);
        }
        let data = if path.is_empty() {
            let args = ["get", "google/calendar-token", "--print"].map(String::from);
            let out = self.vault.run("symvault", &args).map_err(|_| {
                "no token found: set GOOGLE_CALENDAR_TOKEN env or provide --token-path".to_owned()
            })?;
            String::from_utf8_lossy(&out).trim().as_bytes().to_vec()
        } else {
            std::fs::read(&path).map_err(|e| format!("failed to read token file: {e}"))?
        };
        super::parse_object(&data).map_err(|e| format!("failed to parse token JSON: {e}"))
    }

    fn api<T: serde::de::DeserializeOwned + Default>(
        &self,
        url: &str,
        token: &Token,
    ) -> Result<T, String> {
        let kind = if token.token_type.is_empty() {
            "Bearer"
        } else {
            token.token_type.as_str()
        };
        let auth = format!("{kind} {}", token.access_token);
        let response = self
            .http
            .get(url, &[("Authorization", &auth)])
            .map_err(|e| format!("API request failed: {e}"))?;
        if response.status != 200 {
            return Err(format!("API returned status {}", response.status));
        }
        super::parse_object(&response.body)
    }
}

impl SessionImporter for Calendar<'_> {
    fn name(&self) -> &'static str {
        "calendar"
    }
    fn category(&self) -> Option<&str> {
        Some("calendar")
    }
    fn privacy_level(&self) -> Option<PrivacyLevel> {
        Some(PrivacyLevel::Confidential)
    }
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        let token = self
            .load_token()
            .map_err(|e| format!("failed to load OAuth token: {e}"))?;
        let rfc = |t: DateTime<Utc>| t.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let first = format!(
            "{}/calendars/{}/events?timeMin={}&timeMax={}&singleEvents=true&orderBy=startTime&maxResults=250",
            self.api_base,
            self.calendar_id,
            rfc(since),
            rfc((self.now)() + Duration::days(self.include_days))
        );
        let mut events = Vec::new();
        let mut url = first.clone();
        loop {
            let page: Listing = self
                .api(&url, &token)
                .map_err(|e| format!("failed to fetch events: {e}"))?;
            events.extend(page.items.unwrap_or_default());
            if page.next_page_token.is_empty() {
                break;
            }
            url = format!("{url}&pageToken={}", page.next_page_token);
        }
        let mut sessions = Vec::new();
        for event in events.into_iter().filter(|e| e.status != "cancelled") {
            let Some(start) = parse_when(&event.start) else {
                continue;
            };
            let mut metadata = BTreeMap::from([
                ("title".to_owned(), event.summary.clone()),
                ("location".to_owned(), event.location.clone()),
            ]);
            if !event.organizer.name.is_empty() {
                metadata.insert("organizer".into(), event.organizer.name.clone());
            } else if !event.organizer.email.is_empty() {
                metadata.insert("organizer".into(), event.organizer.email.clone());
            }
            if let Some(attendees) = event.attendees.as_ref().filter(|a| !a.is_empty()) {
                let names: Vec<&str> = attendees
                    .iter()
                    .map(|a| {
                        if a.name.is_empty() {
                            a.email.as_str()
                        } else {
                            a.name.as_str()
                        }
                    })
                    .collect();
                metadata.insert("attendees".into(), names.join(", "));
            }
            if !event.recurring_id.is_empty() {
                metadata.insert("recurring".into(), "true".into());
            }
            sessions.push(SessionRef {
                tool: "calendar".into(),
                path: format!("google-calendar://{}/{}", self.calendar_id, event.id),
                session_id: event.id,
                modified_at: start,
                metadata,
            });
        }
        Ok(sessions)
    }
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let token = self
            .load_token()
            .map_err(|e| format!("failed to load OAuth token: {e}"))?;
        let url = format!(
            "{}/calendars/{}/events/{}",
            self.api_base, self.calendar_id, session.session_id
        );
        let event: Event = self
            .api(&url, &token)
            .map_err(|e| format!("failed to fetch event: {e}"))?;
        let (start, end) = (parse_when(&event.start), parse_when(&event.end));
        let mut content = format!("Calendar event: {}", event.summary);
        if let Some(s) = start {
            content.push_str(&format!(" on {}", s.format("%Y-%m-%d %H:%M")));
        }
        if !event.location.is_empty() {
            content.push_str(&format!(" at {}", event.location));
        }
        if self.include_desc && !event.description.is_empty() {
            content.push_str("\n\n");
            content.push_str(truncate_bytes(&event.description, 2000));
            if event.description.len() > 2000 {
                content.push_str("...");
            }
        }
        let mut metadata = BTreeMap::from([
            ("event_id".to_owned(), event.id.clone()),
            ("title".to_owned(), event.summary.clone()),
            ("source".to_owned(), "calendar".to_owned()),
            ("calendar".to_owned(), self.calendar_id.clone()),
        ]);
        let rfc = |t: DateTime<Utc>| t.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        if let Some(s) = start {
            metadata.insert("start".into(), rfc(s));
        }
        if let Some(e) = end {
            metadata.insert("end".into(), rfc(e));
        }
        if !event.location.is_empty() {
            metadata.insert("location".into(), event.location.clone());
        }
        if !event.organizer.email.is_empty() {
            metadata.insert("organizer".into(), event.organizer.email.clone());
        }
        if let Some(attendees) = event.attendees.as_ref().filter(|a| !a.is_empty()) {
            let emails: Vec<&str> = attendees.iter().map(|a| a.email.as_str()).collect();
            metadata.insert(
                "attendees".into(),
                serde_json::to_string(&emails).map_err(|e| e.to_string())?,
            );
        }
        if !event.recurring_id.is_empty() {
            metadata.insert("recurring".into(), "true".into());
            metadata.insert("recurring_id".into(), event.recurring_id.clone());
        }
        Ok(vec![ImportedFact {
            content,
            source: "calendar".into(),
            session_id: session.session_id.clone(),
            timestamp: start.unwrap_or_else(crate::sync_state::zero_time),
            metadata,
            evidence: Vec::new(),
        }])
    }
}

#[cfg(test)]
#[path = "calendar_tests.rs"]
mod tests;
