use std::{
    collections::HashMap,
    io,
    net::{SocketAddr, ToSocketAddrs},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use reqwest::{
    Client as HttpClient, Method,
    cookie::Jar,
    dns::{Addrs, Name, Resolve, Resolving},
    redirect::{Attempt, Policy},
};
use symbrowse_core::policy::{Allowlist, SsrfGuard, is_private_ip};
use tokio::time::timeout;

use crate::{
    client::{FetchClient, FetchError, Request, Response},
    retry::{is_transient_status, parse_retry_after},
};

const DEFAULT_USER_AGENT: &str = "symfetch/0.1 (+https://github.com/danieljustus/symaira-fetch)";
const MAX_REDIRECTS: usize = 10;

#[path = "honest/headers.rs"]
mod headers;
use headers::request_headers;
#[path = "honest/proxy.rs"]
pub(crate) mod proxy;
use proxy::ProxyConfig;
#[path = "honest/response.rs"]
mod response;
use response::read_response;

type ResolverLookup = dyn Fn(&str) -> Result<Vec<SocketAddr>, String> + Send + Sync;

/// DNS resolver that pins one validated address set for each host for the
/// lifetime of the HTTP client. This keeps policy validation and every
/// redirect hop on the same resolution result instead of permitting a second
/// lookup to rebind a host between the check and the connection.
#[derive(Clone)]
pub struct PinnedResolver {
    lookup: Arc<ResolverLookup>,
    pinned: Arc<Mutex<HashMap<String, Vec<SocketAddr>>>>,
}

impl std::fmt::Debug for PinnedResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinnedResolver")
            .field(
                "pinned_hosts",
                &self.pinned.lock().map(|v| v.len()).unwrap_or(0),
            )
            .finish_non_exhaustive()
    }
}

impl PinnedResolver {
    #[must_use]
    pub fn system() -> Self {
        Self::with_lookup(|host| {
            (host, 0)
                .to_socket_addrs()
                .map(|addresses| addresses.collect())
                .map_err(|error| error.to_string())
        })
    }

    #[must_use]
    pub fn with_lookup<F>(lookup: F) -> Self
    where
        F: Fn(&str) -> Result<Vec<SocketAddr>, String> + Send + Sync + 'static,
    {
        Self {
            lookup: Arc::new(lookup),
            pinned: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn resolve_for_policy(&self, host: &str) -> Result<Vec<SocketAddr>, String> {
        self.resolve_once(host).map_err(|error| error.to_string())
    }

    fn resolve_once(&self, host: &str) -> Result<Vec<SocketAddr>, io::Error> {
        if host.eq_ignore_ascii_case("localhost") || host.ends_with(".local") {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "private hostname",
            ));
        }
        if let Some(addresses) = self
            .pinned
            .lock()
            .map_err(|_| io::Error::other("resolver mutex poisoned"))?
            .get(host)
            .cloned()
        {
            return Ok(addresses);
        }
        let addresses = (self.lookup)(host).map_err(io::Error::other)?;
        if addresses.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "no resolved addresses",
            ));
        }
        if addresses.iter().any(|address| is_private_ip(address.ip())) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "private resolved address",
            ));
        }
        self.pinned
            .lock()
            .map_err(|_| io::Error::other("resolver mutex poisoned"))?
            .insert(host.to_owned(), addresses.clone());
        Ok(addresses)
    }
}

impl Resolve for PinnedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let resolver = self.clone();
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addresses = resolver.resolve_once(&host)?;
            let addrs: Addrs = Box::new(addresses.into_iter());
            Ok(addrs)
        })
    }
}

fn policy_error(
    attempt: Attempt<'_>,
    guard: &SsrfGuard,
    allowlist: &Option<Allowlist>,
    proxies: &ProxyConfig,
    explicit: Option<&str>,
) -> reqwest::redirect::Action {
    if attempt.previous().len() >= MAX_REDIRECTS {
        return attempt.error("too many redirects");
    }
    let url = attempt.url().as_str();
    if allowlist.as_ref().is_some_and(|list| !list.allows_url(url)) {
        return attempt.error("blocked_domain: redirect target is not allowlisted");
    }
    if let Err(error) = guard.allows_url(url) {
        return attempt.error(error.to_string());
    }
    if let Err(error) = validate_proxy(proxies, attempt.url(), explicit, guard) {
        return attempt.error(error.to_string());
    }
    attempt.follow()
}

fn validate_proxy(
    config: &ProxyConfig,
    target: &url::Url,
    explicit: Option<&str>,
    guard: &SsrfGuard,
) -> Result<(), FetchError> {
    if let Some(proxy) = config.selected(target, explicit)? {
        if !matches!(proxy.scheme(), "http" | "https" | "socks5" | "socks5h") {
            return Err(FetchError::InvalidProxy(
                "unsupported proxy scheme".to_owned(),
            ));
        }
        let host = proxy
            .host_str()
            .ok_or_else(|| FetchError::InvalidProxy("proxy has no host".to_owned()))?;
        guard
            .allows_host(host, "proxy peer")
            .map_err(|error| FetchError::BlockedPrivate(error.to_string()))?;
    }
    Ok(())
}

pub(crate) fn build_http_client(
    jar: Option<Arc<Jar>>,
    proxy: Option<String>,
    proxies: ProxyConfig,
    guard: SsrfGuard,
    allowlist: Option<Allowlist>,
    allow_private: bool,
    resolver: Option<PinnedResolver>,
) -> Result<HttpClient, FetchError> {
    let redirect_guard = guard.clone();
    let redirect_allowlist = allowlist.clone();
    let guard_enabled = guard.enabled();
    let redirect_proxies = proxies.clone();
    let redirect_proxy = proxy.clone();
    let mut builder = HttpClient::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::custom(move |url| {
            let mut selected = proxies.selected(url, proxy.as_deref()).ok().flatten();
            // Remote SOCKS DNS would bypass our pinned target-address set.
            if !allow_private
                && guard_enabled
                && let Some(url) = &mut selected
                && url.scheme() == "socks5h"
            {
                url.set_scheme("socks5").expect("valid SOCKS scheme");
            }
            selected
        }))
        .redirect(Policy::custom(move |attempt| {
            policy_error(
                attempt,
                &redirect_guard,
                &redirect_allowlist,
                &redirect_proxies,
                redirect_proxy.as_deref(),
            )
        }))
        .danger_accept_invalid_certs(false);

    // Cached connections must not turn an unnamed request into a cookie session.
    if let Some(jar) = jar {
        builder = builder.cookie_provider(jar);
    }

    if !allow_private && guard_enabled {
        builder = builder.dns_resolver(resolver.unwrap_or_else(PinnedResolver::system));
    }
    builder.build().map_err(FetchError::Request)
}

pub(crate) async fn fetch(client: &FetchClient, request: Request) -> Result<Response, FetchError> {
    let parsed = url::Url::parse(&request.url)
        .map_err(|error| FetchError::InvalidRequest(format!("URL: {error}")))?;
    client.validate_request_policy(&request)?;
    let guard = if request.allow_private {
        SsrfGuard::new(true)
    } else {
        client.ssrf.clone()
    };

    validate_proxy(&client.proxies, &parsed, request.proxy.as_deref(), &guard)?;

    let _permit = client.limiter.acquire(&request.url).await;
    let host = request.url.clone();

    let method = Method::from_bytes(if request.method.is_empty() {
        b"GET"
    } else {
        request.method.as_bytes()
    })
    .map_err(|error| FetchError::InvalidRequest(format!("method: {error}")))?;
    let timeout_duration = request
        .timeout
        .filter(|value| !value.is_zero())
        .unwrap_or(client.options.timeout);
    let max_body = request
        .max_body
        .filter(|value| *value > 0)
        .unwrap_or(client.options.max_body);
    let max_compressed = request
        .max_compressed_body
        .unwrap_or(client.options.max_compressed_body);
    let http_client = client.http_client(&request, guard)?;

    let attempts = if client.options.retry {
        client.options.backoff.max_retries
    } else {
        0
    };
    let deadline = Instant::now() + timeout_duration;
    let mut last_error: Option<FetchError> = None;
    for attempt in 0..=attempts {
        if !client.limiter.allow(&host) {
            return Err(FetchError::CircuitOpen(host));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(FetchError::Timeout);
        }
        let started = Instant::now();
        let mut builder = http_client
            .request(method.clone(), parsed.clone())
            .headers(request_headers(&request, &method)?);
        if !request.body.is_empty() {
            builder = builder.body(request.body.clone());
        }

        let result = timeout(remaining, builder.send()).await;
        let response = match result {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                let retryable = retryable_request_error(&error);
                last_error = Some(FetchError::Request(error));
                // The Go oracle retries every transport error when retry is
                // enabled.  Keep the classification for circuit accounting,
                // but do not silently turn an otherwise retryable operation
                // into a one-shot request merely because reqwest classified a
                // platform-specific error differently.
                if retryable {
                    client.limiter.record_failure(&host);
                }
                if attempt >= attempts {
                    break;
                }
                let delay = client.options.backoff.delay(attempt);
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                tokio::time::sleep(delay.min(remaining)).await;
                continue;
            }
            Err(_) => {
                last_error = Some(FetchError::Timeout);
                client.limiter.record_failure(&host);
                if attempt >= attempts {
                    break;
                }
                let delay = client.options.backoff.delay(attempt);
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                tokio::time::sleep(delay.min(remaining)).await;
                continue;
            }
        };

        let status = response.status().as_u16();
        if is_transient_status(status) && attempt < attempts {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .map(parse_retry_after)
                .unwrap_or(Duration::ZERO);
            let delay = retry_after.max(client.options.backoff.delay(attempt));
            client.limiter.record_failure(&host);
            drop(response);
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            tokio::time::sleep(delay.min(remaining)).await;
            continue;
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        let body = match timeout(
            remaining,
            read_response(
                response,
                &request.url,
                max_compressed,
                max_body,
                headers::implicit_gzip(&request, &method),
                method != Method::HEAD,
            ),
        )
        .await
        {
            Ok(body) => body?,
            Err(_) => return Err(FetchError::Timeout),
        };
        client.limiter.record_success(&host);
        return Ok(Response {
            final_url: body.final_url,
            status_code: status,
            headers: body.headers,
            body: body.body,
            protocol: body.protocol,
            content_type: body.content_type,
            elapsed: started.elapsed(),
            from_cache: false,
        });
    }
    match last_error {
        Some(error) => Err(error),
        None => Err(FetchError::InvalidRequest("request failed".to_owned())),
    }
}

fn retryable_request_error(_error: &reqwest::Error) -> bool {
    true
}
