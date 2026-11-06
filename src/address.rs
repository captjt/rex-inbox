//! Addresses: `me`, or a pane as `pane`, `session/pane`, `host:session/pane`,
//! or a block id.

use anyhow::{Result, bail};

use crate::hub::Core;
use crate::mirror::Pane;
use crate::store::End;

pub const ME: &str = "me";

pub fn short(id: &str) -> String {
    crate::show::short_id(id)
}

/// A pane's address: `session/pane`, with `host:` for other hosts. An
/// unlabeled pane goes by the end of its block id.
pub fn of(core: &Core, p: &Pane) -> String {
    let session = core
        .hosts
        .get(&p.host)
        .and_then(|m| m.session_label(&p.session_id))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| short(&p.session_id));
    let pane = if p.label.is_empty() { short(&p.block_id) } else { p.label.clone() };
    let a = format!("{session}/{pane}");
    if p.host == "local" { a } else { format!("{}:{a}", p.host) }
}

pub fn end_of(core: &Core, p: &Pane) -> End {
    End { addr: of(core, p), host: Some(p.host.clone()), session: Some(p.session_id.clone()), block: Some(p.block_id.clone()) }
}

pub fn me() -> End {
    End { addr: ME.to_string(), ..Default::default() }
}

fn session_matches(core: &Core, p: &Pane, s: &str) -> bool {
    let label = core.hosts.get(&p.host).and_then(|m| m.session_label(&p.session_id));
    label == Some(s) || p.session_id == s || (s.len() >= 4 && p.session_id.ends_with(s))
}

fn pane_matches(p: &Pane, name: &str) -> bool {
    p.label == name || p.block_id == name || (name.len() >= 4 && p.block_id.ends_with(name))
}

/// Find the pane an address names. `near` is the sender's `(host, session)`:
/// a bare pane name looks there first.
pub fn resolve<'a>(core: &'a Core, addr: &str, near: Option<(&str, &str)>) -> Result<&'a Pane> {
    let addr = addr.trim();
    if addr.is_empty() {
        bail!("an address is required");
    }
    let all = || core.hosts.values().flat_map(|m| m.panes.values());
    if addr.starts_with("block:") {
        let found: Vec<&Pane> = all().filter(|p| p.block_id == addr).collect();
        return pick(core, addr, found);
    }
    let (host, rest) = match addr.split_once(':') {
        Some((h, r)) => (Some(h), r),
        None => (None, addr),
    };
    let (session, pane) = match rest.rsplit_once('/') {
        Some((s, p)) => (Some(s), p),
        None => (None, rest),
    };
    let found: Vec<&Pane> = all()
        .filter(|p| host.is_none_or(|h| p.host == h))
        .filter(|p| session.is_none_or(|s| session_matches(core, p, s)))
        .filter(|p| pane_matches(p, pane))
        .collect();
    if session.is_none()
        && found.len() > 1
        && let Some((h, s)) = near
    {
        let here: Vec<&Pane> = found.iter().copied().filter(|p| p.host == h && p.session_id == s).collect();
        if !here.is_empty() {
            return pick(core, addr, here);
        }
    }
    pick(core, addr, found)
}

/// One pane, or an error naming the candidates. Among several, a lone agent
/// wins: plain shells can't take messages anyway.
fn pick<'a>(core: &Core, addr: &str, found: Vec<&'a Pane>) -> Result<&'a Pane> {
    match found.len() {
        0 => bail!("no pane is called {addr}. List them with: rex-inbox agents --all"),
        1 => Ok(found[0]),
        _ => {
            let agents: Vec<&Pane> = found.iter().copied().filter(|p| p.is_agent()).collect();
            if agents.len() == 1 {
                return Ok(agents[0]);
            }
            let names: Vec<String> = found.iter().map(|p| format!("{} ({})", of(core, p), p.block_id)).collect();
            bail!(
                "{addr} matches {} panes: {}. Use a block id, or give panes names with: rex-inbox name NAME",
                found.len(),
                names.join(", ")
            )
        }
    }
}
