//! Messages between agents, and between agents and you.

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

    /// The caller as a message end. An agent pane speaks for itself; any
    /// other pane (a shell you type in) speaks for you.
    pub fn end(&self, core: &Core) -> End {
        if self.as_me {
            return address::me();
        }
        match self.block.as_deref().and_then(|b| core.pane(&self.host, b)) {
            Some(p) if p.is_agent() => address::end_of(core, p),
            _ => address::me(),
        }
    }

    fn near(&self) -> Option<(&str, &str)> {
        Some((self.host.as_str(), self.session.as_deref()?))
    }
}

fn is_for(m: &Message, who: &End) -> bool {
    match &who.block {
        None => m.to_me(),
        Some(b) => m.to_block.as_deref() == Some(b) && m.to_host == who.host,
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

fn send(core: &Core, from: &End, to: &End, subject: Option<&str>, body: &str, reply_to: Option<i64>) -> Result<Message> {
    if body.trim().is_empty() {
        bail!("the message is empty");
    }
    if from.block.is_some() && from.block == to.block && from.host == to.host {
        bail!("that's your own pane");
    }
    let m = core.store.insert_message(from, to, subject, body, reply_to)?;
    emit_new(core, &m)?;
    Ok(m)
}

pub async fn dispatch(shared: &Shared, method: &str, params: &Value) -> Result<Value> {
    let caller = Caller::from_params(params);
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
            Ok(json!({ "label": name }))
        }

        "messages.send" => {
            let body = params["body"].as_str().unwrap_or_default();
            let subject = params["subject"].as_str().filter(|s| !s.is_empty());
            let to = params["to"].as_str().ok_or_else(|| anyhow!("to is required"))?;
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
            let m = send(&core, &from, &to_end, subject, body, None)?;
            Ok(serde_json::to_value(m)?)
        }

        "messages.reply" => {
            let id = params["id"].as_i64().ok_or_else(|| anyhow!("id is required"))?;
            let body = params["body"].as_str().unwrap_or_default();
            let core = shared.lock();
            let orig = core.store.message(id)?.ok_or_else(|| anyhow!("no message {id}"))?;
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
            let subject = orig.subject.as_ref().map(|s| if s.starts_with("Re: ") { s.clone() } else { format!("Re: {s}") });
            let m = send(&core, &from, &to, subject.as_deref(), body, Some(id))?;
            // Answering a message means you've read it. Your own inbox
            // keeps only what still needs you, so an answer files it.
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
        let thread_of = |m: &Message| thread.is_none_or(|t| m.thread_id == t || core.store.message(t).ok().flatten().is_some_and(|x| x.thread_id == m.thread_id));
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
                        Ok(HubEvent::MessageNew { message }) if is_for(&message, &who) && thread_id.is_none_or(|t| message.thread_id == t) => {
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
