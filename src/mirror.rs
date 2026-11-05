//! A live picture of one Rex server: its sessions, windows, panes, and every
//! pane's program status records (OSC 7501).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const TERMINAL: &str = "com.superlogical.terminal";

/// One program status record, as Rex reports it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// Empty for the root record.
    #[serde(default)]
    pub id: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

impl Record {
    pub fn is_root(&self) -> bool {
        self.id.is_empty()
    }
}

/// How urgent a state is, for picking a pane's headline state.
pub fn urgency(state: &str) -> u8 {
    match state {
        "blocked" => 5,
        "error" => 4,
        "working" => 3,
        "done" => 2,
        "idle" => 1,
        _ => 0,
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Pane {
    pub host: String,
    pub session_id: String,
    pub block_id: String,
    pub window_id: Option<String>,
    pub label: String,
    pub creator: String,
    /// The folder to show: what the program reported (OSC 7), else the
    /// foreground process's working directory.
    pub cwd: Option<String>,
    #[serde(skip)]
    reported_cwd: Option<String>,
    #[serde(skip)]
    process_cwd: Option<String>,
    /// The foreground process's name, such as `claude` or `zsh`.
    pub process: Option<String>,
    pub records: BTreeMap<String, Record>,
}

impl Pane {
    fn new(host: &str, session_id: &str, block_id: &str) -> Self {
        Pane {
            host: host.to_string(),
            session_id: session_id.to_string(),
            block_id: block_id.to_string(),
            window_id: None,
            label: String::new(),
            creator: TERMINAL.to_string(),
            cwd: None,
            reported_cwd: None,
            process_cwd: None,
            process: None,
            records: BTreeMap::new(),
        }
    }

    fn refresh_cwd(&mut self) {
        self.cwd = self.reported_cwd.clone().or_else(|| self.process_cwd.clone());
    }

    pub fn root(&self) -> Option<&Record> {
        self.records.get("")
    }

    /// The agent or program that reports status here, such as `claude-code`.
    pub fn app(&self) -> Option<&str> {
        self.root()
            .and_then(|r| r.effective_app.as_deref().or(r.app.as_deref()))
            .or_else(|| self.records.values().find_map(|r| r.effective_app.as_deref().or(r.app.as_deref())))
    }

    /// The most urgent state of any record, or `None` with no records.
    pub fn headline(&self) -> Option<&Record> {
        self.records.values().max_by_key(|r| (urgency(&r.state), r.is_root()))
    }

    pub fn is_agent(&self) -> bool {
        matches!(self.app(), Some(a) if crate::agents::is_known(a))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Window {
    pub window_id: String,
    pub label: String,
    pub focused_block_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Session {
    pub session_id: String,
    pub label: String,
    pub active_window_id: Option<String>,
    pub windows: Vec<Window>,
}

/// What a session snapshot holds, fetched outside the hub's lock.
#[derive(Debug)]
pub struct SessionSnapshot {
    pub session: Session,
    /// `(block_id, label, creator, window_id)`.
    pub blocks: Vec<(String, String, String, Option<String>)>,
}

/// What an event changed, so the hub knows what to recompute.
#[derive(Debug, Default)]
pub struct Changes {
    /// Panes whose records changed (or that closed).
    pub panes: BTreeSet<String>,
    /// Sessions whose layout or labels need fetching again.
    pub resync: BTreeSet<String>,
    /// The session list changed.
    pub resync_all: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct HostMirror {
    pub host: String,
    pub connected: bool,
    pub sessions: BTreeMap<String, Session>,
    pub panes: BTreeMap<String, Pane>,
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

impl HostMirror {
    pub fn new(host: &str) -> Self {
        HostMirror { host: host.to_string(), ..Default::default() }
    }

    pub fn session_label(&self, session_id: &str) -> Option<&str> {
        self.sessions.get(session_id).map(|s| s.label.as_str())
    }

    pub fn window_label(&self, session_id: &str, window_id: Option<&str>) -> Option<&str> {
        let w = window_id?;
        self.sessions.get(session_id)?.windows.iter().find(|x| x.window_id == w).map(|x| x.label.as_str())
    }

    /// Replace one session's layout, keeping the records of panes that stay.
    pub fn apply_session(&mut self, snap: SessionSnapshot) -> Changes {
        let mut changes = Changes::default();
        let sid = snap.session.session_id.clone();
        let keep: BTreeSet<&String> = snap.blocks.iter().map(|b| &b.0).collect();
        let gone: Vec<String> = self
            .panes
            .values()
            .filter(|p| p.session_id == sid && !keep.contains(&p.block_id))
            .map(|p| p.block_id.clone())
            .collect();
        for b in gone {
            self.panes.remove(&b);
            changes.panes.insert(b);
        }
        for (block_id, label, creator, window_id) in snap.blocks {
            let host = self.host.clone();
            let p = self.panes.entry(block_id.clone()).or_insert_with(|| Pane::new(&host, &sid, &block_id));
            let moved = p.label != label || p.window_id != window_id;
            p.label = label;
            p.creator = creator;
            p.window_id = window_id;
            if moved && !p.records.is_empty() {
                changes.panes.insert(block_id);
            }
        }
        self.sessions.insert(sid, snap.session);
        changes
    }

    /// Drop sessions that no longer exist.
    pub fn retain_sessions(&mut self, live: &BTreeSet<String>) -> Changes {
        let mut changes = Changes::default();
        self.sessions.retain(|id, _| live.contains(id));
        let gone: Vec<String> =
            self.panes.values().filter(|p| !live.contains(&p.session_id)).map(|p| p.block_id.clone()).collect();
        for b in gone {
            self.panes.remove(&b);
            changes.panes.insert(b);
        }
        changes
    }

    /// Set a pane's records, process and reported folder from a snapshot.
    pub fn apply_pane_status(
        &mut self,
        session_id: &str,
        block_id: &str,
        records: Vec<Record>,
        process: Option<&Value>,
        pwd: Option<&Value>,
    ) {
        let host = self.host.clone();
        let p = self.panes.entry(block_id.to_string()).or_insert_with(|| Pane::new(&host, session_id, block_id));
        p.records = records.into_iter().map(|r| (r.id.clone(), r)).collect();
        if let Some(fg) = process.and_then(|v| v.get("foreground")) {
            set_process(p, fg);
        }
        if let Some(path) = pwd.and_then(|v| v["pwd"].as_str()).filter(|s| !s.is_empty()) {
            p.reported_cwd = Some(path_of(path));
            p.refresh_cwd();
        }
    }

    /// Apply one line of `rex events --all --json`.
    pub fn apply_event(&mut self, ev: &Value) -> Changes {
        let mut changes = Changes::default();
        let typ = ev["type"].as_str().unwrap_or_default();
        let payload = &ev["payload"];
        match typ {
            "session_event" => {
                let Some(sid) = s(&payload["session_id"]) else { return changes };
                let inner = &payload["event"];
                let p = &inner["payload"];
                match inner["type"].as_str().unwrap_or_default() {
                    "block_event" => self.apply_block_event(&sid, p, &mut changes),
                    "block_closed" => {
                        if let Some(b) = s(&p["block_id"]) {
                            self.panes.remove(&b);
                            changes.panes.insert(b);
                        }
                    }
                    "block_created" | "session_view_changed" | "session_label_changed" | "window_created"
                    | "window_closed" | "window_label_changed" | "active_window_changed" => {
                        changes.resync.insert(sid);
                    }
                    "session_destroyed" => changes.resync_all = true,
                    _ => {}
                }
            }
            t if t.starts_with("session_") => changes.resync_all = true,
            _ => {}
        }
        changes
    }

    fn apply_block_event(&mut self, sid: &str, p: &Value, changes: &mut Changes) {
        let Some(block) = s(&p["block_id"]) else { return };
        if p["creator_name"].as_str() != Some(TERMINAL) {
            return;
        }
        let name = p["name"].as_str().unwrap_or_default();
        let body = &p["payload"];
        let host = self.host.clone();
        let known = self.panes.contains_key(&block);
        let pane = self.panes.entry(block.clone()).or_insert_with(|| Pane::new(&host, sid, &block));
        if !known {
            changes.resync.insert(sid.to_string());
        }
        match name {
            "program_status_changed" => {
                let Ok(mut rec) = serde_json::from_value::<Record>(body["record"].clone()) else { return };
                rec.id = body["id"].as_str().unwrap_or_default().to_string();
                // Events queued while a snapshot was taken can be older than it.
                if let Some(old) = pane.records.get(&rec.id) {
                    let t = |r: &Record| r.updated_at.as_deref().and_then(crate::time::parse);
                    if matches!((t(old), t(&rec)), (Some(a), Some(b)) if b < a) {
                        return;
                    }
                }
                if pane.records.get(&rec.id) != Some(&rec) {
                    pane.records.insert(rec.id.clone(), rec);
                    changes.panes.insert(block);
                }
            }
            "program_status_removed" => {
                // Rex lists every record it removed, children included, so
                // remove exactly those. No list means all of them.
                match body["ids"].as_array() {
                    Some(ids) => {
                        for id in ids.iter().filter_map(Value::as_str) {
                            pane.records.remove(id);
                        }
                    }
                    None => pane.records.clear(),
                }
                changes.panes.insert(block);
            }
            // A new program reports its own folder, if any.
            "child_exited" => {
                pane.reported_cwd = None;
                pane.refresh_cwd();
            }
            "process_changed" => {
                if let Some(fg) = body.get("foreground") {
                    set_process(pane, fg);
                }
            }
            "pwd_changed" => {
                if let Some(pwd) = s(&body["pwd"]) {
                    pane.reported_cwd = Some(path_of(&pwd));
                    pane.refresh_cwd();
                }
            }
            _ => {}
        }
    }
}

fn set_process(p: &mut Pane, fg: &Value) {
    if fg.is_null() {
        return;
    }
    p.process = s(&fg["name"]);
    if let Some(cwd) = s(&fg["cwd"]) {
        p.process_cwd = Some(cwd);
        p.refresh_cwd();
    }
}

/// Shells report their folder (OSC 7) as `file://host/path`, percent-encoded.
fn path_of(pwd: &str) -> String {
    let Some(rest) = pwd.strip_prefix("file://") else { return pwd.to_string() };
    let path = match rest.find('/') {
        Some(i) => &rest[i..],
        None => return pwd.to_string(),
    };
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&path[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::path_of;

    #[test]
    fn osc7_paths() {
        assert_eq!(path_of("file://localhost/Users/j/code/acme-api"), "/Users/j/code/acme-api");
        assert_eq!(path_of("file://mac.local/Users/j/My%20Code"), "/Users/j/My Code");
        assert_eq!(path_of("/already/a/path"), "/already/a/path");
        assert_eq!(path_of("file://host/trailing%2"), "/trailing%2");
    }
}
