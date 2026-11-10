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

/// Which build of the web app this hub serves: a page that loaded another
/// one reloads itself (app.js), so a phone never runs an old app on a new hub.
pub fn build() -> &'static str {
    static BUILD: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BUILD.get_or_init(|| {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for a in ASSETS {
            a.path.hash(&mut h);
            a.body.hash(&mut h);
        }
        format!("{:016x}", h.finish())
    })
}

pub async fn asset(uri: Uri) -> Response {
    let path = if uri.path() == "/index.html" { "/" } else { uri.path() };
    let Some(a) = ASSETS.iter().find(|a| a.path == path) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    // REX_INBOX_WEB_DIR (scripts/screenshots.sh --serve) serves web/ from
    // disk, so a change shows on reload without a rebuild. Only the files
    // above, by their fixed names.
    let body = match std::env::var_os("REX_INBOX_WEB_DIR") {
        Some(dir) => {
            let file = if a.path == "/" { "index.html" } else { &a.path[1..] };
            match tokio::fs::read(std::path::Path::new(&dir).join(file)).await {
                Ok(bytes) => bytes.into(),
                Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("{file}: {e}")).into_response(),
            }
        }
        None => axum::body::Body::from(a.body),
    };
    ([(header::CONTENT_TYPE, a.mime), (header::CACHE_CONTROL, "no-cache")], body).into_response()
}
