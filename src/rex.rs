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
/// local server. `server` is what `-S` gets: the label itself, or an
/// address given in `REX_INBOX_HOSTS`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rex {
    pub host: Option<String>,
    server: Option<String>,
}

/// A new agent's pane, as Rex made it.
#[derive(Debug)]
pub struct Created {
    pub session_id: String,
    pub window_id: Option<String>,
    pub block_id: String,
}

impl Rex {
    pub fn local() -> Self {
        Rex { host: None, server: None }
    }

    /// Another host, reached by `server` (its label, or an address).
    pub fn remote(label: &str, server: &str) -> Self {
        Rex { host: Some(label.to_string()), server: Some(server.to_string()) }
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
        if let Some(s) = &self.server {
            c.arg("-S").arg(s);
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

    /// `rex hosts list --json`, from this machine: the labels of the other
    /// Rex servers you connect to.
    pub async fn host_labels(&self) -> Result<Vec<String>> {
        let v = self.run_json(&["hosts", "list", "--json"], None).await?;
        Ok(v["hosts"]
            .as_array()
            .map(|a| a.iter().filter_map(|h| h["host"]["label"].as_str()).map(str::to_string).collect())
            .unwrap_or_default())
    }

    /// A folder on this host, through a terminal there: `(status, path,
    /// subfolders)`. Rex expands `~`; status is `ok`, `missing`, or
    /// `not_dir`.
    pub async fn list_dir(&self, session: &str, block: &str, path: &str) -> Result<(String, String, Vec<String>)> {
        let v = self.block_call(session, block, "list_dir", &json!({ "path": path })).await?;
        let names = v["entries"]
            .as_array()
            .map(|a| a.iter().filter_map(|e| e["name"].as_str()).map(str::to_string).collect())
            .unwrap_or_default();
        Ok((
            v["status"].as_str().unwrap_or_default().to_string(),
            v["path"].as_str().unwrap_or(path).to_string(),
            names,
        ))
    }

    /// Start `argv` in a new tab of a session, labeled `label`, in `cwd`,
    /// without taking focus. With `session` `Err(name)`, make that session
    /// first: its first tab holds the program. The tab and the pane both
    /// take the label.
    pub async fn start(&self, session: std::result::Result<&str, &str>, label: &str, cwd: &str, argv: &[&str]) -> Result<Created> {
        let created = match session {
            Ok(sid) => {
                let mut args = vec!["run", "-s", sid, "--label", label, "-c", cwd, "--focus=false", "--json", "--"];
                args.extend_from_slice(argv);
                let v = self.run_json(&args, None).await?;
                Created {
                    session_id: sid.to_string(),
                    window_id: v["window_id"].as_str().map(str::to_string),
                    block_id: v["block_ids"][0].as_str().context("rex run: no block")?.to_string(),
                }
            }
            Err(name) => {
                let mut args = vec!["new", name, "-c", cwd, "--window", label, "--json", "--"];
                args.extend_from_slice(argv);
                let v = self.run_json(&args, None).await?;
                let w = &v["initial_windows"][0];
                let c = Created {
                    session_id: v["session_id"].as_str().context("rex new: no session")?.to_string(),
                    window_id: w["window_id"].as_str().map(str::to_string),
                    block_id: w["block_ids"][0].as_str().context("rex new: no block")?.to_string(),
                };
                self.run(&["block", "rename", "-s", &c.session_id, &c.block_id, label], None).await?;
                c
            }
        };
        if let Some(w) = &created.window_id {
            // A tab named for its agent; cosmetic, so a failure is fine.
            let _ = self.run(&["window", "rename", "-s", &created.session_id, w, label], None).await;
        }
        Ok(created)
    }

    /// The blocks of a session, as Rex has them now.
    pub async fn block_ids(&self, session: &str) -> Result<Vec<String>> {
        let v = self.api(Some(session), "session.list_blocks", &json!({})).await?;
        Ok(v["blocks"]
            .as_array()
            .map(|a| a.iter().filter_map(|b| b["block_id"].as_str()).map(str::to_string).collect())
            .unwrap_or_default())
    }

    /// Start `rex events --all --json`. Read lines from its stdout.
    pub fn events(&self) -> Result<Child> {
        let mut c = self.command();
        c.args(["events", "--all", "--json"]).stdout(Stdio::piped()).stderr(Stdio::null());
        c.spawn().with_context(|| format!("running {}", rex_bin().display()))
    }
}
