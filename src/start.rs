//! Starting agents from the inbox, on any host the hub follows.
//!
//! `agents.start` opens a new tab in a session on that host (`inbox` unless
//! another is named, made if it doesn't exist), runs Claude Code or Codex
//! there in the folder given, with the first prompt as its argument, and
//! names the pane so it can take messages. Only those two programs: a client
//! can't run anything else through it. The agent then reports its own
//! status, so it shows under Agents and in the inbox like any other.
//!
//! `hosts.list` says what a client needs to pick: each host, whether it's
//! reachable, its sessions, and folders its panes are in. `dirs.list` lists a
//! folder's subfolders on a host, through one of its terminals.

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::Duration;

use crate::hub::{Core, Shared};
use crate::mirror::{HostMirror, TERMINAL};
use crate::rex::Rex;

/// The session new agents go in when none is named.
pub const DEFAULT_SESSION: &str = "inbox";

/// The programs `agents.start` runs, by the names it takes.
fn program(name: &str) -> Result<&'static str> {
    match name {
        "" | "claude" | "claude-code" => Ok("claude"),
        "codex" => Ok("codex"),
        other => bail!("rex-inbox starts claude or codex, not {other}"),
    }
}

/// A pane name agents can address: `[a-z][a-z0-9_-]{0,31}`, as rex-agent takes.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('a'..='z'))
        && name.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// `program`, or `program-2`, `program-3`…: the first not taken.
fn free_name(program: &str, taken: &BTreeSet<String>) -> String {
    if !taken.contains(program) {
        return program.to_string();
    }
    (2..).map(|n| format!("{program}-{n}")).find(|n| !taken.contains(n)).expect("a free name")
}

/// A session by label, id, or the end of its id.
fn find_session<'a>(m: &'a HostMirror, name: &str) -> Option<&'a str> {
    m.sessions
        .values()
        .find(|s| s.label == name || s.session_id == name || (name.len() >= 4 && s.session_id.ends_with(name)))
        .map(|s| s.session_id.as_str())
}

/// A terminal on the host to look at folders through, one in `prefer` first.
fn probe(m: &HostMirror, prefer: Option<&str>) -> Option<(String, String)> {
    let terms = || m.panes.values().filter(|p| p.creator == TERMINAL);
    terms()
        .find(|p| Some(p.session_id.as_str()) == prefer)
        .or_else(|| terms().next())
        .map(|p| (p.session_id.clone(), p.block_id.clone()))
}

/// `~` and `~/x` against `home`.
fn expand(path: &str, home: Option<&str>) -> Option<String> {
    let rest = match path {
        "~" => "",
        p if p.starts_with("~/") => &p[1..],
        p => return Some(p.to_string()),
    };
    home.map(|h| format!("{}{rest}", h.trim_end_matches('/')))
}

/// A followed, reachable host and its Rex, or an error that says why not.
fn reachable<'a>(core: &'a Core, host: &str) -> Result<(&'a HostMirror, Rex)> {
    let rex = core.rex(host).map_err(|_| anyhow!("rex-inbox doesn't follow a host called {host}; see rex hosts"))?;
    let m = core.hosts.get(host).filter(|m| m.connected).ok_or_else(|| {
        let why = core.hosts.get(host).and_then(|m| m.error.clone());
        anyhow!("{host} isn't reachable{}", why.map(|e| format!(": {e}")).unwrap_or_default())
    })?;
    Ok((m, rex))
}

/// Every host, this one first: whether it's reachable, its sessions, and
/// folders worth offering (where its agents are, then other panes).
pub fn hosts(core: &mut Core) -> Value {
    let names: Vec<String> = core.rexes.keys().cloned().collect();
    let mut out: Vec<Value> = Vec::new();
    for name in names {
        let Some(m) = core.hosts.get(&name) else {
            out.push(json!({ "host": name, "connected": false, "error": null, "sessions": [], "folders": [], "agents": 0 }));
            continue;
        };
        let mut panes: Vec<_> = m.panes.values().filter(|p| core.reviewer.managed(m, p).is_none()).collect();
        // Agents first, then the most recently active.
        panes.sort_by_key(|p| (!p.is_agent(), std::cmp::Reverse(p.headline().and_then(|r| r.updated_at.clone()))));
        let mut folders: Vec<String> = Vec::new();
        for p in panes {
            if let Some(c) = &p.cwd
                && !folders.contains(c)
                && Some(c) != m.home.as_ref()
            {
                folders.push(c.clone());
            }
        }
        folders.truncate(12);
        let mut sessions: Vec<&str> = m.sessions.values().map(|s| s.label.as_str()).filter(|l| !l.is_empty()).collect();
        sessions.sort();
        out.push(json!({
            "host": name,
            "connected": m.connected,
            "error": m.error,
            "home": m.home,
            "sessions": sessions,
            "folders": folders,
            "agents": m.panes.values().filter(|p| p.is_agent()).count(),
        }));
    }
    out.sort_by_key(|h| h["host"] != "local");
    Value::Array(out)
}

/// A folder's subfolders on a host.
pub async fn dirs(shared: &Shared, params: &Value) -> Result<Value> {
    let host = params["host"].as_str().unwrap_or("local");
    let path = params["path"].as_str().unwrap_or("~");
    let (rex, probe) = {
        let core = shared.lock();
        let (m, rex) = reachable(&core, host)?;
        (rex, probe(m, None).ok_or_else(|| anyhow!("{host} has no terminal open to look through"))?)
    };
    let (status, path, mut dirs) = rex.list_dir(&probe.0, &probe.1, path).await?;
    dirs.retain(|d| !d.starts_with('.'));
    dirs.sort_by_key(|d| d.to_lowercase());
    Ok(json!({ "status": status, "path": path, "dirs": dirs }))
}

/// Start an agent. Params: `host` (default `local`), `program` (`claude` or
/// `codex`), `cwd` (default `~`), and optionally `name`, `session`, and
/// `prompt`, its first instruction.
pub async fn start(shared: &Shared, params: &Value) -> Result<Value> {
    let host = params["host"].as_str().filter(|h| !h.is_empty()).unwrap_or("local").to_string();
    let program = program(params["program"].as_str().unwrap_or_default())?;
    let prompt = params["prompt"].as_str().map(str::trim).filter(|p| !p.is_empty());
    let wanted_name = params["name"].as_str().map(str::trim).filter(|n| !n.is_empty());
    if let Some(n) = wanted_name
        && !valid_name(n)
    {
        bail!("{n} can't name a pane: use a-z first, then a-z, 0-9, - or _, up to 32 in all");
    }
    let session_name = params["session"].as_str().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(DEFAULT_SESSION);
    let cwd = params["cwd"].as_str().map(str::trim).filter(|c| !c.is_empty()).unwrap_or("~");

    let (rex, session, name, probe, home) = {
        let core = shared.lock();
        let (m, rex) = reachable(&core, &host)?;
        let session = find_session(m, session_name).map(str::to_string);
        let taken: BTreeSet<String> = m
            .panes
            .values()
            .filter(|p| Some(&p.session_id) == session.as_ref())
            .map(|p| p.label.clone())
            .collect();
        let name = match wanted_name {
            Some(n) if taken.contains(n) => bail!("{session_name} on {host} already has a pane called {n}"),
            Some(n) => n.to_string(),
            None => free_name(program, &taken),
        };
        let home = if host == "local" { Some(crate::paths::home().display().to_string()) } else { m.home.clone() };
        let probe = probe(m, session.as_deref());
        (rex, session, name, probe, home)
    };

    // The folder must exist there: Rex would quietly start in another.
    let cwd = match &probe {
        Some((s, b)) => match rex.list_dir(s, b, cwd).await? {
            (st, path, _) if st == "ok" => path,
            (st, path, _) if st == "not_dir" => bail!("{path} on {host} is a file, not a folder"),
            (_, path, _) => bail!("{path} doesn't exist on {host}"),
        },
        None => expand(cwd, home.as_deref())
            .filter(|p| p.starts_with('/'))
            .ok_or_else(|| anyhow!("give the folder on {host} as a full path"))?,
    };

    let mut argv = vec![program];
    // A prompt that starts with - would read as an option.
    let first;
    if let Some(p) = prompt {
        first = if p.starts_with('-') { format!(" {p}") } else { p.to_string() };
        argv.push(&first);
    }
    let created = rex.start(session.as_deref().ok_or(session_name), &name, &cwd, &argv).await?;
    if let Some(m) = shared.lock().hosts.get_mut(&host) {
        m.launched.insert(created.block_id.clone(), std::time::Instant::now());
    }
    settle(&rex, &created.session_id, &created.block_id, program, &host).await?;

    let address = if host == "local" { format!("{session_name}/{name}") } else { format!("{host}:{session_name}/{name}") };
    shared.lock().store.log(
        "agent_started",
        &json!({ "host": host, "program": program, "cwd": cwd, "block_id": created.block_id, "address": address }),
    )?;
    Ok(json!({
        "host": host,
        "program": program,
        "name": name,
        "address": address,
        "cwd": cwd,
        "session_id": created.session_id,
        "window_id": created.window_id,
        "block_id": created.block_id,
    }))
}

/// Wait until the agent is running, or its pane is gone because it
/// couldn't start (not installed there, say). A slow start is fine.
async fn settle(rex: &Rex, session: &str, block: &str, program: &str, host: &str) -> Result<()> {
    for _ in 0..10 {
        tokio::time::sleep(Duration::from_millis(400)).await;
        if !rex.block_ids(session).await?.iter().any(|b| b == block) {
            bail!("{program} exited as soon as it started on {host}. Is it installed there, on a login shell's PATH?");
        }
        if let Ok(p) = rex.block_call(session, block, "process", &json!({})).await
            && p["foreground"]["name"].as_str() == Some(program)
        {
            return Ok(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(valid_name("claude-2"));
        assert!(valid_name("api_fix"));
        assert!(!valid_name("2claude"));
        assert!(!valid_name("Claude"));
        assert!(!valid_name("has space"));
        assert!(!valid_name(&"a".repeat(33)));
        let taken: BTreeSet<String> = ["claude".into(), "claude-2".into(), "shell".into()].into();
        assert_eq!(free_name("claude", &taken), "claude-3");
        assert_eq!(free_name("codex", &taken), "codex");
    }

    #[test]
    fn programs() {
        assert_eq!(program("").unwrap(), "claude");
        assert_eq!(program("claude-code").unwrap(), "claude");
        assert_eq!(program("codex").unwrap(), "codex");
        assert!(program("bash").is_err());
        assert!(program("claude; rm -rf ~").is_err());
    }

    #[test]
    fn tilde() {
        assert_eq!(expand("~", Some("/home/j")).unwrap(), "/home/j");
        assert_eq!(expand("~/code/api", Some("/home/j/")).unwrap(), "/home/j/code/api");
        assert_eq!(expand("/srv/x", None).unwrap(), "/srv/x");
        assert!(expand("~/x", None).is_none());
    }
}
