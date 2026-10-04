//! HTTP bodies and the existing Go error/security-header shape.

use bytes::Bytes;
use hyper::{
    Response,
    body::{Body as HyperBody, Frame, SizeHint},
};
use serde_json::json;
use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

pub(super) struct Body {
    bytes: Option<Bytes>,
    exact: bool,
}

impl HyperBody for Body {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        Poll::Ready(self.bytes.take().map(|bytes| Ok(Frame::data(bytes))))
    }
    fn is_end_stream(&self) -> bool {
        self.bytes.is_none()
    }
    fn size_hint(&self) -> SizeHint {
        let mut hint = SizeHint::default();
        if self.exact {
            hint.set_exact(self.bytes.as_ref().map_or(0, |bytes| bytes.len() as u64));
        }
        hint
    }
}

pub(super) type Reply = Response<Body>;

pub(super) fn bytes(
    status: u16,
    content_type: &'static str,
    data: impl Into<Bytes>,
    exact: bool,
) -> Reply {
    let data = data.into();
    let mut response = Response::new(Body {
        bytes: Some(data),
        exact,
    });
    *response.status_mut() =
        hyper::StatusCode::from_u16(status).unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR);
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        content_type.parse().expect("static MIME"),
    );
    response
}

pub(super) fn raw_json(status: u16, data: impl AsRef<str>) -> Reply {
    let rendered = data
        .as_ref()
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
        + "\n";
    let exact = rendered.len() <= 2048;
    bytes(status, "application/json", rendered, exact)
}

pub(super) fn error(status: u16, code: &str, message: &str) -> Reply {
    raw_json(status, json!({"code":code,"error":message}).to_string())
}

// These are Go parser errors, before application middleware, not JSON errors.
pub(super) fn protocol_error(reason: &'static str) -> Reply {
    let mut reply = bytes(
        400,
        "text/plain; charset=utf-8",
        format!("400 {reason}"),
        true,
    );
    reply
        .extensions_mut()
        .insert(hyper::ext::ReasonPhrase::from_static(reason.as_bytes()));
    reply.headers_mut().insert(
        hyper::header::CONNECTION,
        hyper::header::HeaderValue::from_static("close"),
    );
    reply
}

pub(super) fn unsupported() -> Reply {
    error(
        501,
        "NOT_IMPLEMENTED",
        "operation requires the unported memory pipeline",
    )
}

pub(super) fn security_headers(reply: &mut Reply) {
    for (name, value) in [
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'",
        ),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
    ] {
        reply.headers_mut().insert(
            hyper::header::HeaderName::from_static(name),
            hyper::header::HeaderValue::from_static(value),
        );
    }
}

pub(super) fn cors_headers(reply: &mut Reply, origin: &str) {
    let headers = reply.headers_mut();
    headers.insert(
        "access-control-allow-methods",
        hyper::header::HeaderValue::from_static("POST, GET, OPTIONS, DELETE"),
    );
    headers.insert(
        "access-control-allow-headers",
        hyper::header::HeaderValue::from_static("Content-Type, Authorization"),
    );
    if !origin.is_empty()
        && let Ok(value) = hyper::header::HeaderValue::from_str(origin)
    {
        headers.insert("access-control-allow-origin", value);
    }
}
