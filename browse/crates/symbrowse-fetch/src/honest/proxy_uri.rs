//! Preserve normalization-sensitive absolute-form URIs without a reqwest fork.
use super::{
    proxy::literal_port,
    proxy_body::{ConnectionTask, OwnedBody},
    proxy_io,
    redirect::render,
};
use crate::{FetchClient, FetchError, Request};
use hyper_util::rt::TokioIo;
use reqwest::{
    Method,
    cookie::CookieStore,
    header::{AUTHORIZATION, COOKIE, HeaderMap, PROXY_AUTHORIZATION, SET_COOKIE},
};
use url::Url;

pub(super) fn needed(raw: &str, parsed: &Url, proxy: &Url) -> bool {
    parsed.scheme() == "http"
        && matches!(proxy.scheme(), "http" | "https")
        && literal_port(raw).is_some()
        && literal_port(raw) != literal_port(parsed.as_str())
}

pub(super) fn authorize(
    http: &reqwest::Client,
    proxy: &Url,
    headers: &mut HeaderMap,
) -> Result<(), FetchError> {
    // Go writes its transport proxy credentials after caller header values.
    // Reuse reqwest's userinfo decoding and Basic encoding, without any I/O.
    let auth = http
        .request(Method::GET, proxy.clone())
        .build()
        .map_err(FetchError::Request)?;
    if let Some(value) = auth.headers().get(AUTHORIZATION) {
        headers.append(PROXY_AUTHORIZATION, value.clone());
    }
    Ok(())
}

pub(super) async fn send(
    client: &FetchClient,
    request: &Request,
    raw: &str,
    proxy: Url,
    protected: bool,
    built: reqwest::Request,
) -> Result<reqwest::Response, FetchError> {
    // RequestBuilder already applies URL-userinfo auth and caller-header precedence.
    let target = built.url().clone();
    let mut uri = render(&target, literal_port(raw));
    uri.truncate(uri.find('#').unwrap_or(uri.len()));
    let mut outgoing: http::Request<reqwest::Body> =
        built.try_into().map_err(FetchError::Request)?;
    *outgoing.uri_mut() = uri
        .parse()
        .map_err(|error: http::uri::InvalidUri| FetchError::ProxyTransport(error.to_string()))?;
    // Use the existing named jar; unnamed requests never acquire persistent cookies.
    let jar = request
        .session
        .as_deref()
        .filter(|name| !name.is_empty())
        .map(|name| client.jar(Some(name)));
    if !outgoing.headers().contains_key(COOKIE)
        && let Some(cookie) = jar.as_ref().and_then(|jar| jar.cookies(&target))
    {
        outgoing.headers_mut().insert(COOKIE, cookie);
    }
    let socket = proxy_io::connect(client, &proxy, protected).await?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(socket))
        .await
        .map_err(|error| FetchError::ProxyTransport(error.to_string()))?;
    let task = ConnectionTask(tokio::spawn(async move {
        let _ = connection.await;
    }));
    let response = sender
        .send_request(outgoing)
        .await
        .map_err(|error| FetchError::ProxyTransport(error.to_string()))?;
    drop(sender);
    if let Some(jar) = jar {
        jar.set_cookies(&mut response.headers().get_all(SET_COOKIE).iter(), &target);
    }
    let (parts, body) = response.into_parts();
    // Cancellation/size failures/body drop also abort the owned connection task.
    // The existing caller owns the common operation deadline and bounded decoding.
    Ok(reqwest::Response::from(http::Response::from_parts(
        parts,
        reqwest::Body::wrap(OwnedBody::new(body, task)),
    )))
}
