//! Web Push to the phone: the web app, added to the Home Screen, subscribes
//! with the hub's VAPID key, and the hub sends what needs you straight to the
//! browser's push service. Payloads are end-to-end encrypted (RFC 8291), so
//! the push service carries them without reading them. No developer account.

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use rusqlite::params;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use web_push_native::jwt_simple::algorithms::{ECDSAP256PublicKeyLike, ES256KeyPair};
use web_push_native::p256::PublicKey;
use web_push_native::{Auth, WebPushBuilder};

use crate::hub::{HubEvent, Shared};
use crate::store::Store;

const GRACE: Duration = Duration::from_secs(2);

/// The hub's VAPID key pair, made on first use and kept in the state dir.
pub fn vapid() -> Result<ES256KeyPair> {
    let path = crate::paths::state_dir().join("vapid.key");
    if let Ok(text) = std::fs::read_to_string(&path) {
        let raw = B64.decode(text.trim()).context("reading vapid.key")?;
        return ES256KeyPair::from_bytes(&raw).map_err(|e| anyhow!("vapid.key: {e}"));
    }
    let kp = ES256KeyPair::generate();
    std::fs::create_dir_all(crate::paths::state_dir())?;
    std::fs::write(&path, B64.encode(kp.to_bytes()))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok(kp)
}

fn public_key(kp: &ES256KeyPair) -> String {
    B64.encode(kp.public_key().public_key().to_bytes_uncompressed())
}

impl Store {
    pub fn push_subscribe(&self, endpoint: &str, p256dh: &str, auth: &str, agent: Option<&str>) -> Result<()> {
        self.conn_ref().execute(
            "INSERT INTO push_subs (endpoint, p256dh, auth, agent, created_at) VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(endpoint) DO UPDATE SET p256dh = ?2, auth = ?3, agent = ?4, failures = 0",
            params![endpoint, p256dh, auth, agent, crate::time::now()],
        )?;
        Ok(())
    }

    pub fn push_unsubscribe(&self, endpoint: &str) -> Result<()> {
        self.conn_ref().execute("DELETE FROM push_subs WHERE endpoint = ?1", [endpoint])?;
        Ok(())
    }

    /// `(endpoint, p256dh, auth)` for every subscribed device.
    pub fn push_subs(&self) -> Result<Vec<(String, String, String)>> {
        let mut st = self.conn_ref().prepare("SELECT endpoint, p256dh, auth FROM push_subs")?;
        Ok(st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?)
    }

    fn push_result(&self, endpoint: &str, ok: bool) -> Result<()> {
        let sql = if ok {
            "UPDATE push_subs SET failures = 0, last_ok = ?2 WHERE endpoint = ?1"
        } else {
            "UPDATE push_subs SET failures = failures + 1 WHERE endpoint = ?1 AND ?2 IS NOT NULL"
        };
        self.conn_ref().execute(sql, params![endpoint, crate::time::now()])?;
        Ok(())
    }
}

pub async fn dispatch(shared: &Shared, method: &str, params: &Value) -> Result<Value> {
    match method {
        "push.key" => Ok(json!({ "public_key": public_key(&vapid()?) })),
        "push.subscribe" => {
            let sub = &params["subscription"];
            let endpoint = sub["endpoint"].as_str().ok_or_else(|| anyhow!("subscription.endpoint is required"))?;
            let p256dh = sub["keys"]["p256dh"].as_str().ok_or_else(|| anyhow!("subscription.keys.p256dh is required"))?;
            let auth = sub["keys"]["auth"].as_str().ok_or_else(|| anyhow!("subscription.keys.auth is required"))?;
            if !endpoint.starts_with("https://") {
                bail!("push endpoints are https URLs");
            }
            let core = shared.lock();
            core.store.push_subscribe(endpoint, p256dh, auth, params["agent"].as_str())?;
            core.store.log("push_subscribe", &json!({ "endpoint": endpoint }))?;
            Ok(json!({ "subscribed": true }))
        }
        "push.unsubscribe" => {
            let endpoint = params["endpoint"].as_str().ok_or_else(|| anyhow!("endpoint is required"))?;
            shared.lock().store.push_unsubscribe(endpoint)?;
            Ok(json!({ "subscribed": false }))
        }
        "push.test" => {
            let note = json!({ "title": "Rex Inbox", "body": "Notifications work.", "tag": "test", "url": "/" });
            let sent = send_all(shared, &note).await?;
            Ok(json!({ "sent": sent }))
        }
        other => bail!("unknown method {other}"),
    }
}

/// Notify every subscribed device of what needs you.
pub async fn run(shared: Shared) {
    let mut rx = shared.lock().events.subscribe();
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(RecvError::Lagged(_)) => continue,
            Err(RecvError::Closed) => return,
        };
        let note = match ev {
            HubEvent::AlertOpened { alert } if alert.category == "needs_you" || alert.category == "failed" => {
                // Give it a moment: a prompt answered at once never pings the phone.
                tokio::time::sleep(GRACE).await;
                let open = shared.lock().store.alert(alert.id).ok().flatten().is_some_and(|a| a.status == "open");
                if !open {
                    continue;
                }
                let app = alert.app.as_deref().map(crate::agents::display).unwrap_or("A program").to_string();
                let title = match (alert.category.as_str(), alert.kind.as_deref()) {
                    ("failed", _) => format!("{app} failed"),
                    (_, Some("permission")) => format!("{app} needs your approval"),
                    (_, Some("question")) => format!("{app} has a question"),
                    (_, Some("auth")) => format!("{app} needs you to sign in"),
                    (_, Some("startup")) => format!("{app} is waiting to start"),
                    _ => format!("{app} needs you"),
                };
                let place = crate::show::place(&serde_json::to_value(&alert).unwrap_or_default());
                let what = [alert.title.as_deref(), alert.msg.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(": ");
                json!({
                    "title": title,
                    "body": if what.is_empty() { place.clone() } else { format!("{place}\n{what}") },
                    "tag": format!("item-{}", alert.id),
                    "url": format!("/#item-{}", alert.id),
                })
            }
            HubEvent::MessageNew { message } if message.to_me() => {
                let first = message.body.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string();
                json!({
                    "title": message.headline(),
                    "body": message.subject.as_ref().map(|s| format!("{s}\n{first}")).unwrap_or(first),
                    "tag": format!("item-{}", message.id),
                    "url": format!("/#item-{}", message.id),
                })
            }
            _ => continue,
        };
        if let Err(e) = send_all(&shared, &note).await {
            say!("push: {e:#}");
        }
    }
}

async fn send_all(shared: &Shared, note: &Value) -> Result<usize> {
    let (subs, contact) = {
        let core = shared.lock();
        // RFC 8292 wants a contact; the web app's own URL serves.
        let contact = core.web_url.clone().unwrap_or_else(|| "https://localhost".into());
        (core.store.push_subs()?, contact)
    };
    if subs.is_empty() {
        return Ok(0);
    }
    let kp = vapid()?;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).build()?;
    let body = serde_json::to_vec(note)?;
    let mut sent = 0;
    for (endpoint, p256dh, auth) in subs {
        let outcome = async {
            let ua_public = PublicKey::from_sec1_bytes(&B64.decode(&p256dh)?)?;
            let ua_auth = Auth::clone_from_slice(&B64.decode(&auth)?);
            let request = WebPushBuilder::new(endpoint.parse()?, ua_public, ua_auth)
                .with_valid_duration(Duration::from_secs(3600))
                .with_vapid(&kp, &contact)
                .build(body.clone())?;
            let request: reqwest::Request = request.try_into()?;
            let resp = client.execute(request).await?;
            anyhow::Ok(resp.status())
        }
        .await;
        let core = shared.lock();
        match outcome {
            Ok(s) if s.is_success() => {
                sent += 1;
                core.store.push_result(&endpoint, true)?;
            }
            // The device unsubscribed or the subscription expired.
            Ok(s) if s.as_u16() == 404 || s.as_u16() == 410 => {
                say!("push: dropping a subscription the push service no longer knows ({s})");
                core.store.push_unsubscribe(&endpoint)?;
            }
            Ok(s) => {
                say!("push: {s} from the push service");
                core.store.push_result(&endpoint, false)?;
            }
            Err(e) => {
                say!("push: {e:#}");
                core.store.push_result(&endpoint, false)?;
            }
        }
    }
    Ok(sent)
}
