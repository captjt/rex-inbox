//! Delivering queued messages into agent panes.
//!
//! A message waits until its pane runs a known agent whose root record has
//! been `idle` or `done` for a moment. Then the hub pastes it in and presses
//! Enter, the way `rex-agent prompt` does. A pane that isn't an agent never
//! gets typed into: text there would run as a shell command. Its messages
//! wait until it reads them with `rex-inbox inbox`.

use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;

use crate::hub::Shared;
use crate::messages::emit_updated;
use crate::rex::Rex;
use crate::store::Message;

/// How long a pane must sit idle before a message goes in, so a message
/// doesn't land on top of someone who just finished typing.
const SETTLE: Duration = Duration::from_secs(2);
/// Messages longer than this are announced, not pasted.
const MAX_PASTE: usize = 4000;
const MAX_ATTEMPTS: i64 = 5;

struct Due {
    msg: Message,
    rex: Rex,
    session: String,
    block: String,
    root_updated: Option<String>,
}

pub async fn run(shared: Shared) {
    let mut rx = shared.lock().events.subscribe();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    // The root record each pane had when it last got a message. Nothing
    // more goes in until that record changes (or a minute passes).
    let mut last: HashMap<String, (Option<String>, Instant)> = HashMap::new();
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            r = rx.recv() => if let Err(RecvError::Closed) = r { return },
        }
        for d in plan(&shared, &last) {
            let key = format!("{}|{}", d.msg.to_host.as_deref().unwrap_or(""), d.block);
            if deliver(&shared, &d).await {
                last.insert(key, (d.root_updated.clone(), Instant::now()));
            }
        }
    }
}

fn plan(shared: &Shared, last: &HashMap<String, (Option<String>, Instant)>) -> Vec<Due> {
    let core = shared.lock();
    let queued = match core.store.queued_messages() {
        Ok(q) => q,
        Err(e) => {
            say!("queued messages: {e:#}");
            return Vec::new();
        }
    };
    let mut seen = HashSet::new();
    let mut due = Vec::new();
    for m in queued {
        let (Some(host), Some(block)) = (m.to_host.clone(), m.to_block.clone()) else { continue };
        let key = format!("{host}|{block}");
        // Oldest first, one per pane per pass.
        if !seen.insert(key.clone()) {
            continue;
        }
        let Some(mirror) = core.hosts.get(&host) else { continue };
        if !mirror.connected {
            continue;
        }
        let Some(pane) = mirror.panes.get(&block) else {
            let r = core.store.mark_failed(m.id, "the pane closed before the message could be delivered");
            emit_updated(&core, r.ok().flatten());
            continue;
        };
        if !pane.is_agent() {
            continue;
        }
        let Some(root) = pane.root() else { continue };
        if root.state != "idle" && root.state != "done" {
            continue;
        }
        let settled = root
            .updated_at
            .as_deref()
            .and_then(crate::time::parse)
            .is_some_and(|t| (time::OffsetDateTime::now_utc() - t) >= SETTLE);
        if !settled {
            continue;
        }
        if let Some((upd, at)) = last.get(&key)
            && *upd == root.updated_at
            && at.elapsed() < Duration::from_secs(60)
        {
            continue;
        }
        let Ok(rex) = core.rex(&host) else { continue };
        due.push(Due { root_updated: root.updated_at.clone(), session: pane.session_id.clone(), block, rex, msg: m });
    }
    due
}

/// The text pasted into the agent's pane. An agent on another host can't
/// reach this hub with the CLI yet (its `rex-inbox` finds that machine's
/// hub, or none), so it gets the whole message and answers in its pane.
pub fn render(m: &Message) -> String {
    let remote = m.to_host.as_deref().is_some_and(|h| h != "local");
    let from = if m.from_block.is_none() { "the user (through rex-inbox)".to_string() } else { m.from_addr.clone() };
    let mut head = format!("[rex-inbox] Message #{} from {from}", m.id);
    if let Some(r) = m.reply_to {
        head.push_str(&format!(", replying to #{r}"));
    }
    if m.body.len() > MAX_PASTE && !remote {
        return format!(
            "{head}{}. It's long ({} lines): read it with `rex-inbox read {}`.",
            m.subject.as_ref().map(|s| format!(": {s}")).unwrap_or_default(),
            m.body.lines().count(),
            m.id
        );
    }
    let mut out = head;
    if let Some(s) = &m.subject {
        out.push_str(&format!("\nSubject: {s}"));
    }
    out.push_str("\n\n");
    out.push_str(m.body.trim_end());
    if remote {
        out.push_str(
            "\n\nThis pane is on another machine, where rex-inbox commands don't reach the sender. \
             Answer here, in your reply to this turn: they read it in this pane.",
        );
    } else {
        out.push_str(&format!(
            "\n\nReply with `rex-inbox reply {} \"...\"` (or `rex-inbox reply {} -` with the text on stdin).",
            m.id, m.id
        ));
    }
    out
}

async fn deliver(shared: &Shared, d: &Due) -> bool {
    // Check again: the pane may have moved on since the plan.
    {
        let core = shared.lock();
        let still = core.store.message(d.msg.id).ok().flatten().is_some_and(|m| m.status == "queued");
        let same = core
            .pane(d.msg.to_host.as_deref().unwrap_or(""), &d.block)
            .and_then(|p| p.root())
            .is_some_and(|r| r.updated_at == d.root_updated);
        if !still || !same {
            return false;
        }
    }
    let text = render(&d.msg);
    let result = async {
        d.rex.paste(&d.session, &d.block, &text).await?;
        // Let the agent take in the paste before Enter, so Enter submits.
        tokio::time::sleep(Duration::from_millis(250)).await;
        d.rex.send_keys(&d.session, &d.block, &["enter"]).await
    }
    .await;
    let core = shared.lock();
    match result {
        Ok(()) => {
            // Pasted into its prompt counts as read: `wait` won't hand it over again.
            let r = core.store.mark_delivered(d.msg.id, true);
            let _ = core.store.log("delivered", &serde_json::json!({ "id": d.msg.id, "to": d.msg.to_addr }));
            emit_updated(&core, r.ok().flatten());
            true
        }
        Err(e) => {
            let err = format!("{e:#}");
            say!("delivering #{}: {err}", d.msg.id);
            let r = if d.msg.attempts + 1 >= MAX_ATTEMPTS {
                core.store.mark_failed(d.msg.id, &err)
            } else {
                core.store.note_attempt(d.msg.id, &err)
            };
            emit_updated(&core, r.ok().flatten());
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(host: &str, body: &str) -> Message {
        Message {
            id: 9,
            thread_id: 9,
            reply_to: None,
            from_addr: "me".into(),
            from_host: None,
            from_session: None,
            from_block: None,
            to_addr: "inbox/claude".into(),
            to_host: Some(host.into()),
            to_session: Some("s".into()),
            to_block: Some("b".into()),
            subject: None,
            body: body.into(),
            status: "queued".into(),
            created_at: String::new(),
            delivered_at: None,
            read_at: None,
            done_at: None,
            attempts: 0,
            error: None,
        }
    }

    #[test]
    fn remote_panes_answer_in_place() {
        let here = render(&msg("local", "hi"));
        assert!(here.contains("rex-inbox reply 9"));
        let there = render(&msg("studio", "hi"));
        assert!(!there.contains("rex-inbox reply"));
        assert!(there.contains("Answer here"));
        // Long: a pointer here, the whole text there.
        let long = "x\n".repeat(MAX_PASTE);
        assert!(render(&msg("local", &long)).contains("rex-inbox read 9"));
        assert!(render(&msg("studio", &long)).contains(long.trim_end()));
    }
}
