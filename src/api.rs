//! The hub's wire format, and the client the CLI uses.
//!
//! One request per connection on the unix socket: a JSON line
//! `{"method": "...", "params": {...}}` in, a JSON line
//! `{"ok": true, "result": ...}` or `{"ok": false, "error": "..."}` out.
//! `watch` keeps the connection open and writes one event per line.

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

/// Call a method and return its result.
pub fn call(method: &str, params: Value) -> Result<Value> {
    let mut stream = connect()?;
    send(&mut stream, method, params)?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).context("reading the hub's answer")?;
    if line.is_empty() {
        bail!("the hub closed the connection");
    }
    let resp: Response = serde_json::from_str(&line).context("the hub's answer")?;
    if resp.ok {
        Ok(resp.result.unwrap_or(Value::Null))
    } else {
        Err(anyhow!(resp.error.unwrap_or_else(|| "unknown error".into())))
    }
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
