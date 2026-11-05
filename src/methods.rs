//! The hub's methods.

use anyhow::{Result, anyhow, bail};
use serde::Serialize;
use serde_json::{Value, json};

use crate::hub::Shared;
use crate::mirror::{Pane, urgency};

#[derive(Debug, Serialize)]
pub struct PaneView {
    pub host: String,
    pub session_id: String,
    pub session_label: Option<String>,
    pub window_id: Option<String>,
    pub window_label: Option<String>,
    pub block_id: String,
    pub label: String,
    pub app: Option<String>,
    pub state: Option<String>,
    pub kind: Option<String>,
    pub msg: Option<String>,
    pub title: Option<String>,
    pub since: Option<String>,
    pub cwd: Option<String>,
    pub process: Option<String>,
    /// Child records (subagents, tasks) that are working.
    pub working_children: usize,
}

fn pane_view(core: &crate::hub::Core, p: &Pane) -> PaneView {
    let m = core.hosts.get(&p.host);
    let head = p.headline();
    PaneView {
        host: p.host.clone(),
        session_id: p.session_id.clone(),
        session_label: m.and_then(|m| m.session_label(&p.session_id)).map(str::to_string),
        window_id: p.window_id.clone(),
        window_label: m.and_then(|m| m.window_label(&p.session_id, p.window_id.as_deref())).map(str::to_string),
        block_id: p.block_id.clone(),
        label: p.label.clone(),
        app: p.app().map(str::to_string),
        state: head.map(|r| r.state.clone()),
        kind: head.and_then(|r| r.kind.clone()),
        msg: head.and_then(|r| r.msg.clone()),
        title: head.and_then(|r| r.title.clone()),
        since: head.and_then(|r| r.updated_at.clone()),
        cwd: p.cwd.clone(),
        process: p.process.clone(),
        working_children: p.records.values().filter(|r| !r.is_root() && r.state == "working").count(),
    }
}

fn id(params: &Value) -> Result<i64> {
    params["id"].as_i64().ok_or_else(|| anyhow!("id is required"))
}

pub async fn dispatch(shared: &Shared, method: &str, params: Value) -> Result<Value> {
    match method {
        "status" => {
            let core = shared.lock();
            let hosts: Vec<Value> = core
                .rexes
                .keys()
                .map(|h| {
                    let m = core.hosts.get(h);
                    json!({
                        "host": h,
                        "connected": m.is_some_and(|m| m.connected),
                        "sessions": m.map_or(0, |m| m.sessions.len()),
                        "panes": m.map_or(0, |m| m.panes.len()),
                        "agents": m.map_or(0, |m| m.panes.values().filter(|p| p.is_agent()).count()),
                    })
                })
                .collect();
            let open = core.store.list_alerts(false, false, 1000)?.len();
            Ok(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "started_at": core.started_at,
                "pid": std::process::id(),
                "hosts": hosts,
                "open_alerts": open,
                "notifiers": core.notifiers,
            }))
        }

        "alerts.list" => {
            let core = shared.lock();
            let all = params["all"].as_bool().unwrap_or(false);
            let history = params["history"].as_bool().unwrap_or(false);
            let limit = params["limit"].as_i64().unwrap_or(200);
            Ok(serde_json::to_value(core.store.list_alerts(all, history, limit)?)?)
        }

        "inbox" => {
            let all = params["all"].as_bool().unwrap_or(false);
            let history = params["history"].as_bool().unwrap_or(false);
            let core = shared.lock();
            let alerts = core.store.list_alerts(all || history, history, 200)?;
            let messages = core.store.messages_to_me(history)?;
            Ok(json!({ "alerts": alerts, "messages": messages }))
        }

        "show" => {
            let id = id(&params)?;
            let core = shared.lock();
            if let Some(a) = core.store.alert(id)? {
                return Ok(json!({ "type": "alert", "alert": a }));
            }
            if let Some(m) = core.store.message(id)? {
                let thread = core.store.thread(m.thread_id)?;
                return Ok(json!({ "type": "message", "message": m, "thread": thread }));
            }
            bail!("nothing numbered {id}")
        }

        // Done with an item: dismiss an alert, file a message to you, or
        // cancel a message still waiting for its pane.
        "done" => {
            let id = id(&params)?;
            let is_alert = shared.lock().store.alert(id)?.is_some();
            let method = if is_alert { "alerts.dismiss" } else { "messages.done" };
            Box::pin(dispatch(shared, method, params)).await
        }

        "alerts.dismiss" | "alerts.reopen" | "alerts.snooze" => {
            let id = id(&params)?;
            let core = shared.lock();
            let (status, until) = match method {
                "alerts.dismiss" => ("dismissed", None),
                "alerts.reopen" => ("open", None),
                _ => {
                    let d = params["for"].as_str().ok_or_else(|| anyhow!("for is required, such as 1h"))?;
                    let d = crate::time::parse_duration(d).ok_or_else(|| anyhow!("bad duration {d}; use 30m, 2h, 1d"))?;
                    ("snoozed", Some(crate::time::after(d)))
                }
            };
            let a = core.store.set_alert_status(id, status, until.as_deref())?.ok_or_else(|| anyhow!("no alert {id}"))?;
            if a.status == "resolved" {
                bail!("alert {id} is already resolved");
            }
            core.store.log(method, &json!({ "id": id }))?;
            Ok(serde_json::to_value(a)?)
        }

        "panes.list" => {
            let all = params["all"].as_bool().unwrap_or(false);
            let core = shared.lock();
            let mut v: Vec<PaneView> = core
                .hosts
                .values()
                .flat_map(|m| m.panes.values())
                .filter(|p| all || p.is_agent())
                .map(|p| pane_view(&core, p))
                .collect();
            v.sort_by(|a, b| {
                let u = |p: &PaneView| urgency(p.state.as_deref().unwrap_or(""));
                u(b).cmp(&u(a)).then_with(|| a.session_label.cmp(&b.session_label)).then_with(|| a.label.cmp(&b.label))
            });
            Ok(serde_json::to_value(v)?)
        }

        "open" => {
            let id = id(&params)?;
            let (rex, session, block, window) = {
                let core = shared.lock();
                let (host, session, block, window) = if let Some(a) = core.store.alert(id)? {
                    (a.host, a.session_id, a.block_id, a.window_id)
                } else if let Some(m) = core.store.message(id)? {
                    // The agent's pane: the sender's, or the recipient's for a message from you.
                    match (m.from_host, m.from_session, m.from_block, m.to_host, m.to_session, m.to_block) {
                        (Some(h), Some(s), Some(b), ..) | (.., Some(h), Some(s), Some(b)) => (h, s, b, None),
                        _ => bail!("message {id} has no pane"),
                    }
                } else {
                    bail!("nothing numbered {id}")
                };
                let window = core.pane(&host, &block).and_then(|p| p.window_id.clone()).or(window);
                if core.pane(&host, &block).is_none() {
                    bail!("that pane is gone");
                }
                (core.rex(&host)?, session, block, window)
            };
            rex.focus(&session, &block, window.as_deref()).await?;
            Ok(json!({ "session_id": session, "block_id": block }))
        }

        "panes.open" => {
            let host = params["host"].as_str().unwrap_or("local").to_string();
            let block = params["block_id"].as_str().ok_or_else(|| anyhow!("block_id is required"))?.to_string();
            let (rex, session, window) = {
                let core = shared.lock();
                let p = core.pane(&host, &block).ok_or_else(|| anyhow!("that pane is gone"))?;
                (core.rex(&host)?, p.session_id.clone(), p.window_id.clone())
            };
            rex.focus(&session, &block, window.as_deref()).await?;
            Ok(json!({ "session_id": session, "block_id": block }))
        }

        "approve" | "deny" => crate::approve::answer(shared, id(&params)?, method == "approve", params["seen"].as_str()).await,

        "screen" => {
            let lines = params["lines"].as_u64().unwrap_or(30) as usize;
            Ok(json!({ "text": crate::approve::screen(shared, id(&params)?, lines).await? }))
        }

        m if m.starts_with("messages.") || m == "whoami" || m == "name" => crate::messages::dispatch(shared, m, &params).await,

        other => bail!("unknown method {other}"),
    }
}
