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
    redirect::Policy,
};
use symbrowse_core::policy::{SsrfGuard, is_private_ip};
use tokio::time::timeout;

use crate::{
    client::{FetchClient, FetchError, Request, Response},
    retry::{is_transient_status, parse_retry_after},
};

const DEFAULT_USER_AGENT: &str = "symfetch/0.1 (+https://github.com/danieljustus/symaira-fetch)";

#[path = "honest/headers.rs"]
mod headers;
#[path = "honest/proxy.rs"]
pub(crate) mod proxy;
#[path = "honest/redirect.rs"]
mod redirect;
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

pub(crate) fn build_http_client(
    jar: Option<Arc<Jar>>,
    proxy: Option<url::Url>,
    guard: SsrfGuard,
    allow_private: bool,
    resolver: Option<PinnedResolver>,
) -> Result<HttpClient, FetchError> {
    let guard_enabled = guard.enabled();
    let mut builder = HttpClient::builder()
        .no_proxy()
        .redirect(Policy::none())
        .danger_accept_invalid_certs(false);
    if let Some(mut proxy) = proxy {
        // Remote SOCKS DNS would bypass our pinned target-address set.
        if !allow_private && guard_enabled && proxy.scheme() == "socks5h" {
            proxy.set_scheme("socks5").expect("valid SOCKS scheme");
        }
        builder = builder.proxy(reqwest::Proxy::all(proxy.as_str()).map_err(FetchError::Request)?);
    }

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
    client.validate_request_policy(&request)?;
    let guard = if request.allow_private {
        SsrfGuard::new(true)
    } else {
        client.ssrf.clone()
    };

    redirect::validate_proxy(client, &request.url, request.proxy.as_deref(), &guard)?;

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
        let result = timeout(remaining, redirect::send(client, &request, &method, &guard)).await;
        let (response, final_url, final_method) = match result {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                last_error = Some(error);
                // The Go oracle retries every transport error when retry is
                // enabled, including a failed redirect policy check.
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
                headers::implicit_gzip(&request, &final_method),
                final_method != Method::HEAD,
            ),
        )
        .await
        {
            Ok(body) => body?,
            Err(_) => return Err(FetchError::Timeout),
        };
        client.limiter.record_success(&host);
        return Ok(Response {
            final_url,
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
