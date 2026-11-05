//! SQLite storage for alerts, messages, and an event log.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use std::path::Path;

use crate::time::now;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS seq (id INTEGER PRIMARY KEY AUTOINCREMENT);

-- One alert per occurrence of an alert-worthy program status record.
CREATE TABLE IF NOT EXISTS alerts (
  id INTEGER PRIMARY KEY,
  key TEXT NOT NULL,                 -- host|block|record|state
  status TEXT NOT NULL,              -- open | dismissed | snoozed | resolved
  category TEXT NOT NULL,            -- needs_you | failed | finished
  host TEXT NOT NULL,
  session_id TEXT NOT NULL,
  block_id TEXT NOT NULL,
  window_id TEXT,
  record_id TEXT NOT NULL,
  state TEXT NOT NULL,
  kind TEXT,                         -- permission | question | auth
  app TEXT,
  title TEXT,
  msg TEXT,
  record_updated_at TEXT,
  session_label TEXT,
  window_label TEXT,
  pane_label TEXT,
  cwd TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  snooze_until TEXT,
  resolved_at TEXT,
  resolved_reason TEXT
);
CREATE INDEX IF NOT EXISTS alerts_live ON alerts(status, key);
CREATE INDEX IF NOT EXISTS alerts_block ON alerts(host, block_id);

CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY,
  thread_id INTEGER NOT NULL,
  reply_to INTEGER,
  from_addr TEXT NOT NULL,
  from_host TEXT, from_session TEXT, from_block TEXT,
  to_addr TEXT NOT NULL,
  to_host TEXT, to_session TEXT, to_block TEXT,
  subject TEXT,
  body TEXT NOT NULL,
  status TEXT NOT NULL,              -- to an agent: queued | delivered | failed | cancelled
                                     -- to me: unread | read | done
  created_at TEXT NOT NULL,
  delivered_at TEXT,
  read_at TEXT,
  done_at TEXT,
  attempts INTEGER NOT NULL DEFAULT 0,
  error TEXT
);
CREATE INDEX IF NOT EXISTS messages_to ON messages(to_addr, status);
CREATE INDEX IF NOT EXISTS messages_block ON messages(to_host, to_block, status);

CREATE TABLE IF NOT EXISTS log (
  id INTEGER PRIMARY KEY,
  at TEXT NOT NULL,
  kind TEXT NOT NULL,
  data TEXT NOT NULL
);
"#;

#[derive(Clone, Debug, Serialize)]
pub struct Alert {
    pub id: i64,
    pub key: String,
    pub status: String,
    pub category: String,
    pub host: String,
    pub session_id: String,
    pub block_id: String,
    pub window_id: Option<String>,
    pub record_id: String,
    pub state: String,
    pub kind: Option<String>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub msg: Option<String>,
    pub record_updated_at: Option<String>,
    pub session_label: Option<String>,
    pub window_label: Option<String>,
    pub pane_label: Option<String>,
    pub cwd: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub snooze_until: Option<String>,
    pub resolved_at: Option<String>,
    pub resolved_reason: Option<String>,
}

/// The fields of an alert that come from the live record and pane.
#[derive(Clone, Debug)]
pub struct AlertFields {
    pub key: String,
    pub category: String,
    pub host: String,
    pub session_id: String,
    pub block_id: String,
    pub window_id: Option<String>,
    pub record_id: String,
    pub state: String,
    pub kind: Option<String>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub msg: Option<String>,
    pub record_updated_at: Option<String>,
    pub session_label: Option<String>,
    pub window_label: Option<String>,
    pub pane_label: Option<String>,
    pub cwd: Option<String>,
}

const ALERT_COLS: &str = "id, key, status, category, host, session_id, block_id, window_id, record_id, state, kind, app, \
     title, msg, record_updated_at, session_label, window_label, pane_label, cwd, created_at, updated_at, \
     snooze_until, resolved_at, resolved_reason";

fn alert_row(r: &Row) -> rusqlite::Result<Alert> {
    Ok(Alert {
        id: r.get(0)?,
        key: r.get(1)?,
        status: r.get(2)?,
        category: r.get(3)?,
        host: r.get(4)?,
        session_id: r.get(5)?,
        block_id: r.get(6)?,
        window_id: r.get(7)?,
        record_id: r.get(8)?,
        state: r.get(9)?,
        kind: r.get(10)?,
        app: r.get(11)?,
        title: r.get(12)?,
        msg: r.get(13)?,
        record_updated_at: r.get(14)?,
        session_label: r.get(15)?,
        window_label: r.get(16)?,
        pane_label: r.get(17)?,
        cwd: r.get(18)?,
        created_at: r.get(19)?,
        updated_at: r.get(20)?,
        snooze_until: r.get(21)?,
        resolved_at: r.get(22)?,
        resolved_reason: r.get(23)?,
    })
}

/// Live means not resolved: open, dismissed, or snoozed.
const LIVE: &str = "status != 'resolved'";

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }

    /// The next id, shared by alerts and messages so `show N` is never ambiguous.
    pub fn next_id(&self) -> Result<i64> {
        self.conn.execute("INSERT INTO seq DEFAULT VALUES", [])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn log(&self, kind: &str, data: &serde_json::Value) -> Result<()> {
        self.conn.execute("INSERT INTO log (at, kind, data) VALUES (?1, ?2, ?3)", params![now(), kind, data.to_string()])?;
        Ok(())
    }

    // ---- alerts ----

    pub fn alert(&self, id: i64) -> Result<Option<Alert>> {
        let sql = format!("SELECT {ALERT_COLS} FROM alerts WHERE id = ?1");
        Ok(self.conn.query_row(&sql, [id], alert_row).optional()?)
    }

    pub fn live_alert_by_key(&self, key: &str) -> Result<Option<Alert>> {
        let sql = format!("SELECT {ALERT_COLS} FROM alerts WHERE key = ?1 AND {LIVE} ORDER BY id DESC LIMIT 1");
        Ok(self.conn.query_row(&sql, [key], alert_row).optional()?)
    }

    pub fn live_alerts_for_block(&self, host: &str, block: &str) -> Result<Vec<Alert>> {
        let sql = format!("SELECT {ALERT_COLS} FROM alerts WHERE host = ?1 AND block_id = ?2 AND {LIVE}");
        let mut st = self.conn.prepare(&sql)?;
        Ok(st.query_map(params![host, block], alert_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn live_alerts_for_host(&self, host: &str) -> Result<Vec<Alert>> {
        let sql = format!("SELECT {ALERT_COLS} FROM alerts WHERE host = ?1 AND {LIVE}");
        let mut st = self.conn.prepare(&sql)?;
        Ok(st.query_map([host], alert_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Alerts in the inbox: open ones, plus snoozed ones whose time has come.
    /// `all` adds dismissed and snoozed; `history` adds resolved ones too.
    pub fn list_alerts(&self, all: bool, history: bool, limit: i64) -> Result<Vec<Alert>> {
        let filter = if history {
            "1=1"
        } else if all {
            LIVE
        } else {
            "(status = 'open' OR (status = 'snoozed' AND snooze_until <= strftime('%Y-%m-%dT%H:%M:%SZ','now')))"
        };
        let sql = format!("SELECT {ALERT_COLS} FROM alerts WHERE {filter} ORDER BY id DESC LIMIT ?1");
        let mut st = self.conn.prepare(&sql)?;
        Ok(st.query_map([limit], alert_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn insert_alert(&self, f: &AlertFields) -> Result<Alert> {
        let id = self.next_id()?;
        let t = now();
        self.conn.execute(
            "INSERT INTO alerts (id, key, status, category, host, session_id, block_id, window_id, record_id, state, \
             kind, app, title, msg, record_updated_at, session_label, window_label, pane_label, cwd, created_at, updated_at) \
             VALUES (?1, ?2, 'open', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?19)",
            params![
                id, f.key, f.category, f.host, f.session_id, f.block_id, f.window_id, f.record_id, f.state, f.kind,
                f.app, f.title, f.msg, f.record_updated_at, f.session_label, f.window_label, f.pane_label, f.cwd, t
            ],
        )?;
        Ok(self.alert(id)?.expect("inserted alert"))
    }

    /// Refresh an alert from its live record. Returns the alert if anything
    /// a person would see changed.
    pub fn update_alert(&self, a: &Alert, f: &AlertFields) -> Result<Option<Alert>> {
        let same = a.msg == f.msg
            && a.title == f.title
            && a.kind == f.kind
            && a.session_label == f.session_label
            && a.window_label == f.window_label
            && a.pane_label == f.pane_label
            && a.cwd == f.cwd
            && a.window_id == f.window_id;
        if same && a.record_updated_at == f.record_updated_at {
            return Ok(None);
        }
        self.conn.execute(
            "UPDATE alerts SET kind = ?2, app = ?3, title = ?4, msg = ?5, record_updated_at = ?6, session_label = ?7, \
             window_label = ?8, pane_label = ?9, cwd = ?10, window_id = ?11, updated_at = ?12 WHERE id = ?1",
            params![
                a.id, f.kind, f.app, f.title, f.msg, f.record_updated_at, f.session_label, f.window_label, f.pane_label,
                f.cwd, f.window_id, now()
            ],
        )?;
        let fresh = self.alert(a.id)?.expect("alert");
        Ok(if same { None } else { Some(fresh) })
    }

    pub fn resolve_alert(&self, id: i64, reason: &str) -> Result<Option<Alert>> {
        let t = now();
        self.conn.execute(
            "UPDATE alerts SET status = 'resolved', resolved_at = ?2, resolved_reason = ?3, updated_at = ?2 WHERE id = ?1",
            params![id, t, reason],
        )?;
        self.alert(id)
    }

    /// Set an alert to open, dismissed, or snoozed (until `until`).
    pub fn set_alert_status(&self, id: i64, status: &str, until: Option<&str>) -> Result<Option<Alert>> {
        self.conn.execute(
            "UPDATE alerts SET status = ?2, snooze_until = ?3, updated_at = ?4 WHERE id = ?1 AND status != 'resolved'",
            params![id, status, until, now()],
        )?;
        self.alert(id)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}
