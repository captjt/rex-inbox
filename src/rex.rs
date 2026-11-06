//! A thin wrapper over the `rex` CLI. rex-inbox talks to Rex only through the
//! documented CLI, so it keeps working as Rex's wire protocol changes.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};

/// Where the Rex Server app installs the CLI on macOS, for when `rex` is not
/// on PATH (a LaunchAgent's PATH is short).
const APP_REX: &str = "Library/Application Support/com.superlogical.rex/RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex";

pub fn rex_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("REX_INBOX_REX") {
        return PathBuf::from(p);
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("rex");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    crate::paths::home().join(APP_REX)
}

/// One Rex server. `host` is a label from `rex hosts`, or `None` for the
/// local server.
#[derive(Clone, Debug)]
pub struct Rex {
    pub host: Option<String>,
}

impl Rex {
    pub fn local() -> Self {
        Rex { host: None }
    }

    /// The name this server goes by in rex-inbox records.
    pub fn name(&self) -> &str {
        self.host.as_deref().unwrap_or("local")
    }

    fn command(&self) -> Command {
        let mut c = Command::new(rex_bin());
        // Never let a pane's own location leak into the hub's calls.
        c.env_remove("REX_SESSION").env_remove("REX_BLOCK").env_remove("REX_CLIENT");
        c.arg("--autostart=false");
        if let Some(h) = &self.host {
            c.arg("-S").arg(h);
        }
        c.stdin(Stdio::null()).kill_on_drop(true);
        c
    }

    async fn run(&self, args: &[&str], stdin: Option<String>) -> Result<String> {
        let mut c = self.command();
        c.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
        if stdin.is_some() {
            c.stdin(Stdio::piped());
        }
        let mut child = c.spawn().with_context(|| format!("running {}", rex_bin().display()))?;
        if let Some(input) = stdin {
            let mut si = child.stdin.take().expect("piped stdin");
            si.write_all(input.as_bytes()).await?;
            drop(si);
        }
        let out = child.wait_with_output().await?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            bail!("rex {}: {}", args.join(" "), err.trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    async fn run_json(&self, args: &[&str], stdin: Option<String>) -> Result<Value> {
        let out = self.run(args, stdin).await?;
        let out = out.trim();
        if out.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(out).with_context(|| format!("rex {}: bad JSON", args.join(" ")))
    }

    /// `rex api call [-s SESSION] METHOD -`, with `params` on stdin.
    pub async fn api(&self, session: Option<&str>, method: &str, params: &Value) -> Result<Value> {
        let mut args = vec!["api", "call"];
        if let Some(s) = session {
            args.extend(["-s", s]);
        }
        args.extend([method, "-"]);
        self.run_json(&args, Some(params.to_string())).await
    }

    /// `rex block call -s SESSION -b BLOCK METHOD ARGS`.
    pub async fn block_call(&self, session: &str, block: &str, method: &str, params: &Value) -> Result<Value> {
        let p = params.to_string();
        self.run_json(&["block", "call", "-s", session, "-b", block, method, &p], None).await
    }

    /// Paste text into a terminal, the way a person pastes: bracketed, so
    /// newlines don't submit early.
    pub async fn paste(&self, session: &str, block: &str, text: &str) -> Result<()> {
        let params = json!({ "block_id": block, "event": { "type": "paste", "payload": { "text": text } } });
        self.api(Some(session), "block.event", &params).await.map(|_| ())
    }

    /// `rex send-key -s SESSION -b BLOCK KEY...`.
    pub async fn send_keys(&self, session: &str, block: &str, keys: &[&str]) -> Result<()> {
        let mut args = vec!["send-key", "-s", session, "-b", block];
        args.extend_from_slice(keys);
        self.run(&args, None).await.map(|_| ())
    }

    /// The last `lines` lines of a terminal, as plain text.
    pub async fn capture(&self, session: &str, block: &str, lines: usize) -> Result<String> {
        let out = self.run(&["capture", "-s", session, "-b", block, "--trim"], None).await?;
        let all: Vec<&str> = out.lines().collect();
        Ok(all[all.len().saturating_sub(lines)..].join("\n"))
    }

    /// `rex do ACTION KEY=VALUE...`: an action `init.lua` defines, such as
    /// another tool's. Its result, as JSON.
    pub async fn action(&self, name: &str, args: &[(&str, &str)]) -> Result<Value> {
        let pairs: Vec<String> = args.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let mut argv = vec!["do", name];
        argv.extend(pairs.iter().map(String::as_str));
        self.run_json(&argv, None).await
    }

    /// `rex focus` plus the app's `session.select`, so the pane is in front.
    pub async fn focus(&self, session: &str, block: &str, window: Option<&str>) -> Result<()> {
        self.run(&["focus", "-s", session, block], None).await?;
        let sel = format!("session_id={session}");
        let mut args = vec!["do", "session.select", &sel];
        let win;
        if let Some(w) = window {
            win = format!("window_id={w}");
            args.push(&win);
        }
        // No app connected is fine: the pane is still focused in Rex.
        let _ = self.run(&args, None).await;
        Ok(())
    }

    /// Start `rex events --all --json`. Read lines from its stdout.
    pub fn events(&self) -> Result<Child> {
        let mut c = self.command();
        c.args(["events", "--all", "--json"]).stdout(Stdio::piped()).stderr(Stdio::null());
        c.spawn().with_context(|| format!("running {}", rex_bin().display()))
    }
}
