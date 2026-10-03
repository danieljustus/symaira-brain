use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

use crate::{honest, rate_limit::HostRateLimiter};

pub use crate::types::*;

/// Honest HTTP client with named cookie jars and fetch controls.
#[derive(Clone)]
pub struct FetchClient {
    pub(crate) options: ClientOptions,
    pub(crate) limiter: Arc<HostRateLimiter>,
    pub(crate) sessions: Arc<Mutex<std::collections::HashMap<String, Arc<reqwest::cookie::Jar>>>>,
    pub(crate) ssrf: symbrowse_core::policy::SsrfGuard,
    pub(crate) resolver: Option<honest::PinnedResolver>,
    pub(crate) proxies: honest::proxy::ProxyConfig,
    http_clients: Arc<Mutex<std::collections::HashMap<String, reqwest::Client>>>,
}

impl fmt::Debug for FetchClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FetchClient")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl FetchClient {
    pub fn new(profile: Profile, options: ClientOptions) -> Result<Self, FetchError> {
        if profile != Profile::Honest {
            return Err(FetchError::UnsupportedProfile(profile));
        }
        let resolver = honest::PinnedResolver::system();
        let policy_resolver = resolver.clone();
        Ok(Self {
            limiter: Arc::new(HostRateLimiter::with_rate(
                crate::CircuitBreakerConfig::default(),
                options.rate,
            )),
            options,
            sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
            ssrf: symbrowse_core::policy::SsrfGuard::with_lookup(false, move |host| {
                policy_resolver.resolve_for_policy(host).map(|addresses| {
                    addresses
                        .into_iter()
                        .map(|address| address.ip().to_string())
                        .collect()
                })
            }),
            resolver: Some(resolver),
            proxies: honest::proxy::ProxyConfig::from_env(),
            http_clients: Arc::new(Mutex::new(std::collections::HashMap::new())),
        })
    }

    pub fn honest() -> Result<Self, FetchError> {
        Self::new(Profile::Honest, ClientOptions::default())
    }

    #[must_use]
    pub fn with_ssrf_guard(mut self, guard: symbrowse_core::policy::SsrfGuard) -> Self {
        self.ssrf = guard;
        // Existing transports retain redirect closures and DNS resolver state.
        // Give this configured clone its own cache; do not alter other clones.
        self.http_clients = Arc::new(Mutex::new(std::collections::HashMap::new()));
        self
    }

    #[must_use]
    pub fn with_resolver(mut self, resolver: honest::PinnedResolver) -> Self {
        let policy_resolver = resolver.clone();
        self.ssrf = symbrowse_core::policy::SsrfGuard::with_lookup(false, move |host| {
            policy_resolver.resolve_for_policy(host).map(|addresses| {
                addresses
                    .into_iter()
                    .map(|address| address.ip().to_string())
                    .collect()
            })
        });
        self.resolver = Some(resolver);
        self.http_clients = Arc::new(Mutex::new(std::collections::HashMap::new()));
        self
    }

    pub(crate) fn jar(&self, session: Option<&str>) -> Arc<reqwest::cookie::Jar> {
        let Some(session) = session.filter(|value| !value.is_empty()) else {
            return Arc::new(reqwest::cookie::Jar::default());
        };
        let mut sessions = self.sessions.lock().expect("session mutex poisoned");
        Arc::clone(
            sessions
                .entry(session.to_owned())
                .or_insert_with(|| Arc::new(reqwest::cookie::Jar::default())),
        )
    }

    pub(crate) fn http_client(
        &self,
        request: &Request,
        guard: symbrowse_core::policy::SsrfGuard,
    ) -> Result<reqwest::Client, FetchError> {
        let key = format!(
            "session={};proxy={};private={};allowlist={:?}",
            request.session.as_deref().unwrap_or_default(),
            request.proxy.as_deref().unwrap_or_default(),
            request.allow_private,
            request.allowlist
        );
        if let Some(cached) = self
            .http_clients
            .lock()
            .expect("HTTP client mutex poisoned")
            .get(&key)
            .cloned()
        {
            return Ok(cached);
        }
        let built = honest::build_http_client(
            request
                .session
                .as_deref()
                .filter(|name| !name.is_empty())
                .map(|name| self.jar(Some(name))),
            request.proxy.clone(),
            self.proxies.clone(),
            guard,
            request.allowlist.clone(),
            request.allow_private,
            self.resolver.clone(),
        )?;
        self.http_clients
            .lock()
            .expect("HTTP client mutex poisoned")
            .insert(key, built.clone());
        Ok(built)
    }

    pub fn close(&self) {
        self.sessions
            .lock()
            .expect("session mutex poisoned")
            .clear();
        self.http_clients
            .lock()
            .expect("HTTP client mutex poisoned")
            .clear();
    }

    /// Apply the current request allowlist and SSRF policy without performing I/O.
    pub fn validate_request_policy(&self, request: &Request) -> Result<(), FetchError> {
        let parsed = url::Url::parse(&request.url)
            .map_err(|error| FetchError::InvalidRequest(format!("URL: {error}")))?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(FetchError::InvalidRequest(
                "only HTTP(S) URLs with a host are supported".to_owned(),
            ));
        }
        if request
            .allowlist
            .as_ref()
            .is_some_and(|list| !list.allows_url(&request.url))
        {
            return Err(FetchError::BlockedDomain(request.url.clone()));
        }
        let guard = if request.allow_private {
            symbrowse_core::policy::SsrfGuard::new(true)
        } else {
            self.ssrf.clone()
        };
        guard
            .allows_url(&request.url)
            .map_err(|error| FetchError::BlockedPrivate(error.to_string()))
    }

    pub async fn fetch(&self, request: Request) -> Result<Response, FetchError> {
        honest::fetch(self, request).await
    }

    pub fn fetch_blocking(&self, request: Request) -> Result<Response, FetchError> {
        tokio::runtime::Handle::try_current().map_or_else(
            |_| {
                tokio::runtime::Runtime::new()
                    .expect("tokio runtime")
                    .block_on(self.fetch(request))
            },
            |_| {
                Err(FetchError::InvalidRequest(
                    "fetch_blocking cannot run inside a Tokio runtime".to_owned(),
                ))
            },
        )
    }
}

/// Object-safe async transport boundary for callers that do not need internals.
pub trait Client: Send + Sync {
    fn fetch<'a>(
        &'a self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Response, FetchError>> + Send + 'a>>;
    fn close(&self);
}

impl Client for FetchClient {
    fn fetch<'a>(
        &'a self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Response, FetchError>> + Send + 'a>> {
        Box::pin(FetchClient::fetch(self, request))
    }

    fn close(&self) {
        FetchClient::close(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_honest_client_shares_a_pinned_resolver_with_policy() {
        let client = FetchClient::honest().unwrap();
        assert!(client.resolver.is_some());
    }
}
