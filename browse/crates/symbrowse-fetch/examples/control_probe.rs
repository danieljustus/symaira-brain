//! Supplemental process oracle over the production honest transport.
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{self, Read},
    time::Duration,
};
use symbrowse_fetch::{ClientOptions, FetchClient, FetchError, Profile, Request};

#[allow(dead_code)]
#[path = "../src/honest/proxy.rs"]
mod proxy;

#[derive(Deserialize)]
struct Case {
    id: String,
    url: String,
    #[serde(default)]
    method: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    body: String,
    user_agent: Option<String>,
    timeout_ms: Option<u64>,
    max_body: Option<usize>,
    session: Option<String>,
    allow_private: Option<bool>,
    proxy: Option<String>,
    #[serde(default)]
    route_only: bool,
}

#[tokio::main]
async fn main() {
    let mut input = String::new();
    io::stdin()
        .take(1 << 20)
        .read_to_string(&mut input)
        .unwrap();
    let cases: Vec<Case> = serde_json::from_str(&input).unwrap();
    let client = FetchClient::new(
        Profile::Honest,
        ClientOptions::default().timeout(Duration::from_secs(1)),
    )
    .unwrap();
    for case in cases {
        if case.route_only {
            let observation = match proxy::ProxyConfig::from_env()
                .selected(&url::Url::parse(&case.url).unwrap(), case.proxy.as_deref())
            {
                Ok(route) => {
                    json!({"id":case.id,"error":"","route":route.map(|u| u.to_string()).unwrap_or_default()})
                }
                Err(error) => json!({"id":case.id,"error":"transport","detail":error.to_string()}),
            };
            println!("{observation}");
            continue;
        }
        let request = Request {
            url: case.url,
            method: case.method,
            headers: case.headers,
            body: case.body.into_bytes(),
            user_agent: case.user_agent,
            timeout: case.timeout_ms.map(Duration::from_millis),
            max_body: case.max_body,
            session: case.session,
            allow_private: case.allow_private.unwrap_or(true),
            proxy: case.proxy,
            ..Request::default()
        };
        let observation = match client.fetch(request).await {
            Ok(response) => {
                json!({"id":case.id,"status":response.status_code,"url":response.final_url,
                "body_hex":response.body.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                "headers":response.headers,"protocol":response.protocol,"error":""})
            }
            Err(error) => {
                let kind = match &error {
                    FetchError::Timeout => "timeout",
                    FetchError::BodyTooLarge(_) => "too_large",
                    FetchError::Request(e) if e.is_redirect() => "redirect",
                    FetchError::InvalidRequest(_) => "invalid_request",
                    _ => "transport",
                };
                json!({"id":case.id,"error":kind,"detail":error.to_string()})
            }
        };
        println!("{observation}");
    }
    client.close();
}
