//! rex-reviewer, when it runs alongside: its PRs as items you can act on,
//! and its agents' panes folded under the PR they review. Without
//! rex-reviewer none of this applies.
//!
//! rex-reviewer's watcher reports one program status record per PR that
//! needs you: app `rex-reviewer`, id `pr/<owner>/<name>/<pr>/<round>`. A
//! review ready to post is `blocked` on `permission`. Its Rex actions take
//! that id as `item=`, and refuse when the round it names is over:
//!
//! | here | runs |
//! | --- | --- |
//! | approve | `reviewer_post item=<id>` |
//! | deny | `reviewer_dismiss item=<id>` |
//! | preview | `reviewer_preview item=<id>` |
//! | open | `reviewer_focus item=<id>`: the PR's tab, not the watcher's |
//!
//! Its agents' panes are listed in its ledger (`~/.local/state/rex-reviewer`,
//! the contract in rex-reviewer's docs/ledger.md). Such a pane is managed:
//! it raises no "finished" alerts (the round moves on without you), it takes
//! no messages, it isn't listed with your agents, and its prompts and
//! failures name the PR, such as `api#12 · codex reviewer`. On a host whose
//! ledger the hub can't read, the agents in a session that holds a
//! rex-reviewer pane count as managed.

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::mirror::{HostMirror, Pane};
use crate::rex::Rex;

pub const APP: &str = "rex-reviewer";

/// How long a read of the ledger stays good.
const FRESH: Duration = Duration::from_secs(2);

/// A record of rex-reviewer's that stands for a PR, as opposed to the
/// watcher's own record.
pub fn is_item(app: Option<&str>, record_id: &str) -> bool {
    app == Some(APP) && record_id.starts_with("pr/")
}

/// Where an item stands in for a pane's place: the PR and round its id
/// names, such as `api#12 · round 2`. The watcher's pane and folder say
/// nothing about the PR.
pub fn item_subject(record_id: &str) -> Option<String> {
    let mut parts = record_id.strip_prefix("pr/")?.split('/');
    let (_owner, name, pr, round) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    Some(format!("{name}#{pr} · round {round}"))
}

/// An answer a client offers for an item: the hub method it calls
/// (`approve`, `deny`, or `preview`) and the button's label.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Answer {
    pub action: &'static str,
    pub label: &'static str,
}

/// A review ready to post: post it, dismiss it, or read it first.
pub const ANSWERS: [Answer; 3] = [
    Answer { action: "approve", label: "Post" },
    Answer { action: "deny", label: "Dismiss" },
    Answer { action: "preview", label: "Preview" },
];

/// Run one of rex-reviewer's actions on an item.
pub async fn act(rex: &Rex, action: &str, item: &str) -> Result<Value> {
    rex.action(action, &[("item", item)]).await
}

/// `$REX_REVIEWER_STATE_DIR` (rex-reviewer's menu bar app reads the same),
/// or `~/.local/state/rex-reviewer`.
pub fn state_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("REX_REVIEWER_STATE_DIR") {
        return PathBuf::from(d);
    }
    crate::paths::home().join(".local/state/rex-reviewer")
}

/// Which panes rex-reviewer runs, from its ledger on this machine.
pub struct Ledger {
    dir: PathBuf,
    /// `(session id, pane label)` to what the pane is doing, such as
    /// `api#12 · codex reviewer`.
    panes: HashMap<(String, String), String>,
    /// The ledger exists: rex-reviewer runs here.
    present: bool,
    read_at: Option<Instant>,
}

impl Ledger {
    pub fn new(dir: PathBuf) -> Self {
        Ledger { dir, panes: HashMap::new(), present: false, read_at: None }
    }

    /// Read the ledger again if the last read is stale.
    fn refresh(&mut self) {
        if self.read_at.is_some_and(|t| t.elapsed() < FRESH) {
            return;
        }
        self.read_at = Some(Instant::now());
        self.present = self.dir.join("state.json").is_file();
        self.panes = if self.present { read_panes(&self.dir) } else { HashMap::new() };
    }

    /// What a pane does for rex-reviewer, such as `api#12 · codex reviewer`,
    /// or `None` when rex-reviewer doesn't run it. The ledger is local, so
    /// panes on the hub's own host are looked up there; elsewhere, an agent
    /// in a session that holds a rex-reviewer pane counts, and is named by
    /// its tab.
    pub fn managed(&mut self, m: &HostMirror, p: &Pane) -> Option<String> {
        if p.host == "local" {
            self.refresh();
            if self.present {
                return self.panes.get(&(p.session_id.clone(), p.label.clone())).cloned();
            }
        }
        if !p.is_agent() {
            return None;
        }
        let runs_here = m.panes.values().any(|q| q.session_id == p.session_id && q.app() == Some(APP));
        if !runs_here {
            return None;
        }
        Some(m.window_label(&p.session_id, p.window_id.as_deref()).filter(|l| !l.is_empty()).unwrap_or(&p.label).to_string())
    }
}

/// Every pane an entry in the ledger names, for entries whose panes may be
/// open: a round in progress, or a posted one whose tab hasn't closed yet
/// (the watcher clears `pane` and `checker_pane` when it closes the tab).
fn read_panes(dir: &Path) -> HashMap<(String, String), String> {
    let mut out = HashMap::new();
    let repos = dir.join("repos");
    for owner in read_dir(&repos) {
        for name in read_dir(&owner) {
            for file in read_dir(&name) {
                let is_entry = file.file_name().and_then(|f| f.to_str()).is_some_and(|f| f.starts_with("pr-") && f.ends_with(".json"));
                if !is_entry {
                    continue;
                }
                let Some(e) = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { continue };
                for (pane, what) in entry_panes(&e) {
                    if let Some(session) = e["session_id"].as_str() {
                        out.insert((session.to_string(), pane), what);
                    }
                }
            }
        }
    }
    out
}

fn read_dir(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir).map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect()).unwrap_or_default()
}

/// One entry's panes, each with what it does.
fn entry_panes(e: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let tab_open = e["pane"].is_string() || e["checker_pane"].is_string();
    let open = match e["state"].as_str() {
        Some("reviewing") => true,
        Some("posted") => tab_open,
        _ => false,
    };
    let (Some(repo), Some(pr)) = (e["repo"].as_str(), e["pr"].as_i64()) else { return out };
    if !open {
        return out;
    }
    let name = repo.rsplit('/').next().unwrap_or(repo);
    let label = format!("{name}#{pr}");
    // Each reviewer, by name. Entries before rex-reviewer 0.4 name one
    // reviewer, whose pane is `pane`.
    let mut named = false;
    if let Some(rs) = e["reviewers"].as_object() {
        for (rname, r) in rs {
            if let Some(pane) = r["pane"].as_str() {
                out.push((pane.to_string(), format!("{label} · {rname} reviewer")));
                named = true;
            }
        }
    }
    if !named && let Some(pane) = e["pane"].as_str() {
        let who = e["reviewer"].as_str().unwrap_or("the");
        out.push((pane.to_string(), format!("{label} · {who} reviewer")));
    }
    if let Some(pane) = e["checker_pane"].as_str() {
        out.push((pane.to_string(), format!("{label} · checker")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn subjects() {
        assert_eq!(item_subject("pr/acme/api/12/2").as_deref(), Some("api#12 · round 2"));
        assert_eq!(item_subject("pr/acme/api"), None);
        assert_eq!(item_subject(""), None);
    }

    #[test]
    fn items() {
        assert!(is_item(Some("rex-reviewer"), "pr/acme/api/12/2"));
        assert!(!is_item(Some("rex-reviewer"), ""));
        assert!(!is_item(Some("claude-code"), "pr/acme/api/12/2"));
        assert!(!is_item(None, "pr/acme/api/12/2"));
    }

    #[test]
    fn panes_of_an_entry() {
        let e = json!({
            "repo": "acme/api", "pr": 12, "state": "reviewing", "session_id": "s1",
            "pane": "api-12", "checker_pane": "api-12-check",
            "reviewers": {
                "codex": { "pane": "api-12", "agent": "codex" },
                "claude": { "pane": "api-12-claude", "agent": "claude" }
            }
        });
        let mut got = entry_panes(&e);
        got.sort();
        assert_eq!(
            got,
            vec![
                ("api-12".into(), "api#12 · codex reviewer".into()),
                ("api-12-check".into(), "api#12 · checker".into()),
                ("api-12-claude".into(), "api#12 · claude reviewer".into()),
            ]
        );
    }

    #[test]
    fn old_entries_and_finished_ones() {
        let old = json!({ "repo": "acme/api", "pr": 3, "state": "posted", "pane": "api-3", "reviewer": "codex" });
        assert_eq!(entry_panes(&old), vec![("api-3".into(), "api#3 · codex reviewer".into())]);
        // Posted, and its tab closed: the reviewers' records keep their pane names.
        let closed = json!({ "repo": "acme/api", "pr": 5, "state": "posted", "reviewers": { "codex": { "pane": "api-5" } } });
        assert!(entry_panes(&closed).is_empty());
        let merged = json!({ "repo": "acme/api", "pr": 4, "state": "merged", "pane": "api-4" });
        assert!(entry_panes(&merged).is_empty());
    }

    #[test]
    fn reads_a_ledger() {
        let dir = std::env::temp_dir().join(format!("rex-inbox-ledger-{}", std::process::id()));
        let repo = dir.join("repos/acme/api");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(dir.join("state.json"), "{}").unwrap();
        let e = json!({ "repo": "acme/api", "pr": 12, "state": "reviewing", "session_id": "s1",
                        "reviewers": { "codex": { "pane": "api-12" } }, "checker_pane": "api-12-check" });
        std::fs::write(repo.join("pr-12.json"), e.to_string()).unwrap();
        std::fs::write(repo.join("notes.txt"), "not an entry").unwrap();
        let panes = read_panes(&dir);
        assert_eq!(panes.get(&("s1".into(), "api-12".into())).map(String::as_str), Some("api#12 · codex reviewer"));
        assert_eq!(panes.get(&("s1".into(), "api-12-check".into())).map(String::as_str), Some("api#12 · checker"));
        assert_eq!(panes.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
