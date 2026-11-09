//! The hub: follows every Rex host, keeps alerts in step with program status,
//! and answers requests on a unix socket.

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Semaphore, broadcast};
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep, sleep_until};

use crate::api::{Request, Response};
use crate::mirror::{HostMirror, Pane, Record, SessionSnapshot, Session, TERMINAL, Window};
use crate::rex::Rex;
use crate::store::{Alert, AlertFields, Message, Store};
use crate::time;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HubEvent {
    AlertOpened { alert: Alert },
    AlertUpdated { alert: Alert },
    AlertResolved { alert: Alert },
    MessageNew { message: Message },
    MessageUpdated { message: Message },
    HostConnected { host: String },
    HostDisconnected { host: String, error: Option<String> },
}

pub struct Core {
    pub store: Store,
    pub hosts: BTreeMap<String, HostMirror>,
    pub rexes: BTreeMap<String, Rex>,
    pub events: broadcast::Sender<HubEvent>,
    pub started_at: String,
    /// Apps watching with `notifier: true` post notifications themselves,
    /// so the hub's own stay quiet while any is connected.
    pub notifiers: usize,
    /// The Mac's Tailscale user: the only one the web app answers.
    pub owner: Option<String>,
    /// The web app's address on the tailnet, when Tailscale is up.
    pub web_url: Option<String>,
    /// Which panes rex-reviewer runs, when it runs alongside.
    pub reviewer: crate::reviewer::Ledger,
    /// Wakes the fleet to read `rex hosts` again now.
    pub fleet: Arc<tokio::sync::Notify>,
}

#[derive(Clone)]
pub struct Shared(Arc<Mutex<Core>>);

impl Shared {
    pub fn lock(&self) -> MutexGuard<'_, Core> {
        // A panic while holding the lock leaves the data usable.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Core {
    pub fn emit(&self, e: HubEvent) {
        let _ = self.events.send(e);
    }

    pub fn rex(&self, host: &str) -> Result<Rex> {
        self.rexes.get(host).cloned().ok_or_else(|| anyhow!("unknown host {host}"))
    }

    /// Bring the alerts of some panes in line with their records.
    pub fn reconcile<I: IntoIterator<Item = String>>(&mut self, host: &str, blocks: I) {
        for block in blocks {
            if let Err(e) = self.reconcile_block(host, &block) {
                say!("alerts for {host} {block}: {e:#}");
            }
        }
    }

    fn reconcile_block(&mut self, host: &str, block: &str) -> Result<()> {
        let mirror = self.hosts.get(host);
        let pane = mirror.and_then(|m| m.panes.get(block));
        let wanted = match (mirror, pane) {
            (Some(m), Some(p)) => {
                let managed = self.reviewer.managed(m, p);
                wanted_alerts(m, p, managed.as_deref())
            }
            _ => Vec::new(),
        };
        let closed = pane.is_none();
        let live = self.store.live_alerts_for_block(host, block)?;
        let keys: HashSet<&str> = wanted.iter().map(|f| f.key.as_str()).collect();
        for a in &live {
            if !keys.contains(a.key.as_str()) {
                let reason = if closed { "pane closed" } else { "status changed" };
                if let Some(a) = self.store.resolve_alert(a.id, reason)? {
                    self.emit(HubEvent::AlertResolved { alert: a });
                }
            }
        }
        for f in wanted {
            match live.iter().find(|a| a.key == f.key) {
                Some(a) => {
                    if let Some(a) = self.store.update_alert(a, &f)? {
                        self.emit(HubEvent::AlertUpdated { alert: a });
                    }
                }
                None => {
                    let a = self.store.insert_alert(&f)?;
                    self.store.log("alert_opened", &json!({ "id": a.id, "key": a.key, "msg": a.msg }))?;
                    self.emit(HubEvent::AlertOpened { alert: a });
                }
            }
        }
        Ok(())
    }

    /// After a snapshot: every pane with records or live alerts.
    fn reconcile_host(&mut self, host: &str) {
        let mut blocks: BTreeSet<String> = match self.store.live_alerts_for_host(host) {
            Ok(v) => v.into_iter().map(|a| a.block_id).collect(),
            Err(e) => {
                say!("alerts for {host}: {e:#}");
                BTreeSet::new()
            }
        };
        if let Some(m) = self.hosts.get(host) {
            blocks.extend(m.panes.values().filter(|p| !p.records.is_empty()).map(|p| p.block_id.clone()));
        }
        self.reconcile(host, blocks);
    }

    pub fn pane(&self, host: &str, block: &str) -> Option<&Pane> {
        self.hosts.get(host)?.panes.get(block)
    }

    /// What a pane does for rex-reviewer, when rex-reviewer runs it.
    pub fn managed(&mut self, host: &str, block: &str) -> Option<String> {
        let Core { hosts, reviewer, .. } = self;
        let m = hosts.get(host)?;
        reviewer.managed(m, m.panes.get(block)?)
    }
}

/// The alerts a pane's records call for. Root records alert on blocked,
/// error, and done. Child records (such as Claude Code's subagents) alert
/// only on blocked and error: a subagent finishing is not news. Nor is an
/// agent finishing in a pane another tool runs (`managed`, what the pane
/// does for it): that tool moves on by itself.
fn wanted_alerts(m: &HostMirror, p: &Pane, managed: Option<&str>) -> Vec<AlertFields> {
    p.records
        .values()
        .filter_map(|r| {
            let category = match r.state.as_str() {
                "blocked" => "needs_you",
                "error" => "failed",
                "done" if r.is_root() && managed.is_none() => "finished",
                _ => return None,
            };
            let app = r.effective_app.clone().or_else(|| r.app.clone()).or_else(|| p.app().map(str::to_string));
            let item = crate::reviewer::is_item(app.as_deref(), &r.id);
            Some(AlertFields {
                key: format!("{}|{}|{}|{}", p.host, p.block_id, r.id, r.state),
                category: category.to_string(),
                host: p.host.clone(),
                session_id: p.session_id.clone(),
                block_id: p.block_id.clone(),
                window_id: p.window_id.clone(),
                record_id: r.id.clone(),
                state: r.state.clone(),
                kind: if r.state == "blocked" { r.kind.clone() } else { None },
                app,
                title: r.title.clone(),
                msg: r.msg.clone(),
                record_updated_at: r.updated_at.clone(),
                session_label: m.session_label(&p.session_id).map(str::to_string),
                window_label: m.window_label(&p.session_id, p.window_id.as_deref()).map(str::to_string),
                pane_label: Some(p.label.clone()).filter(|l| !l.is_empty()),
                cwd: if item { None } else { p.cwd.clone() },
                subject: if item { crate::reviewer::item_subject(&r.id) } else { managed.map(str::to_string) },
            })
        })
        .collect()
}

// ---- following a host ----

/// Follow one host forever: snapshot, then apply events; reconnect on loss.
pub async fn watch_host(shared: Shared, rex: Rex) {
    let host = rex.name().to_string();
    let mut backoff = Duration::from_secs(1);
    loop {
        let started = Instant::now();
        let err = match follow(&shared, &rex).await {
            Ok(()) => None,
            Err(e) => Some(format!("{e:#}")),
        };
        {
            let mut core = shared.lock();
            let m = core.hosts.entry(host.clone()).or_insert_with(|| HostMirror::new(&host));
            m.connected = false;
            // Rex's own words, without the command that met them.
            m.error = err.as_deref().map(|e| e.rsplit("Error: ").next().unwrap_or(e).trim().to_string());
            core.emit(HubEvent::HostDisconnected { host: host.clone(), error: err.clone() });
        }
        say!("lost {host}: {}", err.as_deref().unwrap_or("event stream ended"));
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

async fn follow(shared: &Shared, rex: &Rex) -> Result<()> {
    let host = rex.name().to_string();
    // Start the stream before the snapshot, so nothing falls between them.
    let mut child = rex.events()?;
    let stdout = child.stdout.take().context("events stdout")?;
    let mut lines = BufReader::new(stdout).lines();

    let snap = snapshot(rex, None).await?;
    {
        let mut core = shared.lock();
        let mut m = HostMirror::new(&host);
        m.connected = true;
        apply_snapshot(&mut m, snap);
        core.hosts.insert(host.clone(), m);
        core.reconcile_host(&host);
        core.emit(HubEvent::HostConnected { host: host.clone() });
    }
    say!("following {host}");

    let mut pending: BTreeSet<String> = BTreeSet::new();
    let mut pending_all = false;
    let mut deadline: Option<Instant> = None;
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else { bail!("event stream ended") };
                let Ok(ev) = serde_json::from_str::<Value>(&line) else { continue };
                let mut core = shared.lock();
                let Some(m) = core.hosts.get_mut(&host) else { continue };
                let changes = m.apply_event(&ev);
                core.reconcile(&host, changes.panes);
                if !changes.resync.is_empty() || changes.resync_all {
                    pending.extend(changes.resync);
                    pending_all |= changes.resync_all;
                    deadline.get_or_insert(Instant::now() + Duration::from_millis(300));
                }
            }
            _ = async { sleep_until(deadline.unwrap()).await }, if deadline.is_some() => {
                deadline = None;
                let only = if std::mem::take(&mut pending_all) { None } else { Some(std::mem::take(&mut pending)) };
                pending.clear();
                if let Err(e) = resync(shared, rex, only).await {
                    say!("resync {host}: {e:#}");
                }
            }
        }
    }
}

/// One pane's status in a snapshot: `(session, block, records, process, pwd)`.
type PaneStatus = (String, String, Vec<Record>, Option<Value>, Option<Value>);

struct HostSnapshot {
    /// `None` when only some sessions were fetched.
    all_sessions: Option<BTreeSet<String>>,
    sessions: Vec<SessionSnapshot>,
    statuses: Vec<PaneStatus>,
}

fn apply_snapshot(m: &mut HostMirror, snap: HostSnapshot) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    if let Some(live) = &snap.all_sessions {
        changed.extend(m.retain_sessions(live).panes);
    }
    for s in snap.sessions {
        changed.extend(m.apply_session(s).panes);
    }
    for (sid, bid, records, process, pwd) in snap.statuses {
        m.apply_pane_status(&sid, &bid, records, process.as_ref(), pwd.as_ref());
        changed.insert(bid);
    }
    changed
}

/// Fetch sessions (all, or `only`), their layouts, and the status of their
/// terminals. With `only`, status is fetched just for panes new to the mirror.
async fn snapshot(rex: &Rex, only: Option<(BTreeSet<String>, BTreeSet<String>)>) -> Result<HostSnapshot> {
    let (wanted, all_sessions, known) = match only {
        Some((sessions, known)) => (sessions.into_iter().map(|s| (s, None)).collect::<Vec<_>>(), None, Some(known)),
        None => {
            let list = rex.api(None, "session.list", &json!({})).await?;
            let sessions: Vec<(String, Option<String>)> = list["sessions"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|s| Some((s["session_id"].as_str()?.to_string(), s["label"].as_str().map(str::to_string))))
                        .collect()
                })
                .unwrap_or_default();
            let ids = sessions.iter().map(|s| s.0.clone()).collect();
            (sessions, Some(ids), None)
        }
    };

    let limit = Arc::new(Semaphore::new(8));
    let mut set = JoinSet::new();
    for (sid, _) in wanted {
        let rex = rex.clone();
        let limit = limit.clone();
        set.spawn(async move {
            let _permit = limit.acquire_owned().await;
            fetch_session(&rex, &sid).await
        });
    }
    let mut sessions = Vec::new();
    while let Some(r) = set.join_next().await {
        match r? {
            Ok(s) => sessions.push(s),
            // A session can vanish between list and view.
            Err(e) => say!("{e:#}"),
        }
    }

    let mut set = JoinSet::new();
    for s in &sessions {
        for (bid, _, creator, _) in &s.blocks {
            if creator != TERMINAL || known.as_ref().is_some_and(|k| k.contains(bid)) {
                continue;
            }
            let (rex, sid, bid, limit) = (rex.clone(), s.session.session_id.clone(), bid.clone(), limit.clone());
            set.spawn(async move {
                let _permit = limit.acquire_owned().await;
                let status = rex.block_call(&sid, &bid, "program_status", &json!({})).await;
                let process = rex.block_call(&sid, &bid, "process", &json!({})).await.ok();
                let pwd = rex.block_call(&sid, &bid, "pwd", &json!({})).await.ok();
                (sid, bid, status, process, pwd)
            });
        }
    }
    let mut statuses = Vec::new();
    while let Some(r) = set.join_next().await {
        let (sid, bid, status, process, pwd) = r?;
        let Ok(status) = status else { continue };
        let records: Vec<Record> = status["records"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect())
            .unwrap_or_default();
        statuses.push((sid, bid, records, process, pwd));
    }
    Ok(HostSnapshot { all_sessions, sessions, statuses })
}

async fn fetch_session(rex: &Rex, sid: &str) -> Result<SessionSnapshot> {
    let view = rex.api(Some(sid), "session.view", &json!({})).await?;
    let blocks = rex.api(Some(sid), "session.list_blocks", &json!({})).await?;
    let windows = view["windows"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|w| {
                    Some(Window {
                        window_id: w["window_id"].as_str()?.to_string(),
                        label: w["label"].as_str().unwrap_or_default().to_string(),
                        focused_block_id: w["focused_block_id"].as_str().map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let blocks = blocks["blocks"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|b| {
                    Some((
                        b["block_id"].as_str()?.to_string(),
                        b["label"].as_str().unwrap_or_default().to_string(),
                        b["creator_name"].as_str().unwrap_or_default().to_string(),
                        b["window_id"].as_str().map(str::to_string),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(SessionSnapshot {
        session: Session {
            session_id: sid.to_string(),
            label: view["label"].as_str().unwrap_or_default().to_string(),
            active_window_id: view["active_window_id"].as_str().map(str::to_string),
            windows,
        },
        blocks,
    })
}

fn known_panes(shared: &Shared, host: &str) -> BTreeSet<String> {
    shared.lock().hosts.get(host).map(|m| m.panes.keys().cloned().collect()).unwrap_or_default()
}

fn retain_sessions(shared: &Shared, host: &str, ids: &BTreeSet<String>) {
    let mut core = shared.lock();
    if let Some(m) = core.hosts.get_mut(host) {
        let changed = m.retain_sessions(ids).panes;
        core.reconcile(host, changed);
    }
}

fn apply_partial(shared: &Shared, host: &str, snap: HostSnapshot) {
    let mut core = shared.lock();
    if let Some(m) = core.hosts.get_mut(host) {
        let changed = apply_snapshot(m, snap);
        core.reconcile(host, changed);
    }
}

/// Fetch some sessions again (or the whole list, with `None`). Status is
/// fetched only for panes new to the mirror; events keep the rest current.
async fn resync(shared: &Shared, rex: &Rex, only: Option<BTreeSet<String>>) -> Result<()> {
    let host = rex.name();
    let known = known_panes(shared, host);
    let sessions = match only {
        Some(s) => s,
        None => {
            let list = rex.api(None, "session.list", &json!({})).await?;
            let ids: BTreeSet<String> = list["sessions"]
                .as_array()
                .map(|a| a.iter().filter_map(|s| s["session_id"].as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            retain_sessions(shared, host, &ids);
            ids
        }
    };
    let snap = snapshot(rex, Some((sessions, known))).await?;
    apply_partial(shared, host, snap);
    Ok(())
}

// ---- the socket ----

pub async fn run(socket: PathBuf, db: PathBuf) -> Result<()> {
    let store = Store::open(&db)?;
    let (events, _) = broadcast::channel(256);
    let shared = Shared(Arc::new(Mutex::new(Core {
        store,
        hosts: BTreeMap::new(),
        rexes: BTreeMap::new(),
        events,
        started_at: time::now(),
        notifiers: 0,
        owner: None,
        web_url: None,
        reviewer: crate::reviewer::Ledger::new(crate::reviewer::state_dir()),
        fleet: Arc::new(tokio::sync::Notify::new()),
    })));
    let listener = bind(&socket).await?;
    say!("hub listening on {}", socket.display());
    tokio::spawn(crate::fleet::run(shared.clone()));
    tokio::spawn(crate::deliver::run(shared.clone()));
    tokio::spawn(crate::notify::run(shared.clone()));
    tokio::spawn(crate::push::run(shared.clone()));
    if let Some(addr) = crate::web::addr() {
        tokio::spawn(crate::web::watch_owner(shared.clone()));
        let web = shared.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::web::serve(web, addr).await {
                say!("web app: {e:#}");
            }
        });
    }
    loop {
        let (stream, _) = listener.accept().await?;
        let shared = shared.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_conn(shared, stream).await {
                say!("connection: {e:#}");
            }
        });
    }
}

async fn bind(path: &Path) -> Result<UnixListener> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        if UnixStream::connect(path).await.is_ok() {
            bail!("another hub is already listening on {}", path.display());
        }
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path).with_context(|| format!("binding {}", path.display()))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

async fn serve_conn(shared: Shared, stream: UnixStream) -> Result<()> {
    let (r, mut w) = stream.into_split();
    let mut lines = BufReader::new(r).lines();
    let Some(line) = lines.next_line().await? else { return Ok(()) };
    let req: Request = match serde_json::from_str(&line) {
        Ok(r) => r,
        Err(e) => {
            let resp = Response::err(format!("bad request: {e}"));
            w.write_all(format!("{}\n", serde_json::to_string(&resp)?).as_bytes()).await?;
            return Ok(());
        }
    };
    if req.method == "hub.stop" {
        w.write_all(format!("{}\n", serde_json::to_string(&Response::ok(json!({ "stopping": true })))?).as_bytes()).await?;
        w.flush().await?;
        say!("stopping on request");
        std::process::exit(0);
    }
    if req.method == "watch" {
        struct Notifier(Option<Shared>);
        impl Drop for Notifier {
            fn drop(&mut self) {
                if let Some(s) = &self.0 {
                    s.lock().notifiers -= 1;
                }
            }
        }
        let _notifier = if req.params["notifier"].as_bool() == Some(true) {
            shared.lock().notifiers += 1;
            Notifier(Some(shared.clone()))
        } else {
            Notifier(None)
        };
        let mut rx = shared.lock().events.subscribe();
        w.write_all(format!("{}\n", serde_json::to_string(&Response::ok(json!({ "watching": true })))?).as_bytes()).await?;
        loop {
            tokio::select! {
                ev = rx.recv() => match ev {
                    Ok(ev) => {
                        let line = format!("{}\n", serde_json::to_string(&ev)?);
                        if w.write_all(line.as_bytes()).await.is_err() {
                            return Ok(());
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return Ok(()),
                },
                // The watcher hung up: stop now, not at the next event.
                line = lines.next_line() => if !matches!(line, Ok(Some(_))) { return Ok(()) },
            }
        }
    }
    let resp = match crate::methods::dispatch(&shared, &req.method, req.params).await {
        Ok(v) => Response::ok(v),
        Err(e) => Response::err(format!("{e:#}")),
    };
    w.write_all(format!("{}\n", serde_json::to_string(&resp)?).as_bytes()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(state: &str) -> Record {
        Record { state: state.into(), app: Some("claude-code".into()), ..Default::default() }
    }

    #[test]
    fn panes_another_tool_runs_raise_no_finished() {
        let mut m = HostMirror::new("local");
        m.apply_pane_status("s1", "b1", vec![rec("done")], None, None);
        let p = &m.panes["b1"];
        assert_eq!(wanted_alerts(&m, p, None).len(), 1);
        assert!(wanted_alerts(&m, p, Some("api#12 · codex reviewer")).is_empty());

        m.apply_pane_status("s1", "b1", vec![rec("blocked")], None, None);
        let p = &m.panes["b1"];
        let w = wanted_alerts(&m, p, Some("api#12 · codex reviewer"));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].category, "needs_you");
        assert_eq!(w[0].subject.as_deref(), Some("api#12 · codex reviewer"));

        m.apply_pane_status("s1", "b1", vec![rec("error")], None, None);
        let p = &m.panes["b1"];
        assert_eq!(wanted_alerts(&m, p, Some("api#12 · codex reviewer"))[0].category, "failed");
    }

    #[test]
    fn agents_beside_a_reviewer_elsewhere_are_its() {
        let mut ledger = crate::reviewer::Ledger::new(std::env::temp_dir().join("rex-inbox-no-ledger-here"));
        let mut m = HostMirror::new("studio");
        let watcher = Record { state: "idle".into(), app: Some("rex-reviewer".into()), ..Default::default() };
        m.apply_pane_status("s1", "w", vec![watcher], None, None);
        m.apply_pane_status("s1", "a", vec![rec("working")], None, None);
        m.apply_pane_status("s2", "b", vec![rec("working")], None, None);
        assert!(ledger.managed(&m, &m.panes["a"]).is_some());
        assert!(ledger.managed(&m, &m.panes["b"]).is_none());
        assert!(ledger.managed(&m, &m.panes["w"]).is_none());
    }
}
