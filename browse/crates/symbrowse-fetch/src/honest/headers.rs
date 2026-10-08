//! Go-compatible honest request defaults and explicit header precedence.
use super::DEFAULT_USER_AGENT;
use crate::{FetchError, Request};
use reqwest::{
    Method,
    header::{
        ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, HeaderMap, HeaderName, HeaderValue, RANGE,
        USER_AGENT,
    },
};

pub(super) fn request_headers(request: &Request, method: &Method) -> Result<HeaderMap, FetchError> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.5"));
    for (name, value) in &request.headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| FetchError::InvalidRequest(format!("header name: {error}")))?;
        let value = HeaderValue::from_str(value)
            .map_err(|error| FetchError::InvalidRequest(format!("header value: {error}")))?;
        // Header.Set replaces defaults; RequestBuilder::header would append.
        headers.insert(name, value);
    }
    if let Some(value) = request
        .user_agent
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(value)
                .map_err(|error| FetchError::InvalidRequest(format!("user agent: {error}")))?,
        );
    }
    // net/http omits an explicitly empty User-Agent. reqwest has no default UA.
    if headers.get(USER_AGENT).is_some_and(HeaderValue::is_empty) {
        headers.remove(USER_AGENT);
    }
    // Match net/http's implicit compression advertisement, including HEAD/ranges.
    if headers
        .get(ACCEPT_ENCODING)
        .is_none_or(HeaderValue::is_empty)
        && headers.get(RANGE).is_none_or(HeaderValue::is_empty)
        && method != Method::HEAD
    {
        headers.append(ACCEPT_ENCODING, HeaderValue::from_static("gzip"));
    }
    Ok(headers)
}

pub(super) fn implicit_gzip(request: &Request, method: &Method) -> bool {
    method != Method::HEAD
        && !request.headers.iter().any(|(name, value)| {
            (name.eq_ignore_ascii_case("accept-encoding") || name.eq_ignore_ascii_case("range"))
                && !value.is_empty()
        })
}
