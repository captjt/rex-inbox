//! Guarded approve and deny for permission prompts.
//!
//! Keys go to a pane only if, read from Rex right now, the prompt is the
//! one the alert showed: the same record, still `blocked` on `permission`,
//! with the same message and the same report time, and it is the only
//! prompt waiting in that pane. Otherwise nothing is sent.
//!
//! A review rex-reviewer has ready to post is answered the same way, after
//! the same check, with its actions instead of keys (see reviewer.rs).

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
    if let Some(seen) = seen
        && alert.msg.as_deref() != Some(seen)
    {
        bail!("#{id} changed since you saw it; nothing was sent. Look again with: rex-inbox show {id}");
    }
    let app = alert.app.clone().unwrap_or_default();
    let review = crate::reviewer::is_item(alert.app.as_deref(), &alert.record_id);
    let keys = crate::agents::prompt_keys(&app);
    if keys.is_none() && !review {
        bail!("rex-inbox doesn't know how to answer {app}'s prompts yet; open it with: rex-inbox open {id}");
    }

    // The guard: read the prompt from Rex now, not from the hub's mirror.
    let records = live_records(&rex, &alert.session_id, &alert.block_id).await?;
    let now = records.iter().find(|r| r.id == alert.record_id);
    let same = now.is_some_and(|r| {
        r.state == "blocked" && r.kind.as_deref() == Some("permission") && r.msg == alert.msg && r.updated_at == alert.record_updated_at
    });
    if !same {
        bail!("#{id}'s prompt has changed: it is now {}. Nothing was sent.", describe(now));
    }
    if review {
        return answer_review(shared, &rex, &alert, approve).await;
    }
    let Some((yes, no)) = keys else { unreachable!("checked above") };
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

/// Post or dismiss a review rex-reviewer has ready. rex-reviewer checks the
/// round is still the one the item names. The item stays until the watcher
/// clears its record, so it's set aside here at once.
async fn answer_review(shared: &Shared, rex: &crate::rex::Rex, alert: &crate::store::Alert, post: bool) -> Result<Value> {
    let id = alert.id;
    let (verb, action) = if post { ("approve", "reviewer_post") } else { ("deny", "reviewer_dismiss") };
    let result = crate::reviewer::act(rex, action, &alert.record_id).await?;
    let summary = if post {
        match result["url"].as_str() {
            Some(url) => format!("Posted the review: {url}"),
            None => "Posted the review".to_string(),
        }
    } else {
        "Dismissed the review; the round ended without posting".to_string()
    };
    {
        let core = shared.lock();
        core.store.log(verb, &json!({ "id": id, "app": crate::reviewer::APP, "item": alert.record_id, "result": result }))?;
        if let Some(a) = core.store.set_alert_status(id, "dismissed", None)? {
            core.emit(crate::hub::HubEvent::AlertUpdated { alert: a });
        }
    }
    Ok(
        json!({ "id": id, "answer": verb, "cleared": true, "now": if post { "posted" } else { "dismissed" }, "summary": summary, "result": result }),
    )
}

/// A review's text as it would post, from rex-reviewer's preview action.
pub async fn preview(shared: &Shared, id: i64) -> Result<String> {
    let (rex, alert) = {
        let core = shared.lock();
        let a = core.store.alert(id)?.ok_or_else(|| anyhow!("no alert {id}"))?;
        (core.rex(&a.host)?, a)
    };
    if !crate::reviewer::is_item(alert.app.as_deref(), &alert.record_id) {
        bail!("#{id} has no preview; only reviews rex-reviewer has ready do");
    }
    let v = crate::reviewer::act(&rex, "reviewer_preview", &alert.record_id).await?;
    v["preview"].as_str().map(str::to_string).ok_or_else(|| anyhow!("rex-reviewer's preview came back empty"))
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
