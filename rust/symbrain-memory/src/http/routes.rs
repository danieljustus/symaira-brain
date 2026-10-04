//! One middleware and route owner; unported routes remain explicit migration limits.

use super::{Server, auth::Claims, middleware, read, wire};
use http_body_util::BodyExt;
use hyper::{Method, Request, body::Incoming};
use std::{net::IpAddr, sync::Arc, time::Duration};

const MAX_BODY: usize = 1 << 20;
const API_ROUTES: &[&str] = &[
    "/api/status",
    "/api/stats",
    "/api/search",
    "/api/set",
    "/api/list",
    "/api/sync/changes",
    "/api/sync/apply",
    "/api/sync/relay",
    "/api/get",
    "/api/delete",
    "/api/rules",
    "/api/entities",
    "/api/token/revoke",
];

impl Server {
    pub(super) async fn handle(
        self: Arc<Self>,
        request: Request<Incoming>,
        peer: IpAddr,
    ) -> wire::Reply {
        let path = request.uri().path().to_owned();
        let api = API_ROUTES.contains(&path.as_str());
        let origin = middleware::header(request.headers(), "origin").to_owned();
        let method = request.method().clone();
        let head = method == Method::HEAD;
        let origin_allowed = middleware::origin_allowed(
            &origin,
            middleware::header(request.headers(), "host"),
            self.listener_port
                .load(std::sync::atomic::Ordering::Relaxed),
        );
        let cors = api
            && origin_allowed
            && middleware::csrf_allowed(&method, request.headers())
            && middleware::loopback_host(middleware::header(request.headers(), "host"));
        let mut reply = if !self.limiter.allow(peer) {
            let mut reply = wire::bytes(
                429,
                "application/json",
                r#"{"error":"rate limit exceeded"}"#,
                true,
            );
            reply
                .headers_mut()
                .insert("retry-after", hyper::header::HeaderValue::from_static("60"));
            return reply;
        } else if !middleware::csrf_allowed(&method, request.headers()) {
            wire::error(403, "FORBIDDEN", "CSRF validation failed")
        } else if !middleware::loopback_host(middleware::header(request.headers(), "host")) {
            wire::error(403, "FORBIDDEN", "non-loopback Host header rejected")
        } else if api && !origin_allowed {
            wire::error(403, "FORBIDDEN", "origin not allowed")
        } else if api && method == Method::OPTIONS {
            {
                let mut response = wire::bytes(200, "text/plain; charset=utf-8", "", true);
                response.headers_mut().remove(hyper::header::CONTENT_TYPE);
                response
            }
        } else {
            self.authorized_route(request, &path).await
        };
        wire::security_headers(&mut reply);
        if cors {
            wire::cors_headers(&mut reply, &origin);
        }
        if head {
            // Keep the full representation length while never sending a body.
            use hyper::body::Body;
            if let Some(length) = reply.body().size_hint().exact()
                && let Ok(value) = hyper::header::HeaderValue::from_str(&length.to_string())
            {
                reply
                    .headers_mut()
                    .insert(hyper::header::CONTENT_LENGTH, value);
            }
            reply = reply.map(|_| wire::bytes(200, "text/plain", "", true).into_body());
        }
        reply
    }

    async fn authorized_route(
        self: &Arc<Self>,
        request: Request<Incoming>,
        path: &str,
    ) -> wire::Reply {
        if !API_ROUTES.contains(&path) {
            return static_asset(path, request.method());
        }
        if path == "/api/status" {
            return wire::raw_json(200,serde_json::json!({"status":"healthy","version":self.options.version,"server":"symaira-memory","embedding_backend":"ollama"}).to_string());
        }
        let authorization = middleware::header(request.headers(), "authorization");
        if !authorization.starts_with("Bearer ") {
            return wire::error(401, "FORBIDDEN", "missing or invalid Authorization header");
        }
        let Some(claims) = self.authenticate(authorization) else {
            return wire::error(401, "FORBIDDEN", "invalid or expired token");
        };
        if [
            "/api/set",
            "/api/delete",
            "/api/sync/apply",
            "/api/sync/relay",
        ]
        .contains(&path)
            && let Err(message) = self.write_authorized(&claims)
        {
            return wire::error(403, "FORBIDDEN", message);
        }
        let (parts, body) = request.into_parts();
        let mut raw = Vec::new();
        if ["/api/set", "/api/search", "/api/token/revoke"].contains(&path)
            && parts.method == Method::POST
        {
            let bounded = http_body_util::Limited::new(body, MAX_BODY).collect();
            match tokio::time::timeout(Duration::from_secs(30), bounded).await {
                Ok(Ok(collected)) => raw = collected.to_bytes().to_vec(),
                Ok(Err(error))
                    if error
                        .downcast_ref::<http_body_util::LengthLimitError>()
                        .is_some() =>
                {
                    return wire::error(413, "INVALID_REQUEST", "request body exceeds size limit");
                }
                _ => return wire::error(400, "INVALID_REQUEST", "Bad request body"),
            }
        }
        let params = read::query(parts.uri.query());
        let server = Arc::clone(self);
        let method = parts.method;
        let path = path.to_owned();
        match tokio::task::spawn_blocking(move || {
            server.dispatch(&path, &method, &params, &raw, &claims)
        })
        .await
        {
            Ok(reply) => reply,
            Err(_) => wire::error(500, "INTERNAL_ERROR", "memory operation failed"),
        }
    }

    fn dispatch(
        &self,
        path: &str,
        method: &Method,
        params: &std::collections::BTreeMap<String, String>,
        body: &[u8],
        claims: &Claims,
    ) -> wire::Reply {
        let allowed = match path {
            "/api/list" => true,
            "/api/rules" | "/api/entities" | "/api/get" => method == Method::GET,
            "/api/search" | "/api/set" | "/api/token/revoke" => method == Method::POST,
            "/api/delete" => method == Method::DELETE || method == Method::POST,
            _ => return wire::unsupported(),
        };
        if !allowed {
            return wire::error(405, "METHOD_NOT_ALLOWED", "Method not allowed");
        }
        match path {
            "/api/list" => self.list(params),
            "/api/rules" => self.rules(params),
            "/api/entities" => self.entities(),
            "/api/get" => self.get(params),
            "/api/search" => self.search(body),
            "/api/set" => self.set(body, claims),
            "/api/delete" => self.delete(params),
            "/api/token/revoke" => self.token_revoke(body),
            _ => wire::unsupported(),
        }
    }
}

fn static_asset(path: &str, method: &Method) -> wire::Reply {
    if path != "/" && method != Method::GET && method != Method::HEAD {
        let mut reply = wire::bytes(
            405,
            "text/plain; charset=utf-8",
            "Method Not Allowed\n",
            true,
        );
        reply.headers_mut().insert(
            "allow",
            hyper::header::HeaderValue::from_static("GET, HEAD"),
        );
        return reply;
    }
    let (bytes, mime, exact) = match path {
        "/" => (
            include_bytes!("../../assets/web/index.html").as_slice(),
            "text/html; charset=utf-8",
            false,
        ),
        "/style.css" => (
            include_bytes!("../../assets/web/style.css").as_slice(),
            "text/css; charset=utf-8",
            true,
        ),
        "/app.js" => (
            include_bytes!("../../assets/web/app.js").as_slice(),
            "text/javascript; charset=utf-8",
            true,
        ),
        _ => {
            return wire::bytes(
                404,
                "text/plain; charset=utf-8",
                "404 page not found\n",
                true,
            );
        }
    };
    let mut reply = wire::bytes(200, mime, bytes, exact);
    if path != "/" {
        reply.headers_mut().insert(
            "accept-ranges",
            hyper::header::HeaderValue::from_static("bytes"),
        );
    }
    reply
}
