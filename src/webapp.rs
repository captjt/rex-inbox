//! The phone web app, built into the binary from web/.

use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

struct Asset {
    path: &'static str,
    mime: &'static str,
    body: &'static [u8],
}

const ASSETS: &[Asset] = &[
    Asset { path: "/", mime: "text/html; charset=utf-8", body: include_bytes!("../web/index.html") },
    Asset { path: "/app.js", mime: "text/javascript; charset=utf-8", body: include_bytes!("../web/app.js") },
    Asset { path: "/app.css", mime: "text/css; charset=utf-8", body: include_bytes!("../web/app.css") },
    Asset { path: "/sw.js", mime: "text/javascript; charset=utf-8", body: include_bytes!("../web/sw.js") },
    Asset { path: "/manifest.webmanifest", mime: "application/manifest+json", body: include_bytes!("../web/manifest.webmanifest") },
    Asset { path: "/icon-180.png", mime: "image/png", body: include_bytes!("../web/icon-180.png") },
    Asset { path: "/icon-512.png", mime: "image/png", body: include_bytes!("../web/icon-512.png") },
];

pub async fn asset(uri: Uri) -> Response {
    let path = if uri.path() == "/index.html" { "/" } else { uri.path() };
    match ASSETS.iter().find(|a| a.path == path) {
        Some(a) => (
            [(header::CONTENT_TYPE, a.mime), (header::CACHE_CONTROL, "no-cache")],
            a.body,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
