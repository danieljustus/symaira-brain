//! Bounded redirect hops retain written ports before WHATWG URL normalization.
use super::{headers::request_headers, proxy::literal_port};
use crate::{FetchClient, FetchError, Request};
use reqwest::{
    Method,
    header::{HOST, HeaderMap, HeaderValue, LOCATION, REFERER},
};
use symbrowse_core::policy::SsrfGuard;
use url::{Position, Url};

pub(super) fn validate_proxy(
    client: &FetchClient,
    raw: &str,
    explicit: Option<&str>,
    guard: &SsrfGuard,
) -> Result<Option<Url>, FetchError> {
    let target =
        Url::parse(raw).map_err(|error| FetchError::InvalidRequest(format!("URL: {error}")))?;
    let selected = client.proxies.selected(&target, raw, explicit)?;
    if let Some(proxy) = &selected {
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
    Ok(selected)
}

pub(super) fn render(url: &Url, port: Option<&str>) -> String {
    port.map_or_else(
        || url.to_string(),
        |port| {
            format!(
                "{}{}:{port}{}",
                &url[..Position::BeforeHost],
                url.host_str().unwrap_or_default(),
                &url[Position::AfterPort..]
            )
        },
    )
}

fn destination(current: &str, location: &str) -> Result<String, FetchError> {
    let base = Url::parse(current).map_err(|error| FetchError::Redirect(error.to_string()))?;
    let next = base
        .join(location)
        .map_err(|error| FetchError::Redirect(error.to_string()))?;
    let authority = location.starts_with("//") || Url::parse(location).is_ok();
    let port = if authority {
        literal_port(location)
    } else {
        literal_port(current)
    };
    Ok(render(&next, port))
}

fn trusted_redirect(original: &Url, target: &Url) -> bool {
    let source = original.host_str().unwrap_or_default();
    let host = target.host_str().unwrap_or_default();
    host == source
        || (!host.contains([':', '%'])
            && host.strip_suffix(source).is_some_and(|p| p.ends_with('.')))
}

fn redirect_headers(
    headers: &mut HeaderMap,
    previous: &str,
    next: &str,
    original: &Url,
    body: bool,
    sensitive: &mut bool,
) {
    let before = Url::parse(previous).expect("validated URL");
    let after = Url::parse(next).expect("validated URL");
    *sensitive |= !trusted_redirect(original, &after);
    if *sensitive {
        for name in [
            "authorization",
            "www-authenticate",
            "cookie",
            "cookie2",
            "proxy-authorization",
            "proxy-authenticate",
        ] {
            headers.remove(name);
        }
    }
    if !body {
        for name in [
            "content-encoding",
            "content-language",
            "content-location",
            "content-type",
            "content-length",
            "transfer-encoding",
        ] {
            headers.remove(name);
        }
    }
    if before.scheme() != "https" || after.scheme() == "https" {
        if !headers.contains_key(REFERER) {
            let mut referer = before.clone();
            let _ = referer.set_username("");
            let _ = referer.set_password(None);
            referer.set_fragment(None);
            if let Ok(value) = HeaderValue::from_str(&render(&referer, literal_port(previous))) {
                headers.insert(REFERER, value);
            }
        }
    } else {
        headers.remove(REFERER);
    }
}

/// One retry attempt, with all hops inside its caller's shared deadline.
pub(super) async fn send(
    client: &FetchClient,
    request: &Request,
    original_method: &Method,
    guard: &SsrfGuard,
) -> Result<(reqwest::Response, String, Method), FetchError> {
    let original =
        Url::parse(&request.url).map_err(|error| FetchError::InvalidRequest(error.to_string()))?;
    let mut current = render(&original, literal_port(&request.url));
    let mut method = original_method.clone();
    let mut body = true;
    let mut sensitive = false;
    let mut headers = request_headers(request, &method)?;
    // Custom Referer remains custom; automatic Referer changes at every hop.
    let custom_referer = headers.contains_key(REFERER);
    for count in 0..10 {
        let parsed =
            Url::parse(&current).map_err(|error| FetchError::Redirect(error.to_string()))?;
        let proxy = validate_proxy(client, &current, request.proxy.as_deref(), guard)?;
        let http = client.http_client(request, guard.clone(), proxy.clone())?;
        let mut hop_headers = headers.clone();
        if literal_port(&current).is_some() {
            // Preserve Go's Host authority even when Url removed default-port zeroes.
            let host = render(&parsed, literal_port(&current));
            let authority = host
                .split_once("://")
                .expect("HTTP URL")
                .1
                .split(['/', '?', '#'])
                .next()
                .unwrap_or_default()
                .rsplit('@')
                .next()
                .unwrap_or_default();
            hop_headers.insert(
                HOST,
                HeaderValue::from_str(authority)
                    .map_err(|e| FetchError::InvalidRequest(e.to_string()))?,
            );
        }
        if parsed.scheme() == "http"
            && let Some(proxy) = proxy
                .as_ref()
                .filter(|proxy| matches!(proxy.scheme(), "http" | "https"))
        {
            super::proxy_uri::authorize(&http, proxy, &mut hop_headers)?;
        }
        let raw_proxy = proxy.filter(|proxy| super::proxy_uri::needed(&current, &parsed, proxy));
        let mut builder = http.request(method.clone(), parsed).headers(hop_headers);
        if body && !request.body.is_empty() {
            builder = builder.body(request.body.clone());
        }
        let mut response = if let Some(proxy) = raw_proxy {
            super::proxy_uri::send(
                client,
                request,
                &current,
                proxy,
                !request.allow_private && guard.enabled(),
                builder.build().map_err(FetchError::Request)?,
            )
            .await?
        } else {
            builder.send().await.map_err(FetchError::Request)?
        };
        let status = response.status().as_u16();
        if !matches!(status, 301 | 302 | 303 | 307 | 308) {
            return Ok((response, current, method));
        }
        let Some(location) = response
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty())
        else {
            return Ok((response, current, method));
        };
        if count == 9 {
            return Err(FetchError::Redirect("too many redirects".to_owned()));
        }
        let next = destination(&current, location)?;
        if request
            .allowlist
            .as_ref()
            .is_some_and(|list| !list.allows_url(&next))
        {
            return Err(FetchError::Redirect(
                "blocked_domain: redirect target is not allowlisted".to_owned(),
            ));
        }
        guard
            .allows_url(&next)
            .map_err(|error| FetchError::Redirect(error.to_string()))?;
        validate_proxy(client, &next, request.proxy.as_deref(), guard)
            .map_err(|error| FetchError::Redirect(error.to_string()))?;
        if matches!(status, 301..=303) {
            if method != Method::HEAD {
                method = Method::GET;
            }
            body = false;
        }
        if !custom_referer {
            headers.remove(REFERER);
        }
        redirect_headers(
            &mut headers,
            &current,
            &next,
            &original,
            body,
            &mut sensitive,
        );
        // Like Go, bounded draining permits keepalive without unbounded redirect bodies.
        if response.content_length().is_none_or(|size| size <= 2048) {
            let mut remaining = 2048usize;
            while remaining > 0 {
                match response.chunk().await {
                    Ok(Some(chunk)) => remaining = remaining.saturating_sub(chunk.len()),
                    _ => break,
                }
            }
        }
        current = next;
    }
    unreachable!("redirect count checked before following")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_and_absolute_redirects_retain_literal_authority_ports() {
        assert_eq!(
            destination("http://example.com:080/a", "/b").unwrap(),
            "http://example.com:080/b"
        );
        assert_eq!(
            destination("http://example.com:080/a", "http://example.com:00080/b").unwrap(),
            "http://example.com:00080/b"
        );
        assert_eq!(
            destination("http://example.com:080/a", "//[2001:4860::1]:080/b").unwrap(),
            "http://[2001:4860::1]:080/b"
        );
    }
}
