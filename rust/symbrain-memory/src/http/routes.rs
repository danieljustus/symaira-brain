//! One middleware and route owner mirroring the shipped `httpMux` chain:
//! rate limit → request log → security headers → CSRF → Host guard → per
//! route CORS → authentication → role → handler.

use super::{Server, auth::Claims, authority, log, middleware, read, wire};
use http_body_util::BodyExt;
use hyper::{Method, Request, body::Incoming};
use std::{net::IpAddr, sync::Arc, time::Duration, time::Instant};

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
/// Routes wrapped in `RequireRole(RoleReadWrite)`.
const WRITE_ROUTES: &[&str] = &[
    "/api/set",
    "/api/delete",
    "/api/sync/apply",
    "/api/sync/relay",
];
/// Routes whose handler reads a JSON body for a POST.
const BODY_ROUTES: &[&str] = &[
    "/api/set",
    "/api/search",
    "/api/token/revoke",
    "/api/sync/apply",
    "/api/sync/relay",
];

impl Server {
    pub(super) async fn handle(
        self: Arc<Self>,
        request: Request<Incoming>,
        peer: IpAddr,
    ) -> wire::Reply {
        let started = Instant::now();
        let host = match authority::effective_host(&request) {
            Ok(host) => host,
            Err(rejection) => return wire::protocol_error(rejection.reason()),
        };
        let path = request.uri().path().to_owned();
        let api = API_ROUTES.contains(&path.as_str());
        let origin = middleware::header(request.headers(), "origin").to_owned();
        let method = request.method().clone();
        let head = method == Method::HEAD;
        let origin_allowed = middleware::origin_allowed(
            &origin,
            host,
            self.listener_port
                .load(std::sync::atomic::Ordering::Relaxed),
        );
        let csrf = middleware::csrf_allowed(&method, request.headers());
        let loopback = middleware::loopback_host(host);
        let cors = api && origin_allowed && csrf && loopback;
        let key = self.limiter.key(peer, request.headers());
        let host_text = host.to_owned();
        let mut reply = if !self.limiter.allow(&key) {
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
        } else if !csrf {
            log::warn(
                "CSRF blocked request",
                &[
                    ("method", method.to_string()),
                    ("path", path.clone()),
                    ("origin", origin.clone()),
                ],
            );
            wire::error(403, "FORBIDDEN", "CSRF validation failed")
        } else if !loopback {
            log::warn(
                "Host header validation rejected",
                &[
                    ("host", host_text),
                    ("method", method.to_string()),
                    ("path", path.clone()),
                ],
            );
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
        log::info(
            "HTTP request",
            &[
                ("method", method.to_string()),
                ("path", path),
                ("status", reply.status().as_u16().to_string()),
                ("duration", log::duration(started.elapsed())),
            ],
        );
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
            return not_found();
        }
        if path == "/api/status" {
            return wire::raw_json(
                200,
                serde_json::json!({"status":"healthy","version":self.options.version,"server":"symaira-memory","embedding_backend":self.metrics.backend()}).to_string(),
            );
        }
        let authorization = middleware::header(request.headers(), "authorization");
        if !authorization.starts_with("Bearer ") {
            return wire::error(401, "FORBIDDEN", "missing or invalid Authorization header");
        }
        let claims = match self.authenticate(authorization) {
            Ok(claims) => claims,
            Err(reason) => {
                log::error(
                    "HTTP error",
                    &[
                        ("code", "FORBIDDEN".to_owned()),
                        ("msg", "invalid or expired token".to_owned()),
                        ("err", reason.to_owned()),
                    ],
                );
                return wire::error(401, "FORBIDDEN", "invalid or expired token");
            }
        };
        if WRITE_ROUTES.contains(&path)
            && let Err(message) = self.write_authorized(&claims)
        {
            return wire::error(403, "FORBIDDEN", message);
        }
        let (parts, body) = request.into_parts();
        let mut raw = Vec::new();
        let reads_body = BODY_ROUTES.contains(&path) && parts.method == Method::POST;
        if reads_body {
            let bounded = http_body_util::Limited::new(body, MAX_BODY).collect();
            match tokio::time::timeout(Duration::from_secs(30), bounded).await {
                Ok(Ok(collected)) => raw = collected.to_bytes().to_vec(),
                Ok(Err(error))
                    if error
                        .downcast_ref::<http_body_util::LengthLimitError>()
                        .is_some() =>
                {
                    return wire::error_logged(
                        413,
                        "INVALID_REQUEST",
                        "request body exceeds size limit",
                        "http: request body too large",
                    );
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
            "/api/rules" | "/api/entities" | "/api/get" => method == Method::GET,
            "/api/search" | "/api/set" | "/api/token/revoke" => method == Method::POST,
            "/api/delete" => method == Method::DELETE || method == Method::POST,
            // `/api/list`, `/api/stats` and the sync handlers (which answer
            // their own method errors) accept any method here.
            _ => true,
        };
        if !allowed {
            return wire::error(405, "METHOD_NOT_ALLOWED", "Method not allowed");
        }
        match path {
            "/api/list" => self.list(params),
            "/api/stats" => wire::raw_json(200, self.metrics.document()),
            "/api/rules" => self.rules(params),
            "/api/entities" => self.entities(),
            "/api/get" => self.get(params),
            "/api/search" => self.search(body),
            "/api/set" => self.set(body, claims),
            "/api/delete" => self.delete(params),
            "/api/token/revoke" => self.token_revoke(body),
            "/api/sync/changes" => self.sync_changes(method, params),
            "/api/sync/apply" => self.sync_apply(method, body, claims),
            "/api/sync/relay" => self.sync_relay(method, params, body),
            _ => wire::error(404, "NOT_FOUND", "not found"),
        }
    }
}

/// Anything outside the API: the embedded Web Console is retired (#763), so
/// every non-API path is the generic not-found page.
fn not_found() -> wire::Reply {
    wire::bytes(
        404,
        "text/plain; charset=utf-8",
        "404 page not found\n",
        true,
    )
}
