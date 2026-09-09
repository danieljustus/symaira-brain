use crate::providers::trusted_https_url;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Hard limits applied before a provider response enters a parser.
pub const MAX_RESPONSE_BYTES: usize = 1_048_576;
pub const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024;

/// Cooperative cancellation shared by a provider worker and its transport.
#[derive(Debug, Clone, Default)]
pub struct Cancellation {
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    deadline: Arc<Mutex<Option<Instant>>>,
}

impl Cancellation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            deadline: Arc::new(Mutex::new(Some(Instant::now() + timeout))),
        }
    }
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
        if let Ok(mut deadline) = self.deadline.lock() {
            let shortened = Instant::now() + Duration::from_millis(100);
            if deadline.is_none_or(|current| current > shortened) {
                *deadline = Some(shortened);
            }
        }
    }
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Acquire)
    }
    #[must_use]
    pub fn remaining(&self) -> Option<Duration> {
        self.deadline.lock().ok().and_then(|deadline| {
            deadline.map(|value| value.saturating_duration_since(Instant::now()))
        })
    }
}

/// Provider request passed to an injectable transport.
#[derive(Debug, Clone)]
pub struct Request {
    pub provider_id: String,
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Vec<u8>>,
}

/// Minimal response needed by provider parsers.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: BTreeMap<String, String>,
}

/// Side-effect boundary for provider reads. Implementations should check the
/// cancellation flag between I/O chunks; the default keeps old test doubles
/// source-compatible while production transports are fully cooperative.
pub trait Transport: Send + Sync {
    /// # Errors
    /// Returns the transport error when the request cannot be completed.
    fn request(&self, request: Request) -> Result<Response, String>;
    /// # Errors
    /// Returns the transport error or cancellation when the request cannot be completed.
    fn request_with_cancel(
        &self,
        request: Request,
        cancel: &Cancellation,
    ) -> Result<Response, String> {
        if cancel.is_cancelled() {
            return Err("request cancelled".to_string());
        }
        self.request(request)
    }
}

/// Deterministic provider-keyed transport for tests and oracle fixtures.
#[derive(Debug, Clone, Default)]
pub struct FixtureTransport {
    responses: Arc<BTreeMap<String, Result<Response, String>>>,
    sequences: Arc<BTreeMap<String, Vec<Result<Response, String>>>>,
    requests: Arc<Mutex<Vec<Request>>>,
}

impl FixtureTransport {
    #[must_use]
    pub fn new(responses: BTreeMap<String, Response>) -> Self {
        Self::with_errors(
            responses
                .into_iter()
                .map(|(id, response)| (id, Ok(response)))
                .collect(),
        )
    }

    #[must_use]
    pub fn with_errors(responses: BTreeMap<String, Result<Response, String>>) -> Self {
        Self {
            responses: Arc::new(responses),
            sequences: Arc::new(BTreeMap::new()),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[must_use]
    pub fn with_sequences(sequences: BTreeMap<String, Vec<Result<Response, String>>>) -> Self {
        Self {
            responses: Arc::new(BTreeMap::new()),
            sequences: Arc::new(sequences),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[must_use]
    pub fn requests(&self) -> Vec<Request> {
        self.requests
            .lock()
            .map_or_else(|_| Vec::new(), |r| r.clone())
    }
}

impl Transport for FixtureTransport {
    fn request(&self, request: Request) -> Result<Response, String> {
        if let Ok(mut requests) = self.requests.lock() {
            requests.push(request.clone());
        }
        if let Some(sequence) = self.sequences.get(&request.provider_id) {
            let index = self
                .requests
                .lock()
                .map_or(1, |requests| {
                    requests
                        .iter()
                        .filter(|item| item.provider_id == request.provider_id)
                        .count()
                })
                .saturating_sub(1);
            return sequence.get(index).cloned().unwrap_or_else(|| {
                sequence
                    .last()
                    .cloned()
                    .unwrap_or_else(|| Err("fixture sequence is empty".to_string()))
            });
        }
        self.responses
            .get(&request.provider_id)
            .cloned()
            .unwrap_or_else(|| {
                Err(format!(
                    "fixture missing provider {:?}",
                    request.provider_id
                ))
            })
    }
}

/// Production HTTPS transport. It never logs headers or bodies, refuses
/// redirects, and bounds response bodies before returning them.
#[derive(Debug)]
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(8)))
            .max_redirects(0)
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl UreqTransport {
    fn execute(request: Request, agent: &ureq::Agent) -> Result<Response, String> {
        if request
            .body
            .as_ref()
            .is_some_and(|b| b.len() > MAX_REQUEST_BODY_BYTES)
        {
            return Err("request body exceeds limit".to_string());
        }
        if !trusted_https_url(&request.url, request.provider_id == "antigravity") {
            return Err("provider URL must use HTTPS".to_string());
        }
        let mut builder = ureq::http::Request::builder()
            .method(request.method.as_str())
            .uri(request.url.as_str());
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let body = request.body.unwrap_or_default();
        let http_request = builder
            .body(body)
            .map_err(|e| format!("build request: {e}"))?;
        match agent.run(http_request) {
            Ok(mut response) => {
                let status = response.status().as_u16();
                if (300..400).contains(&status) {
                    return Err("provider redirect refused".to_string());
                }
                let body = response
                    .body_mut()
                    .with_config()
                    .limit((MAX_RESPONSE_BYTES + 1) as u64)
                    .read_to_vec()
                    .map_err(|e| format!("read response: {e}"))?;
                if body.len() > MAX_RESPONSE_BYTES {
                    return Err("provider response exceeds 1048576 bytes".to_string());
                }
                let mut headers = BTreeMap::new();
                for (name, value) in response.headers() {
                    if let Ok(value) = value.to_str() {
                        headers.insert(name.as_str().to_ascii_lowercase(), value.to_string());
                    }
                }
                Ok(Response {
                    status,
                    body,
                    headers,
                })
            }
            Err(error) => Err(error.to_string()),
        }
    }
}

impl Transport for UreqTransport {
    fn request(&self, request: Request) -> Result<Response, String> {
        Self::execute(request, &self.agent)
    }

    fn request_with_cancel(
        &self,
        request: Request,
        cancel: &Cancellation,
    ) -> Result<Response, String> {
        if cancel.is_cancelled() {
            return Err("request cancelled".to_string());
        }
        let timeout = cancel.remaining().unwrap_or(Duration::from_secs(8));
        if timeout.is_zero() {
            return Err("request cancelled".to_string());
        }
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .max_redirects(0)
            .build();
        let agent = ureq::Agent::new_with_config(config);
        Self::execute(request, &agent)
    }
}
