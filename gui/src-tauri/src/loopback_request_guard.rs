//! The admission rule for the reify-debug HTTP server: serve only requests whose
//! Host names the bound loopback listener and that carry no foreign Origin.
//! Rationale: docs/debug-mcp-contract.md §7.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::{GetAll, HOST, ORIGIN};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde_json::json;

/// `router` with every request refused (403) unless it is addressed to the
/// listener bound at `bound` (see [`ListenerAuthorities::check`]).
pub(crate) fn admit_only_requests_addressed_to(router: Router, bound: SocketAddr) -> Router {
    router.layer(middleware::from_fn_with_state(
        Arc::new(ListenerAuthorities::of(bound)),
        refuse_unless_addressed_to_listener,
    ))
}

async fn refuse_unless_addressed_to_listener(
    State(listener): State<Arc<ListenerAuthorities>>,
    request: Request,
    next: Next,
) -> Response {
    match listener.check(request.headers()) {
        Ok(()) => next.run(request).await,
        Err(refusal) => {
            let message = listener.refusal_message(&refusal);
            tracing::warn!("{message}");
            (StatusCode::FORBIDDEN, Json(json!({ "error": message }))).into_response()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refusal {
    MissingHost,
    ForeignHost(String),
    ForeignOrigin(String),
}

/// The Host and Origin values that name one bound listener.
#[derive(Debug)]
pub(crate) struct ListenerAuthorities {
    hosts: [String; 2],
    origins: [String; 2],
}

impl ListenerAuthorities {
    pub(crate) fn of(bound: SocketAddr) -> Self {
        let hosts = [bound.to_string(), format!("localhost:{}", bound.port())];
        let origins = hosts.clone().map(|host| format!("http://{host}"));
        Self { hosts, origins }
    }

    /// `Ok` when at least one Host is present, every Host names this listener,
    /// and every Origin (if any) is this listener's own `http://` origin.
    pub(crate) fn check(&self, headers: &HeaderMap) -> Result<(), Refusal> {
        let hosts = headers.get_all(HOST);
        if hosts.iter().next().is_none() {
            return Err(Refusal::MissingHost);
        }
        if let Some(host) = first_unaccepted(hosts, &self.hosts) {
            return Err(Refusal::ForeignHost(host));
        }
        if let Some(origin) = first_unaccepted(headers.get_all(ORIGIN), &self.origins) {
            return Err(Refusal::ForeignOrigin(origin));
        }
        Ok(())
    }

    fn refusal_message(&self, refusal: &Refusal) -> String {
        let [ip_host, localhost_host] = &self.hosts;
        let [ip_origin, localhost_origin] = &self.origins;
        match refusal {
            Refusal::MissingHost => format!(
                "reify-debug refused a request with no Host header; \
                 accepted Host: {ip_host} or {localhost_host}"
            ),
            Refusal::ForeignHost(host) => format!(
                "reify-debug refused Host {host:?}: it does not name this listener; \
                 accepted Host: {ip_host} or {localhost_host}"
            ),
            Refusal::ForeignOrigin(origin) => format!(
                "reify-debug refused Origin {origin:?}: it is not this listener's origin; \
                 accepted Origin: none, {ip_origin} or {localhost_origin}"
            ),
        }
    }
}

fn first_unaccepted(values: GetAll<'_, HeaderValue>, accepted: &[String; 2]) -> Option<String> {
    values
        .iter()
        .find(|value| {
            !accepted
                .iter()
                .any(|form| value.as_bytes().eq_ignore_ascii_case(form.as_bytes()))
        })
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
}
