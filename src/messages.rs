//! Messages between agents, and between agents and you.
//!
//! A message can offer answers, as buttons: `send --option Approve --option
//! Wait`. The reply that answers it says which one it picks (`choice`), so
//! whoever asked reads the answer without parsing the reply's words. A
//! preview's answers work the same way (see preview.rs).

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::broadcast;

use crate::address::{self, ME};
use crate::hub::{Core, HubEvent, Shared};
use crate::store::{End, Message};

/// Who is calling: the pane the CLI runs in (`REX_SESSION`, `REX_BLOCK`),
/// or the person.
#[derive(Debug, Default)]
pub struct Caller {
    pub host: String,
    pub session: Option<String>,
    pub block: Option<String>,
    pub as_me: bool,
}

impl Caller {
    pub fn from_params(p: &Value) -> Self {
        let c = &p["caller"];
        Caller {
            host: c["host"].as_str().unwrap_or("local").to_string(),
            session: c["session_id"].as_str().map(str::to_string),
            block: c["block_id"].as_str().map(str::to_string),
            as_me: c["as_me"].as_bool().unwrap_or(false),
        }
    }

    /// The caller's pane, wherever it is. The CLI knows its pane but not
    /// what the hub calls its machine, and Rex's block ids are unique across
    /// servers, so the host is the one that has the block.
    pub fn locate(&mut self, core: &Core) {
        let Some(b) = self.block.as_deref() else { return };
        if core.pane(&self.host, b).is_some() {
            return;
        }
        if let Some(p) = core.hosts.values().flat_map(|m| m.panes.values()).find(|p| p.block_id == b) {
            self.host = p.host.clone();
        }
    }

    /// The caller as a message end. A pane whose program reports its status
    /// to Rex speaks for itself: an agent, or a tool such as rex-develop,
    /// which asks you things and reads your answers with `inbox`. A plain
    /// shell reports nothing, and what you type there speaks for you.
    pub fn end(&self, core: &Core) -> End {
        if self.as_me {
            return address::me();
        }
        match self.block.as_deref().and_then(|b| core.pane(&self.host, b)) {
            Some(p) if p.app().is_some() => address::end_of(core, p),
            _ => address::me(),
        }
    }

    fn near(&self) -> Option<(&str, &str)> {
        Some((self.host.as_str(), self.session.as_deref()?))
    }
}

/// Within the last ten seconds.
fn just_now(at: &str) -> bool {
    crate::time::parse(at).is_some_and(|t| time::OffsetDateTime::now_utc() - t < time::Duration::seconds(10))
}

fn is_for(m: &Message, who: &End) -> bool {
    match &who.block {
        None => m.to_me(),
        Some(b) => m.to_block.as_deref() == Some(b) && m.to_host == who.host,
    }
}

/// The most answers a message can offer: as many as fit on a phone's card
/// and in a notification's menu.
const MAX_OPTIONS: usize = 6;

/// The answers a message offers, from `send`'s `options`: trimmed, without
/// blanks or repeats (in any case), in the order given.
fn options(v: &Value) -> Result<Vec<String>> {
    if !v.is_null() && !v.is_array() {
        bail!("options is a list of answers, such as [\"Approve\", \"Wait\"]");
    }
    let mut out: Vec<String> = Vec::new();
    for o in v.as_array().into_iter().flatten() {
        let o = o.as_str().ok_or_else(|| anyhow!("an option is text, not {o}"))?.trim();
        if !o.is_empty() && !out.iter().any(|x| x.to_lowercase() == o.to_lowercase()) {
            out.push(o.to_string());
        }
    }
    if out.len() > MAX_OPTIONS {
        bail!("a message offers at most {MAX_OPTIONS} options, not {}; ask about fewer, or leave them to the reply", out.len());
    }
    Ok(out)
}

/// The option of `orig` that `choice` names, as `orig` spells it. Case
/// doesn't matter, so `--choice approve` picks "Approve".
fn pick(orig: &Message, choice: &str) -> Result<String> {
    let offered = orig.offered();
    if offered.is_empty() {
        bail!("#{} offers no options to choose from; reply without a choice", orig.id);
    }
    let found = offered.iter().find(|o| *o == choice).or_else(|| offered.iter().find(|o| o.to_lowercase() == choice.to_lowercase()));
    match found {
        Some(o) => Ok(o.clone()),
        None => {
            let valid: Vec<String> = offered.iter().map(|o| format!("\"{o}\"")).collect();
            bail!("\"{choice}\" isn't one of #{}'s options: {}", orig.id, valid.join(", "))
        }
    }
}

/// The option a reply picks: the one it names, or, when it names none, the
/// option its words are (typing "approve" in the reply box picks Approve).
fn choice_of(orig: &Message, choice: Option<&str>, body: &str) -> Result<Option<String>> {
    match choice.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => pick(orig, c).map(Some),
        None => Ok(pick(orig, body.trim()).ok()),
    }
}

fn emit_new(core: &Core, m: &Message) -> Result<()> {
    core.store.log("message", &json!({ "id": m.id, "from": m.from_addr, "to": m.to_addr }))?;
    core.emit(HubEvent::MessageNew { message: m.clone() });
    Ok(())
}

pub fn emit_updated(core: &Core, m: Option<Message>) {
    if let Some(m) = m {
        core.emit(HubEvent::MessageUpdated { message: m });
    }
}

/// Store a new message. The caller sets what else it carries (its options,
/// or the choice it makes), then tells the apps with `emit_new`.
fn send(core: &Core, from: &End, to: &End, subject: Option<&str>, body: &str, reply_to: Option<i64>) -> Result<Message> {
    if body.trim().is_empty() {
        bail!("the message is empty");
    }
    if from.block.is_some() && from.block == to.block && from.host == to.host {
        bail!("that's your own pane");
    }
    core.store.insert_message(from, to, subject, body, reply_to)
}

pub async fn dispatch(shared: &Shared, method: &str, params: &Value) -> Result<Value> {
    let mut caller = Caller::from_params(params);
    caller.locate(&shared.lock());
    match method {
        "whoami" => {
            let core = shared.lock();
            let me = caller.end(&core);
            let pane = caller.block.as_deref().and_then(|b| core.pane(&caller.host, b));
            Ok(json!({
                "address": me.addr,
                "is_agent": pane.is_some_and(|p| p.is_agent()),
                "app": pane.and_then(|p| p.app()),
                "pane": pane.map(|p| address::of(&core, p)),
                "block_id": caller.block,
                "session_id": caller.session,
            }))
        }

        "name" => {
            let name = params["name"].as_str().unwrap_or("").trim().to_string();
            if name.is_empty() || name.contains(['/', ':']) || name.starts_with("block") || name == ME {
                bail!("pick a name without / or :, other than me");
            }
            let (session, block) = (
                caller.session.clone().ok_or_else(|| anyhow!("run this in a Rex pane"))?,
                caller.block.clone().ok_or_else(|| anyhow!("run this in a Rex pane"))?,
            );
            let rex = shared.lock().rex(&caller.host)?;
            rex.api(Some(&session), "session.set_block_label", &json!({ "block_id": block, "label": name })).await?;
            // Rex's event for the new label comes later; until then whoami,
            // ls and sends to the new name would still see the old one.
            if let Some(p) = shared.lock().hosts.get_mut(&caller.host).and_then(|m| m.panes.get_mut(&block)) {
                p.label = name.clone();
            }
            Ok(json!({ "label": name }))
        }

        "messages.send" => {
            let body = params["body"].as_str().unwrap_or_default();
            let subject = params["subject"].as_str().filter(|s| !s.is_empty());
            let to = params["to"].as_str().ok_or_else(|| anyhow!("to is required"))?;
            let options = options(&params["options"])?;
            let mut core = shared.lock();
            let from = caller.end(&core);
            let to_end = if to == ME { address::me() } else { address::end_of(&core, address::resolve(&core, to, caller.near())?) };
            if from.block.is_none() && to_end.block.is_none() {
                bail!("a message from you to you? Pass --to with an agent's pane");
            }
            // A message pasted into a reviewer mid-round would end up in its review.
            if let (Some(host), Some(block)) = (&to_end.host, &to_end.block)
                && let Some(what) = core.managed(host, block)
            {
                bail!("{to} is rex-reviewer's ({what}), so it takes no messages; open it in Rex instead");
            }
            let mut m = send(&core, &from, &to_end, subject, body, None)?;
            if !options.is_empty() {
                m = core.store.set_options(m.id, &options)?.unwrap_or(m);
            }
            emit_new(&core, &m)?;
            Ok(serde_json::to_value(m)?)
        }

        "messages.reply" => {
            let id = params["id"].as_i64().ok_or_else(|| anyhow!("id is required"))?;
            let body = params["body"].as_str().unwrap_or_default();
            let core = shared.lock();
            let orig = core.store.message(id)?.ok_or_else(|| anyhow!("no message {id}"))?;
            let choice = choice_of(&orig, params["choice"].as_str(), body)?;
            // A choice alone says it all: its label is the reply.
            let body = match &choice {
                Some(c) if body.trim().is_empty() => c.as_str(),
                _ => body,
            };
            let from = caller.end(&core);
            // Answer the other party: the sender, unless you are the sender.
            let sent_it = match (&from.block, &orig.from_block) {
                (None, None) => true,
                (Some(a), Some(b)) => a == b,
                _ => false,
            };
            let (addr, host, session, block) = if sent_it {
                (&orig.to_addr, &orig.to_host, &orig.to_session, &orig.to_block)
            } else {
                (&orig.from_addr, &orig.from_host, &orig.from_session, &orig.from_block)
            };
            let to = End { addr: addr.clone(), host: host.clone(), session: session.clone(), block: block.clone() };
            // The same answer twice in a moment is one tap too many (or one
            // on the Mac and one on the phone): the agent gets it once.
            if let Some(dup) = core.store.thread(orig.thread_id)?.into_iter().rev().find(|m| {
                m.reply_to == Some(id)
                    && m.from_block == from.block
                    && m.body.trim() == body.trim()
                    && m.choice == choice
                    && just_now(&m.created_at)
            }) {
                return Ok(serde_json::to_value(dup)?);
            }
            let subject = orig.subject.as_ref().map(|s| if s.starts_with("Re: ") { s.clone() } else { format!("Re: {s}") });
            let mut m = send(&core, &from, &to, subject.as_deref(), body, Some(id))?;
            if let Some(c) = &choice {
                m = core.store.set_choice(m.id, c)?.unwrap_or(m);
            }
            emit_new(&core, &m)?;
            // A preview remembers your answer, so it shows as given.
            if from.block.is_none()
                && let Some(mut p) = orig.preview.clone()
            {
                p["answer"] = json!(choice.as_deref().unwrap_or(body.trim()));
                p["answered_at"] = json!(m.created_at);
                emit_updated(&core, core.store.set_preview(orig.id, &p)?);
            }
            // Answering a message means you've read it. Your own inbox
            // keeps only what still needs you, so an answer files it.
            // Answering a preview finishes it too: filed, it comes off the
            // tailnet, and the agent shares again for another look.
            if is_for(&orig, &from) {
                let fresh = if orig.to_me() { core.store.mark_done(orig.id)? } else { core.store.mark_read(&orig)? };
                emit_updated(&core, fresh);
            }
            Ok(serde_json::to_value(m)?)
        }

        "messages.list" => {
            let all = params["all"].as_bool().unwrap_or(false);
            let core = shared.lock();
            let who = caller.end(&core);
            let list = match (&who.block, &who.host) {
                (Some(b), Some(h)) => {
                    let mut v = core.store.messages_to_block(h, b, !all)?;
                    if all {
                        v.extend(core.store.messages_from_block(h, b)?);
                        v.sort_by_key(|m| std::cmp::Reverse(m.id));
                    }
                    v
                }
                _ => core.store.messages_to_me(all)?,
            };
            Ok(serde_json::to_value(list)?)
        }

        "messages.read" => {
            let id = params["id"].as_i64().ok_or_else(|| anyhow!("id is required"))?;
            let core = shared.lock();
            let m = core.store.message(id)?.ok_or_else(|| anyhow!("no message {id}"))?;
            let who = caller.end(&core);
            let m = if is_for(&m, &who) {
                let fresh = core.store.mark_read(&m)?;
                emit_updated(&core, fresh.clone());
                fresh.unwrap_or(m)
            } else {
                m
            };
            let thread = core.store.thread(m.thread_id)?;
            Ok(json!({ "message": m, "thread": thread }))
        }

        "messages.done" => {
            let id = params["id"].as_i64().ok_or_else(|| anyhow!("id is required"))?;
            let core = shared.lock();
            let m = core.store.mark_done(id)?.ok_or_else(|| anyhow!("no message {id}"))?;
            emit_updated(&core, Some(m.clone()));
            Ok(serde_json::to_value(m)?)
        }

        "messages.wait" => wait(shared, &caller, params).await,

        "previews.share" => crate::preview::share(shared, &caller, params).await,
        "previews.stop" => crate::preview::stop(shared, &caller, params).await,

        other => bail!("unknown method {other}"),
    }
}

/// Wait for the next unread message to the caller (in one thread, with
/// `thread`), mark it read, and return it.
async fn wait(shared: &Shared, caller: &Caller, params: &Value) -> Result<Value> {
    let timeout = params["timeout"].as_f64().map(Duration::from_secs_f64);
    let thread = params["thread"].as_i64();
    let (mut rx, who, pending) = {
        let core = shared.lock();
        // Subscribe before looking, so nothing lands in between.
        let rx = core.events.subscribe();
        let who = caller.end(&core);
        let pending = match (&who.block, &who.host) {
            (Some(b), Some(h)) => core.store.messages_to_block(h, b, true)?,
            _ => core.store.messages_to_me(false)?.into_iter().filter(|m| m.status == "unread").collect(),
        };
        let thread_of = |m: &Message| {
            thread.is_none_or(|t| m.thread_id == t || core.store.message(t).ok().flatten().is_some_and(|x| x.thread_id == m.thread_id))
        };
        let pending = pending.into_iter().filter(|m| thread_of(m)).min_by_key(|m| m.id);
        (rx, who, pending)
    };
    let found = match pending {
        Some(m) => m,
        None => {
            let thread_id = thread.map(|t| shared.lock().store.message(t).ok().flatten().map_or(t, |m| m.thread_id));
            let next = async {
                loop {
                    match rx.recv().await {
                        Ok(HubEvent::MessageNew { message })
                            if is_for(&message, &who) && thread_id.is_none_or(|t| message.thread_id == t) =>
                        {
                            return Ok(message);
                        }
                        Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(e) => return Err(anyhow!("the hub is stopping: {e}")),
                    }
                }
            };
            match timeout {
                Some(t) => match tokio::time::timeout(t, next).await {
                    Ok(r) => r?,
                    Err(_) => return Ok(json!({ "timeout": true })),
                },
                None => next.await?,
            }
        }
    };
    let core = shared.lock();
    let fresh = core.store.mark_read(&found)?;
    emit_updated(&core, fresh.clone());
    Ok(json!({ "message": fresh.unwrap_or(found) }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mirror::Record;

    /// A hub with one pane, `b1`, whose program reports as `app`.
    fn hub_with(app: &str) -> Shared {
        let shared = Shared::for_tests();
        let r = Record { state: "idle".into(), app: Some(app.into()), ..Default::default() };
        shared.lock().hosts.get_mut("local").unwrap().apply_pane_status("s1", "b1", vec![r], None, None);
        shared
    }

    fn from_pane(mut params: Value) -> Value {
        params["caller"] = json!({ "session_id": "s1", "block_id": "b1" });
        params
    }

    fn as_me(mut params: Value) -> Value {
        params["caller"] = json!({ "as_me": true });
        params
    }

    async fn call(shared: &Shared, method: &str, params: Value) -> Result<Value> {
        dispatch(shared, method, &params).await
    }

    #[test]
    fn options_are_trimmed_kept_once_and_few() {
        let o = options(&json!([" Approve ", "", "Wait", "approve", "  "])).unwrap();
        assert_eq!(o, ["Approve", "Wait"]);
        assert!(options(&Value::Null).unwrap().is_empty());
        assert_eq!(options(&json!(["1", "2", "3", "4", "5", "6"])).unwrap().len(), MAX_OPTIONS);
        let e = options(&json!(["1", "2", "3", "4", "5", "6", "7"])).unwrap_err().to_string();
        assert!(e.contains("at most 6"), "{e}");
        assert!(options(&json!("Approve")).is_err());
        assert!(options(&json!([1])).is_err());
    }

    #[tokio::test]
    async fn a_reply_picks_one_of_the_messages_options() {
        let hub = hub_with("claude-code");
        let ask = json!({ "to": "me", "subject": "PR 12", "body": "Merge it?", "options": ["Merge", " Wait ", "merge"] });
        let m = call(&hub, "messages.send", from_pane(ask)).await.unwrap();
        assert_eq!(m["options"], json!(["Merge", "Wait"]));
        assert_eq!(m["choice"], Value::Null);
        let id = m["id"].as_i64().unwrap();

        let e = call(&hub, "messages.reply", as_me(json!({ "id": id, "body": "", "choice": "Maybe" }))).await.unwrap_err();
        assert_eq!(e.to_string(), format!("\"Maybe\" isn't one of #{id}'s options: \"Merge\", \"Wait\""));

        // A choice alone is the answer; its case is the message's.
        let r = call(&hub, "messages.reply", as_me(json!({ "id": id, "body": "  ", "choice": "wait" }))).await.unwrap();
        assert_eq!((r["choice"].as_str(), r["body"].as_str()), (Some("Wait"), Some("Wait")));
        assert_eq!((r["reply_to"].as_i64(), r["to_block"].as_str(), r["status"].as_str()), (Some(id), Some("b1"), Some("queued")));
        assert_eq!(hub.lock().store.message(id).unwrap().unwrap().status, "done", "answered, it's filed");

        // Words with it are kept.
        let r = call(&hub, "messages.reply", as_me(json!({ "id": id, "body": "after the tests", "choice": "Merge" }))).await.unwrap();
        assert_eq!((r["choice"].as_str(), r["body"].as_str()), (Some("Merge"), Some("after the tests")));
    }

    #[tokio::test]
    async fn a_reply_that_says_an_option_picks_it() {
        let hub = hub_with("claude-code");
        let m = call(&hub, "messages.send", from_pane(json!({ "to": "me", "body": "Merge it?", "options": ["Merge", "Wait"] }))).await;
        let id = m.unwrap()["id"].as_i64().unwrap();
        let r = call(&hub, "messages.reply", as_me(json!({ "id": id, "body": "merge " }))).await.unwrap();
        assert_eq!(r["choice"], "Merge");
        let r = call(&hub, "messages.reply", as_me(json!({ "id": id, "body": "merge it tomorrow" }))).await.unwrap();
        assert_eq!(r["choice"], Value::Null);
    }

    #[tokio::test]
    async fn a_choice_needs_options_to_choose_from() {
        let hub = hub_with("claude-code");
        let m = call(&hub, "messages.send", from_pane(json!({ "to": "me", "body": "Done with the migration" }))).await.unwrap();
        assert_eq!(m["options"], json!([]));
        let id = m["id"].as_i64().unwrap();
        let e = call(&hub, "messages.reply", as_me(json!({ "id": id, "choice": "Approve" }))).await.unwrap_err();
        assert!(e.to_string().contains("offers no options"), "{e}");
        let e = call(&hub, "messages.send", from_pane(json!({ "to": "me", "body": "?", "options": ["1", "2", "3", "4", "5", "6", "7"] })));
        assert!(e.await.is_err());
    }

    #[tokio::test]
    async fn answering_a_preview_with_a_button_is_a_choice_too() {
        let hub = hub_with("claude-code");
        let id = {
            let core = hub.lock();
            let from = End { addr: "s1/b1".into(), host: Some("local".into()), session: Some("s1".into()), block: Some("b1".into()) };
            let m = core.store.insert_message(&from, &address::me(), Some("Checkout form"), "Does it read right?", None).unwrap();
            let p =
                json!({ "url": "https://mac.tail1.ts.net:7431/", "port": 5173, "options": ["Looks right", "Needs work"], "state": "live" });
            core.store.set_preview(m.id, &p).unwrap();
            m.id
        };
        let e = call(&hub, "messages.reply", as_me(json!({ "id": id, "choice": "Approve" }))).await.unwrap_err();
        assert!(e.to_string().contains("\"Looks right\", \"Needs work\""), "{e}");
        let r = call(&hub, "messages.reply", as_me(json!({ "id": id, "body": "the button's off", "choice": "Needs work" }))).await.unwrap();
        assert_eq!((r["choice"].as_str(), r["body"].as_str()), (Some("Needs work"), Some("the button's off")));
        let p = hub.lock().store.message(id).unwrap().unwrap().preview.unwrap();
        assert_eq!(p["answer"], "Needs work", "the preview shows the button you pressed");
    }

    #[tokio::test]
    async fn a_tool_that_isnt_an_agent_asks_and_reads_the_answer_itself() {
        let hub = hub_with("rex-develop");
        let ask = json!({ "to": "me", "body": "Ship it?", "options": ["Ship", "Hold"] });
        let m = call(&hub, "messages.send", from_pane(ask)).await.unwrap();
        assert_eq!((m["from_block"].as_str(), m["from_addr"].as_str()), (Some("b1"), Some("s1/b1")));
        let id = m["id"].as_i64().unwrap();
        let r = call(&hub, "messages.reply", as_me(json!({ "id": id, "choice": "Ship" }))).await.unwrap();
        assert_eq!(r["status"], "queued", "it waits: a tool's pane is never typed into");

        // The tool lists it until it reads it, and then not again.
        let list = call(&hub, "messages.list", from_pane(json!({}))).await.unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(
            (list[0]["reply_to"].as_i64(), list[0]["thread_id"].as_i64(), list[0]["choice"].as_str()),
            (Some(id), Some(id), Some("Ship"))
        );
        let read = call(&hub, "messages.read", from_pane(json!({ "id": r["id"] }))).await.unwrap();
        assert!(read["message"]["read_at"].is_string());
        assert!(call(&hub, "messages.list", from_pane(json!({}))).await.unwrap().as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_plain_shell_speaks_for_you() {
        let hub = Shared::for_tests();
        hub.lock().hosts.get_mut("local").unwrap().apply_pane_status("s1", "b1", vec![], None, None);
        let e = call(&hub, "messages.send", from_pane(json!({ "to": "me", "body": "hi" }))).await.unwrap_err();
        assert!(e.to_string().contains("from you to you"), "{e}");
        let who = call(&hub, "whoami", from_pane(json!({}))).await.unwrap();
        assert_eq!(who["address"], "me");
    }

    #[tokio::test]
    async fn the_same_words_with_another_choice_are_another_answer() {
        let hub = hub_with("claude-code");
        let m = call(&hub, "messages.send", from_pane(json!({ "to": "me", "body": "Merge it?", "options": ["Merge", "Wait"] }))).await;
        let id = m.unwrap()["id"].as_i64().unwrap();
        let a = call(&hub, "messages.reply", as_me(json!({ "id": id, "body": "ok", "choice": "Merge" }))).await.unwrap();
        let again = call(&hub, "messages.reply", as_me(json!({ "id": id, "body": "ok", "choice": "Merge" }))).await.unwrap();
        assert_eq!(a["id"], again["id"], "a second tap is the same answer");
        let other = call(&hub, "messages.reply", as_me(json!({ "id": id, "body": "ok", "choice": "Wait" }))).await.unwrap();
        assert_ne!(a["id"], other["id"]);
    }
}
