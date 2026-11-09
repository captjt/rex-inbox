//! The hub's wire format, and the client the CLI uses.
//!
//! One request per connection on the unix socket: a JSON line
//! `{"method": "...", "params": {...}}` in, a JSON line
//! `{"ok": true, "result": ...}` or `{"ok": false, "error": "..."}` out.
//! `watch` keeps the connection open and writes one event per line.
//!
//! On a machine without a hub (another Rex host), the CLI reaches the hub
//! through Rex instead: `rex -S SERVER do inbox_rpc req=REQUEST` runs
//! `rex-inbox rpc REQUEST` on the hub's machine (see rex/init.lua). SERVER
//! is `REX_INBOX_VIA`, or else this machine's Rex home when that is another
//! machine. `REX_INBOX_VIA=local` keeps to the socket.

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(v: Value) -> Self {
        Response { ok: true, result: Some(v), error: None }
    }
    pub fn err(e: String) -> Self {
        Response { ok: false, result: None, error: Some(e) }
    }
}

fn connect() -> Result<UnixStream> {
    let path = crate::paths::socket();
    UnixStream::connect(&path).map_err(|_| {
        anyhow!("the rex-inbox hub isn't running (no {}). Start it with: rex-inbox up", crate::paths::tilde(&path.to_string_lossy()))
    })
}

fn send(stream: &mut UnixStream, method: &str, params: Value) -> Result<()> {
    let req = Request { method: method.to_string(), params };
    stream.write_all(format!("{}\n", serde_json::to_string(&req)?).as_bytes())?;
    Ok(())
}

/// Call a method and return its result, by the socket or through Rex.
pub fn call(method: &str, params: Value) -> Result<Value> {
    match via() {
        Some(server) => via_rex(&server, method, params),
        None => call_local(method, params),
    }
}

/// The Rex server to reach the hub through, when not the socket.
fn via() -> Option<String> {
    if let Ok(v) = std::env::var("REX_INBOX_VIA") {
        return Some(v).filter(|v| !v.is_empty() && v != "local");
    }
    if crate::paths::socket().exists() {
        return None;
    }
    rex_home()
}

/// This machine's Rex home, when it's another machine: where the hub runs.
fn rex_home() -> Option<String> {
    let out = rex_cmd().args(["hosts", "list", "--json"]).output().ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    let home = &v["home"];
    if home["this_machine"].as_bool() != Some(false) {
        return None;
    }
    let label = home["label"].as_str().filter(|l| !l.is_empty());
    label.or(home["endpoint"].as_str()).map(str::to_string)
}

/// `rex`, without this pane's location: it names a place on this machine's
/// server, not the hub's.
fn rex_cmd() -> std::process::Command {
    let mut c = std::process::Command::new(crate::rex::rex_bin());
    c.env_remove("REX_SESSION").env_remove("REX_BLOCK").env_remove("REX_CLIENT").arg("--autostart=false");
    c
}

/// How long one request through Rex may hold a `wait` open; the CLI asks
/// again until its own timeout.
const WAIT_CHUNK: f64 = 45.0;

fn via_rex(server: &str, method: &str, mut params: Value) -> Result<Value> {
    if method == "watch" {
        bail!("watch needs the hub's socket; run it on the hub's machine");
    }
    if method != "messages.wait" {
        return rex_rpc(server, method, &params);
    }
    let deadline = params["timeout"].as_f64().map(|t| std::time::Instant::now() + std::time::Duration::from_secs_f64(t));
    loop {
        let left = deadline.map(|d| d.saturating_duration_since(std::time::Instant::now()).as_secs_f64());
        params["timeout"] = Value::from(left.unwrap_or(WAIT_CHUNK).min(WAIT_CHUNK));
        let v = rex_rpc(server, method, &params)?;
        let more = left.is_none_or(|l| l > WAIT_CHUNK);
        if v["timeout"] != Value::Bool(true) || !more {
            return Ok(v);
        }
    }
}

/// One request through `rex do inbox_rpc`.
fn rex_rpc(server: &str, method: &str, params: &Value) -> Result<Value> {
    let req = serde_json::to_string(&Request { method: method.to_string(), params: params.clone() })?;
    let out = rex_cmd()
        .args(["-S", server, "--timeout", "90s", "do", "inbox_rpc"])
        .arg(format!("req={req}"))
        .output()
        .with_context(|| format!("running {}", crate::rex::rex_bin().display()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim().trim_start_matches("Error: ");
        if err.contains("inbox_rpc") && !err.contains("rex-inbox rpc") {
            bail!("{server} doesn't have rex-inbox's Rex action: load rex/init.lua from rex-inbox in its init.lua ({err})");
        }
        bail!("reaching the hub through Rex ({server}): {err}");
    }
    let v: Value = serde_json::from_slice(&out.stdout).context("rex do inbox_rpc: bad JSON")?;
    let text = v["response"].as_str().context("rex do inbox_rpc: no response")?;
    answer(serde_json::from_str(text).context("the hub's answer")?)
}

fn answer(resp: Response) -> Result<Value> {
    if resp.ok {
        Ok(resp.result.unwrap_or(Value::Null))
    } else {
        Err(anyhow!(resp.error.unwrap_or_else(|| "unknown error".into())))
    }
}

/// Call a method over the socket.
pub fn call_local(method: &str, params: Value) -> Result<Value> {
    let mut stream = connect()?;
    send(&mut stream, method, params)?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).context("reading the hub's answer")?;
    if line.is_empty() {
        bail!("the hub closed the connection");
    }
    answer(serde_json::from_str(&line).context("the hub's answer")?)
}

/// Call a streaming method. `f` gets each line after the first; return
/// `false` to stop.
pub fn stream(method: &str, params: Value, mut f: impl FnMut(Value) -> Result<bool>) -> Result<()> {
    let mut stream = connect()?;
    send(&mut stream, method, params)?;
    let mut reader = BufReader::new(stream);
    let mut first = true;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            bail!("the hub closed the connection");
        }
        let v: Value = serde_json::from_str(&line)?;
        if first {
            first = false;
            if v["ok"] == Value::Bool(false) {
                bail!("{}", v["error"].as_str().unwrap_or("unknown error"));
            }
            continue;
        }
        if !f(v)? {
            return Ok(());
        }
    }
}
