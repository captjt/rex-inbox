//! Previews: an app an agent serves on this Mac, published on the tailnet
//! for you to look at and answer, from the Mac or the phone.
//!
//! `rex-inbox share 3000` puts the app on its own HTTPS port of the Mac's
//! tailnet name (`tailscale serve --https=7431 http://127.0.0.1:3000`), so
//! it's at `/` and its assets, cookies and live reload work as they do on
//! localhost. The preview rides on a message to you: the agent's question,
//! and the answers it offers. Your answer goes back as a reply.
//!
//! A preview stays up until its message is filed, the agent's pane closes,
//! nothing answers on the port any more, or the agent stops it.

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::time::Duration;

use crate::hub::{HubEvent, Shared};
use crate::messages::Caller;
use crate::store::Message;

/// The Mac's HTTPS ports previews take, one each.
const PORTS: std::ops::RangeInclusive<u16> = 7431..=7499;
/// How often the hub looks at what previews still need serving.
const SWEEP: Duration = Duration::from_secs(5);
/// Sweeps in a row the app must not answer in before its preview comes down.
const MISSES: u32 = 3;

/// The first port in `PORTS` no live preview has.
fn free_port(taken: &[u16]) -> Option<u16> {
    PORTS.clone().find(|p| !taken.contains(p))
}

/// The preview's address: the inbox's tailnet name, on its own port.
fn url_for(web_url: &str, https_port: u16) -> Result<String> {
    let base = web_url.trim_end_matches('/');
    let host = base.strip_prefix("https://").ok_or_else(|| anyhow!("the inbox isn't on https ({base})"))?;
    let host = host.split(['/', ':']).next().unwrap_or(host);
    Ok(format!("https://{host}:{https_port}/"))
}

fn https_port(m: &Message) -> Option<u16> {
    m.preview.as_ref()?["https_port"].as_u64().map(|p| p as u16)
}

async fn answers(port: u16) -> bool {
    tokio::time::timeout(Duration::from_secs(1), tokio::net::TcpStream::connect(("127.0.0.1", port)))
        .await
        .is_ok_and(|r| r.is_ok())
}

async fn tailscale(args: &[&str]) -> Result<()> {
    let bin = crate::web::tailscale_bin().ok_or_else(|| anyhow!("Tailscale isn't installed on this Mac"))?;
    let out = tokio::process::Command::new(bin).args(args).output().await?;
    if !out.status.success() {
        bail!("tailscale {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// Share an app the caller's pane serves on `port`. Params: `port`,
/// `title`, `ask`, `options` (the answers to offer, as buttons).
pub async fn share(shared: &Shared, caller: &Caller, params: &Value) -> Result<Value> {
    let port = params["port"].as_u64().filter(|p| (1..=65535).contains(p)).ok_or_else(|| anyhow!("a port is required"))? as u16;
    let title = params["title"].as_str().map(str::trim).filter(|s| !s.is_empty());
    let ask = params["ask"].as_str().map(str::trim).filter(|s| !s.is_empty());
    let options: Vec<String> = params["options"]
        .as_array()
        .map(|a| a.iter().filter_map(|o| o.as_str()).map(str::trim).filter(|o| !o.is_empty()).map(str::to_string).collect())
        .unwrap_or_default();
    // A preview asks for approval or comments: Approve, and Reply for the rest.
    let options = if options.is_empty() { vec!["Approve".to_string()] } else { options };

    let (from, web_url, replaced) = {
        let core = shared.lock();
        let from = caller.end(&core);
        if from.block.is_none() {
            bail!("run this in an agent's pane: a preview is the agent asking you about something it serves");
        }
        if from.host.as_deref() != Some("local") {
            bail!("previews from other machines aren't in yet; serve it on this Mac for now");
        }
        let web_url = core.web_url.clone().ok_or_else(|| {
            anyhow!("the inbox isn't on your tailnet yet, so there's nowhere to publish it (see rex-inbox web)")
        })?;
        // Sharing the same port again replaces the last one, on its address.
        let replaced = core.store.live_previews()?.into_iter().find(|m| {
            m.from_block == from.block && m.preview.as_ref().and_then(|p| p["port"].as_u64()) == Some(port as u64)
        });
        (from, web_url, replaced)
    };
    if !answers(port).await {
        bail!("nothing answers on port {port} on this Mac; start the app first");
    }

    let https = match replaced.as_ref().and_then(https_port) {
        Some(p) => p,
        None => {
            let taken: Vec<u16> = shared.lock().store.live_previews()?.iter().filter_map(https_port).collect();
            free_port(&taken).ok_or_else(|| anyhow!("every preview port ({}–{}) is taken; stop one first", PORTS.start(), PORTS.end()))?
        }
    };
    let url = url_for(&web_url, https)?;
    let target = format!("http://127.0.0.1:{port}");
    tailscale(&["serve", "--bg", &format!("--https={https}"), &target]).await?;

    let core = shared.lock();
    if let Some(old) = replaced {
        stopped(&core, &old, "shared again")?;
        let filed = core.store.mark_done(old.id)?;
        crate::messages::emit_updated(&core, filed);
    }
    let subject = title.map(str::to_string).unwrap_or_else(|| format!("Preview on port {port}"));
    let body = ask.unwrap_or("Take a look, and tell me what you think.");
    let m = core.store.insert_message(&from, &crate::address::me(), Some(&subject), body, None)?;
    let preview = json!({ "url": url, "port": port, "https_port": https, "options": options, "state": "live" });
    let m = core.store.set_preview(m.id, &preview)?.expect("inserted message");
    core.store.log("preview", &json!({ "id": m.id, "from": m.from_addr, "port": port, "url": url }))?;
    core.emit(HubEvent::MessageNew { message: m.clone() });
    Ok(json!({ "id": m.id, "url": url, "message": m }))
}

/// Stop the caller's preview of `port`, or every one it has.
pub async fn stop(shared: &Shared, caller: &Caller, params: &Value) -> Result<Value> {
    let port = params["port"].as_u64();
    let mine: Vec<Message> = {
        let core = shared.lock();
        let from = caller.end(&core);
        if from.block.is_none() {
            bail!("run this in the agent's pane that shared it");
        }
        core.store
            .live_previews()?
            .into_iter()
            .filter(|m| m.from_block == from.block)
            .filter(|m| port.is_none() || m.preview.as_ref().and_then(|p| p["port"].as_u64()) == port)
            .collect()
    };
    if mine.is_empty() {
        bail!("this pane has no preview{} up", port.map(|p| format!(" of port {p}")).unwrap_or_default());
    }
    for m in &mine {
        take_down(shared, m, "the agent stopped it").await;
    }
    Ok(json!({ "stopped": mine.iter().map(|m| m.id).collect::<Vec<_>>() }))
}

/// Turn its port off on the tailnet, and mark it stopped.
async fn take_down(shared: &Shared, m: &Message, why: &str) {
    if let Some(p) = https_port(m)
        && let Err(e) = tailscale(&["serve", &format!("--https={p}"), "off"]).await
    {
        say!("preview #{}: {e:#}", m.id);
    }
    let core = shared.lock();
    if let Err(e) = stopped(&core, m, why) {
        say!("preview #{}: {e:#}", m.id);
    }
}

fn stopped(core: &crate::hub::Core, m: &Message, why: &str) -> Result<()> {
    let mut p = m.preview.clone().unwrap_or_else(|| json!({}));
    p["state"] = json!("stopped");
    p["stopped"] = json!(why);
    let fresh = core.store.set_preview(m.id, &p)?;
    crate::messages::emit_updated(core, fresh);
    Ok(())
}

/// Why a live preview should come down now, if it should.
fn why_down(core: &crate::hub::Core, m: &Message, up: bool) -> Option<&'static str> {
    if m.status == "done" {
        return Some("filed");
    }
    let pane = m.from_host.as_deref().zip(m.from_block.as_deref()).and_then(|(h, b)| core.pane(h, b));
    // A host that's out of reach says nothing about its panes.
    let followed = m.from_host.as_deref().is_some_and(|h| core.hosts.get(h).is_some_and(|x| x.connected));
    if followed && pane.is_none() {
        return Some("the agent's pane closed");
    }
    if !up {
        return Some("nothing answers on its port any more");
    }
    None
}

/// Take previews down when they're no longer wanted.
pub async fn run(shared: Shared) {
    let mut misses: std::collections::HashMap<i64, u32> = Default::default();
    loop {
        tokio::time::sleep(SWEEP).await;
        let live = match shared.lock().store.live_previews() {
            Ok(v) => v,
            Err(e) => {
                say!("previews: {e:#}");
                continue;
            }
        };
        misses.retain(|id, _| live.iter().any(|m| m.id == *id));
        for m in live {
            let port = m.preview.as_ref().and_then(|p| p["port"].as_u64()).unwrap_or(0) as u16;
            // An app restarting (a dev server reloading) is down for a
            // moment, so it takes three misses in a row to count.
            let up = if answers(port).await {
                misses.remove(&m.id);
                true
            } else {
                let n = misses.entry(m.id).or_default();
                *n += 1;
                *n < MISSES
            };
            let why = why_down(&shared.lock(), &m, up);
            if let Some(why) = why {
                take_down(&shared, &m, why).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_is_the_inbox_on_its_own_port() {
        let u = url_for("https://mac.tail1.ts.net", 7431).unwrap();
        assert_eq!(u, "https://mac.tail1.ts.net:7431/");
        assert_eq!(url_for("https://mac.tail1.ts.net/", 7432).unwrap(), "https://mac.tail1.ts.net:7432/");
        assert!(url_for("http://127.0.0.1:7430", 7431).is_err());
    }

    #[test]
    fn each_live_preview_has_a_port_of_its_own() {
        assert_eq!(free_port(&[]), Some(7431));
        assert_eq!(free_port(&[7431, 7432]), Some(7433));
        let all: Vec<u16> = PORTS.collect();
        assert_eq!(free_port(&all), None);
    }
}
