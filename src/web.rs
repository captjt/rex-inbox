//! The hub's HTTP side, for the phone. It listens on localhost only;
//! `tailscale serve` publishes it on the tailnet with HTTPS and tells the hub
//! who is calling (the `Tailscale-User-Login` header). The hub answers only
//! the Mac's own Tailscale user.
//!
//! - `POST /api/call` `{"method", "params"}`: the socket's methods, as you.
//! - `GET /api/events`: the hub's events, as server-sent events.
//! - `GET /*`: the web app (see web/).

use anyhow::{Context, Result};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::time::Duration;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::api;
use crate::hub::Shared;

/// What the phone may call. No naming panes, no focusing panes on the Mac,
/// no stopping the hub. It may start an agent, but only Claude Code or Codex
/// (`agents.start` runs nothing else).
const ALLOWED: &[&str] = &[
    "status",
    "inbox",
    "panes.list",
    "panes.screen",
    "hosts.list",
    "dirs.list",
    "agents.start",
    "show",
    "screen",
    "approve",
    "deny",
    "preview",
    "done",
    "alerts.snooze",
    "alerts.reopen",
    "messages.list",
    "messages.read",
    "messages.reply",
    "messages.send",
    "sessions.rename",
    "push.key",
    "push.subscribe",
    "push.unsubscribe",
    "push.test",
];

pub fn addr() -> Option<SocketAddr> {
    let v = std::env::var("REX_INBOX_HTTP").unwrap_or_else(|_| "127.0.0.1:7430".into());
    if v == "off" {
        return None;
    }
    v.parse().ok()
}

pub async fn serve(shared: Shared, addr: SocketAddr) -> Result<()> {
    let app = Router::new()
        .route("/api/call", post(call))
        .route("/api/events", get(events))
        .fallback(crate::webapp::asset)
        .layer(middleware::from_fn_with_state(shared.clone(), guard))
        .with_state(shared);
    let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("binding {addr}"))?;
    say!("web app on http://{addr} (publish it with: tailscale serve --bg {})", addr.port());
    axum::serve(listener, app).await?;
    Ok(())
}

fn deny(status: StatusCode, why: &str) -> Response {
    (status, why.to_string()).into_response()
}

/// Only the owner, only at our own host names, and no cross-site writes.
async fn guard(State(shared): State<Shared>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    // DNS rebinding: a page on another domain that resolves to this Mac
    // would be "same-origin" with us and could send any header. Its Host
    // header still names that domain, so refuse every host but our own.
    let web_url = shared.lock().web_url.clone();
    if !host_ok(headers, web_url.as_deref()) {
        return deny(StatusCode::MISDIRECTED_REQUEST, "unknown host");
    }
    if std::env::var("REX_INBOX_WEB_DEV").is_err() {
        let login = headers.get("tailscale-user-login").and_then(|v| v.to_str().ok());
        let owner = shared.lock().owner.clone();
        match (login, owner) {
            (Some(l), Some(o)) if l.eq_ignore_ascii_case(&o) => {}
            (_, None) => return deny(StatusCode::SERVICE_UNAVAILABLE, "rex-inbox doesn't know its owner yet: is Tailscale running on the Mac?"),
            _ => return deny(StatusCode::FORBIDDEN, "rex-inbox answers only the Mac's Tailscale user, through tailscale serve"),
        }
    }
    if req.method() != Method::GET && req.method() != Method::HEAD && !same_site(headers) {
        return deny(StatusCode::FORBIDDEN, "cross-site request refused");
    }
    next.run(req).await
}

/// This Mac by loopback, or its name on the tailnet (from tailscale serve).
fn host_ok(h: &HeaderMap, web_url: Option<&str>) -> bool {
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    let name = match host.rsplit_once(':') {
        Some((n, port)) if port.chars().all(|c| c.is_ascii_digit()) => n,
        _ => host,
    };
    if matches!(name, "127.0.0.1" | "localhost" | "[::1]") {
        return true;
    }
    web_url
        .map(|u| u.trim_start_matches("https://").trim_end_matches('/'))
        .is_some_and(|tn| name.eq_ignore_ascii_case(tn))
}

/// A write must carry our header (which forces a CORS preflight we never
/// answer), and its Origin, when sent, must be this host.
fn same_site(h: &HeaderMap) -> bool {
    if h.get("x-rex-inbox").is_none() {
        return false;
    }
    let Some(origin) = h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else { return true };
    let host = h.get("x-forwarded-host").or_else(|| h.get(header::HOST)).and_then(|v| v.to_str().ok()).unwrap_or("");
    let origin_host = origin.split_once("://").map_or(origin, |(_, rest)| rest);
    origin_host.eq_ignore_ascii_case(host)
}

async fn call(State(shared): State<Shared>, Json(req): Json<api::Request>) -> Json<api::Response> {
    if !ALLOWED.contains(&req.method.as_str()) {
        return Json(api::Response::err(format!("{} isn't available from the web app", req.method)));
    }
    let mut params = if req.params.is_object() { req.params } else { json!({}) };
    // The phone is always the person.
    params["caller"] = json!({ "as_me": true });
    let result = match req.method.as_str() {
        m if m.starts_with("push.") => crate::push::dispatch(&shared, m, &params).await,
        m => crate::methods::dispatch(&shared, m, params).await,
    };
    Json(match result {
        Ok(v) => api::Response::ok(v),
        Err(e) => api::Response::err(format!("{e:#}")),
    })
}

async fn events(State(shared): State<Shared>) -> impl IntoResponse {
    let rx = shared.lock().events.subscribe();
    let stream = BroadcastStream::new(rx)
        .filter_map(|r| r.ok())
        .map(|ev| Event::default().json_data(ev));
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(20)))
}

// ---- Tailscale ----

pub(crate) fn tailscale_bin() -> Option<std::path::PathBuf> {
    // A stand-in, such as scripts/screenshots.sh's, so a made-up world
    // never changes this Mac's real tailscale serve.
    if let Some(p) = std::env::var_os("REX_INBOX_TAILSCALE") {
        return Some(p.into());
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let p = dir.join("tailscale");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let app = std::path::PathBuf::from("/Applications/Tailscale.app/Contents/MacOS/Tailscale");
    app.is_file().then_some(app)
}

/// `tailscale status --json`, if Tailscale is installed and running.
pub async fn tailscale_status() -> Option<Value> {
    let out = tokio::process::Command::new(tailscale_bin()?).args(["status", "--json"]).output().await.ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// What `rex-inbox web` checks before it sends you to the phone: each is
/// None when Tailscale didn't say.
pub struct Readiness {
    /// MagicDNS, so the phone can find this Mac by name.
    pub magic_dns: Option<bool>,
    /// HTTPS certificates, so `tailscale serve` can answer on https.
    pub https: Option<bool>,
    /// `tailscale serve` proxies to this port.
    pub served: Option<bool>,
}

/// Ask Tailscale (from the CLI, without the hub) what's still missing.
pub fn readiness(port: &str) -> Readiness {
    let run = |args: &[&str]| -> Option<Value> {
        let out = std::process::Command::new(tailscale_bin()?).args(args).output().ok()?;
        out.status.success().then(|| serde_json::from_slice(&out.stdout).ok()).flatten()
    };
    readiness_of(run(&["status", "--json"]).as_ref(), run(&["serve", "status", "--json"]).as_ref(), port)
}

/// Readiness from `tailscale status --json` and `tailscale serve status --json`.
fn readiness_of(status: Option<&Value>, serve: Option<&Value>, port: &str) -> Readiness {
    let target = format!(":{port}");
    let proxies_here = |site: &Value| {
        site["Handlers"].as_object().is_some_and(|hs| {
            hs.values().any(|h| h["Proxy"].as_str().is_some_and(|p| p.trim_end_matches('/').ends_with(&target)))
        })
    };
    Readiness {
        magic_dns: status.and_then(|s| s["CurrentTailnet"]["MagicDNSEnabled"].as_bool()),
        https: status.map(|s| s["CertDomains"].as_array().is_some_and(|d| !d.is_empty())),
        served: serve.map(|s| s["Web"].as_object().is_some_and(|sites| sites.values().any(proxies_here))),
    }
}

/// This Mac's Tailscale user (`REX_INBOX_OWNER` overrides), and its HTTPS
/// name on the tailnet.
pub async fn owner_and_url() -> (Option<String>, Option<String>) {
    let status = tailscale_status().await;
    let owner = std::env::var("REX_INBOX_OWNER").ok().or_else(|| {
        let s = status.as_ref()?;
        let uid = s["Self"]["UserID"].as_i64()?;
        s["User"][uid.to_string()]["LoginName"].as_str().map(str::to_string)
    });
    let url = status
        .as_ref()
        .and_then(|s| s["Self"]["DNSName"].as_str())
        .map(|d| format!("https://{}", d.trim_end_matches('.')));
    (owner, url)
}

/// Keep the owner current: Tailscale may start after the hub.
pub async fn watch_owner(shared: Shared) {
    loop {
        let (owner, url) = owner_and_url().await;
        {
            let mut core = shared.lock();
            if core.owner != owner {
                if let Some(o) = &owner {
                    say!("web app owner: {o}");
                }
                core.owner = owner;
            }
            core.web_url = url;
        }
        tokio::time::sleep(Duration::from_secs(120)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{host_ok, readiness_of};
    use serde_json::json;

    #[test]
    fn readiness_reads_tailscale() {
        // A new tailnet: MagicDNS and certificates off, nothing served.
        let off = json!({"CurrentTailnet": {"MagicDNSEnabled": false}, "CertDomains": null});
        let r = readiness_of(Some(&off), Some(&json!({})), "7430");
        assert_eq!((r.magic_dns, r.https, r.served), (Some(false), Some(false), Some(false)));

        let on = json!({"CurrentTailnet": {"MagicDNSEnabled": true}, "CertDomains": ["mac.tail1.ts.net"]});
        let serve = json!({"TCP": {"443": {"HTTPS": true}},
            "Web": {"mac.tail1.ts.net:443": {"Handlers": {"/": {"Proxy": "http://127.0.0.1:7430"}}}}});
        let r = readiness_of(Some(&on), Some(&serve), "7430");
        assert_eq!((r.magic_dns, r.https, r.served), (Some(true), Some(true), Some(true)));

        // Serving something else, on another port.
        assert_eq!(readiness_of(Some(&on), Some(&serve), "7431").served, Some(false));
        // Tailscale didn't answer: say nothing.
        let r = readiness_of(None, None, "7430");
        assert_eq!((r.magic_dns, r.https, r.served), (None, None, None));
    }
    use axum::http::{HeaderMap, HeaderValue, header};

    fn host(h: &str) -> HeaderMap {
        let mut m = HeaderMap::new();
        m.insert(header::HOST, HeaderValue::from_str(h).unwrap());
        m
    }

    #[test]
    fn hosts() {
        let url = Some("https://mac.tail1234.ts.net");
        assert!(host_ok(&host("127.0.0.1:7430"), None));
        assert!(host_ok(&host("localhost:7430"), None));
        assert!(host_ok(&host("mac.tail1234.ts.net"), url));
        assert!(host_ok(&host("MAC.tail1234.ts.net:443"), url));
        assert!(!host_ok(&host("evil.example:7430"), url));
        assert!(!host_ok(&host("mac.tail1234.ts.net.evil.example"), url));
        assert!(!host_ok(&host("mac.tail1234.ts.net"), None));
        assert!(!host_ok(&HeaderMap::new(), url));
    }
}
