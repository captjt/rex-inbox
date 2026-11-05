//! Guarded approve and deny for permission prompts.
//!
//! Keys go to a pane only if, read from Rex right now, the prompt is the
//! one the alert showed: the same record, still `blocked` on `permission`,
//! with the same message and the same report time, and it is the only
//! prompt waiting in that pane. Otherwise nothing is sent.

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::time::Duration;

use crate::hub::Shared;
use crate::mirror::Record;

async fn live_records(rex: &crate::rex::Rex, session: &str, block: &str) -> Result<Vec<Record>> {
    let v = rex.block_call(session, block, "program_status", &json!({})).await?;
    Ok(v["records"].as_array().map(|a| a.iter().filter_map(|r| serde_json::from_value(r.clone()).ok()).collect()).unwrap_or_default())
}

fn describe(r: Option<&Record>) -> String {
    match r {
        None => "gone".into(),
        Some(r) => {
            let mut s = r.state.clone();
            if let Some(k) = &r.kind {
                s.push_str(&format!(" ({k})"));
            }
            if let Some(m) = &r.msg {
                s.push_str(&format!(": {m}"));
            }
            s
        }
    }
}

/// `approve: true` approves, `false` denies. `seen` is the prompt text the
/// person was shown, when the client has it.
pub async fn answer(shared: &Shared, id: i64, approve: bool, seen: Option<&str>) -> Result<Value> {
    let verb = if approve { "approve" } else { "deny" };
    let (rex, alert) = {
        let core = shared.lock();
        let a = core.store.alert(id)?.ok_or_else(|| anyhow!("no alert {id}"))?;
        (core.rex(&a.host)?, a)
    };
    if alert.status == "resolved" {
        bail!("#{id} is already resolved ({}); nothing was sent", alert.resolved_reason.as_deref().unwrap_or("its status changed"));
    }
    if alert.category != "needs_you" || alert.kind.as_deref() != Some("permission") {
        bail!("#{id} isn't a permission prompt; open it with: rex-inbox open {id}");
    }
    if let Some(seen) = seen {
        if alert.msg.as_deref() != Some(seen) {
            bail!("#{id} changed since you saw it; nothing was sent. Look again with: rex-inbox show {id}");
        }
    }
    let app = alert.app.clone().unwrap_or_default();
    let Some((yes, no)) = crate::agents::prompt_keys(&app) else {
        bail!("rex-inbox doesn't know how to answer {app}'s prompts yet; open it with: rex-inbox open {id}");
    };

    // The guard: read the prompt from Rex now, not from the hub's mirror.
    let records = live_records(&rex, &alert.session_id, &alert.block_id).await?;
    let now = records.iter().find(|r| r.id == alert.record_id);
    let same = now.is_some_and(|r| {
        r.state == "blocked"
            && r.kind.as_deref() == Some("permission")
            && r.msg == alert.msg
            && r.updated_at == alert.record_updated_at
    });
    if !same {
        bail!("#{id}'s prompt has changed: it is now {}. Nothing was sent.", describe(now));
    }
    let waiting = records.iter().filter(|r| r.state == "blocked").count();
    if waiting > 1 {
        bail!("{waiting} prompts are waiting in that pane, so a key could answer the wrong one. Open it with: rex-inbox open {id}");
    }

    let keys = if approve { yes } else { no };
    rex.send_keys(&alert.session_id, &alert.block_id, keys).await?;
    shared.lock().store.log(verb, &json!({ "id": id, "app": app, "msg": alert.msg, "keys": keys }))?;

    // Report whether the prompt went away.
    let mut after = None;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let records = live_records(&rex, &alert.session_id, &alert.block_id).await?;
        let r = records.into_iter().find(|r| r.id == alert.record_id);
        let moved = r.as_ref().is_none_or(|r| r.state != "blocked" || r.updated_at != alert.record_updated_at);
        if moved {
            after = Some(r);
            break;
        }
    }
    Ok(match after {
        Some(r) => json!({ "id": id, "answer": verb, "cleared": true, "now": describe(r.as_ref()) }),
        None => json!({ "id": id, "answer": verb, "cleared": false, "now": "still blocked after 5s" }),
    })
}

/// The bottom of the alert's pane, so you can see the prompt itself.
pub async fn screen(shared: &Shared, id: i64, lines: usize) -> Result<String> {
    let (rex, session, block) = {
        let core = shared.lock();
        let a = core.store.alert(id)?.ok_or_else(|| anyhow!("no alert {id}"))?;
        (core.rex(&a.host)?, a.session_id, a.block_id)
    };
    rex.capture(&session, &block, lines).await
}
