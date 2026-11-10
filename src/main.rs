//! rex-inbox: an inbox and message bus for coding agents running in Rex.

/// A hub log line, with the time.
macro_rules! say {
    ($($t:tt)*) => { eprintln!("{} {}", crate::time::now(), format!($($t)*)) };
}

mod address;
mod agents;
mod api;
mod approve;
mod deliver;
mod fleet;
mod hooks;
mod hub;
mod messages;
mod methods;
mod mirror;
mod notify;
mod paths;
mod preview;
mod push;
mod reviewer;
mod rex;
mod service;
mod show;
mod skill;
mod start;
mod store;
mod time;
mod web;
mod webapp;

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::io::Read;

#[derive(Parser)]
#[command(name = "rex-inbox", version, about = "Everything your Rex agents need from you, and messages between them.")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the hub in the foreground.
    Hub,
    /// Install and start the hub as a LaunchAgent (again, to restart it).
    Up,
    /// Stop the hub's LaunchAgent.
    Down,
    /// The end of the hub's log.
    Logs {
        #[arg(long, short = 'n', default_value_t = 50)]
        lines: usize,
    },
    /// What needs you: blocked, failed, and finished agents.
    Ls {
        /// Include dismissed and snoozed items.
        #[arg(long)]
        all: bool,
        /// Include resolved items too.
        #[arg(long)]
        history: bool,
        #[arg(long)]
        json: bool,
    },
    /// Every agent pane and what it's doing.
    Agents {
        /// Every pane, not just agents.
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// Start Claude Code (or Codex) in a new tab, here or on another Rex host.
    Start {
        /// The host, by its label in rex hosts (default: this machine).
        #[arg(long)]
        on: Option<String>,
        /// Codex instead of Claude Code.
        #[arg(long)]
        codex: bool,
        /// The folder (default: this one here, ~ on another host).
        #[arg(long, short = 'C')]
        cwd: Option<String>,
        /// The pane's name, to message it by (default: claude, claude-2, …).
        #[arg(long)]
        name: Option<String>,
        /// The Rex session for its tab, made if missing (default: inbox).
        #[arg(long, short)]
        session: Option<String>,
        #[arg(long)]
        json: bool,
        /// The first prompt; `-` reads stdin. Without one the agent waits.
        #[arg(trailing_var_arg = true)]
        prompt: Vec<String>,
    },
    /// The machines the hub follows: where agents can start.
    Hosts {
        #[arg(long)]
        json: bool,
    },
    /// Send a message to an agent's pane, or to you (--to me).
    Send {
        /// `me`, a pane name (`reviewer`), `session/pane`, or a block id.
        #[arg(long)]
        to: String,
        #[arg(long, short)]
        subject: Option<String>,
        /// Send as the person, even from an agent's pane.
        #[arg(long)]
        as_me: bool,
        /// The message; `-` (or nothing, when piped) reads stdin.
        body: Option<String>,
    },
    /// Answer a message: it goes to the other side of that conversation.
    Reply {
        id: i64,
        #[arg(long)]
        as_me: bool,
        /// The answer; `-` (or nothing, when piped) reads stdin.
        body: Option<String>,
    },
    /// Messages to this pane (or to you, outside an agent's pane).
    Inbox {
        /// Include read messages and the ones this pane sent.
        #[arg(long)]
        all: bool,
        #[arg(long)]
        as_me: bool,
        #[arg(long)]
        json: bool,
    },
    /// Read a message and its thread, and mark it read.
    Read {
        id: i64,
        #[arg(long)]
        as_me: bool,
        #[arg(long)]
        json: bool,
    },
    /// Wait for the next message to this pane, print it, and mark it read.
    Wait {
        /// Only a message in the thread of this message, such as a reply.
        #[arg(long)]
        thread: Option<i64>,
        /// Give up after this many seconds (exit 2).
        #[arg(long)]
        timeout: Option<f64>,
        #[arg(long)]
        json: bool,
    },
    /// Show the person an app this pane serves on this Mac, on the tailnet,
    /// and ask them about it. Their answer comes back as a reply.
    Share {
        /// The app's port on this Mac (127.0.0.1).
        port: Option<u16>,
        /// What it is, such as "The new checkout form".
        #[arg(long)]
        title: Option<String>,
        /// What you want to know, such as "Does the error state read right?".
        #[arg(long)]
        ask: Option<String>,
        /// An answer to offer as a button; repeat for more.
        #[arg(long = "option", value_name = "ANSWER")]
        options: Vec<String>,
        /// Stop sharing it (or, without a port, everything this pane shares).
        #[arg(long)]
        stop: bool,
        #[arg(long)]
        json: bool,
    },
    /// This pane's address, for others to send to.
    Whoami {
        #[arg(long)]
        json: bool,
    },
    /// Name this pane, so others can send to it by name.
    Name { name: String },
    /// Show one item.
    Show {
        id: i64,
        /// Also print the bottom of the item's pane, such as the prompt itself.
        #[arg(long)]
        screen: bool,
        #[arg(long)]
        json: bool,
    },
    /// Approve a permission prompt, if it is still the one the item shows.
    /// For a review rex-reviewer has ready, post it.
    Approve { id: i64 },
    /// Deny a permission prompt, if it is still the one the item shows.
    /// For a review rex-reviewer has ready, dismiss it.
    Deny { id: i64 },
    /// Print a review rex-reviewer has ready, as it would post.
    Preview { id: i64 },
    /// Bring an item's pane to the front in Rex.
    Open { id: i64 },
    /// Dismiss an alert until its pane's status changes, file a message, or
    /// cancel a message still waiting for its pane.
    Done { id: i64 },
    /// Hide an item for a while, such as 30m, 2h, or 1d.
    Snooze { id: i64, duration: String },
    /// Put a dismissed or snoozed item back.
    Reopen { id: i64 },
    /// Whether the hub is running, and what it follows.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// The phone web app: where it is, and how to publish it on your tailnet.
    Web,
    /// Print the hub's events as JSON lines until interrupted.
    Watch,
    /// Show the agents' Stop hooks, which put what an agent said when it
    /// finished on its Finished item; install or remove them.
    Hooks {
        /// install, or remove; nothing to show them.
        action: Option<String>,
    },
    /// Show the rex-inbox skill, which teaches Claude Code and Codex to
    /// message, reach you, and share what they serve; install or remove it.
    /// `up` installs it when it's missing.
    Skill {
        /// install, or remove; nothing to show it.
        action: Option<String>,
    },
    /// Run as an agent's hook: `stop` reads the hook's input on stdin.
    #[command(hide = true)]
    Hook { event: String },
    /// Answer one request ({"method", "params"}), given or from stdin, from
    /// this machine's hub. Rex's inbox_rpc action runs it for other hosts.
    Rpc {
        /// The request as JSON; without it, stdin.
        request: Option<String>,
    },
}

fn print_json(v: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

/// Where the CLI runs, from the variables Rex sets in every pane. The hub
/// finds which host has the pane: this machine's, or another's when the
/// CLI runs there.
fn caller(as_me: bool) -> Value {
    json!({
        "session_id": std::env::var("REX_SESSION").ok(),
        "block_id": std::env::var("REX_BLOCK").ok(),
        "as_me": as_me,
    })
}

fn body(arg: Option<String>) -> Result<String> {
    use std::io::IsTerminal;
    match arg.as_deref() {
        Some("-") => read_stdin(),
        Some(b) => Ok(b.to_string()),
        None if !std::io::stdin().is_terminal() => read_stdin(),
        None => anyhow::bail!("give the message as an argument, or - to read it from stdin"),
    }
}

fn read_stdin() -> Result<String> {
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s)?;
    Ok(s)
}

/// Whether this machine's Rex has rex-inbox's inbox_rpc action.
fn has_rpc_action() -> bool {
    let out = std::process::Command::new(rex::rex_bin()).args(["--autostart=false", "actions", "--json"]).output();
    let Ok(out) = out else { return false };
    let v: Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    list(&v["actions"]).iter().any(|a| a["name"] == "inbox_rpc")
}

/// Approve or deny; exit 3 if the prompt is still up afterwards.
fn answer(method: &str, id: i64) -> Result<()> {
    let v = api::call(method, json!({ "id": id }))?;
    let done = if method == "approve" { "approved" } else { "denied" };
    match v["summary"].as_str() {
        Some(s) => println!("#{id}: {s}"),
        None => println!("#{id} {done}; the pane is now {}", v["now"].as_str().unwrap_or("?")),
    }
    if v["cleared"] != Value::Bool(true) {
        std::process::exit(3);
    }
    Ok(())
}

fn list(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("rex-inbox: {e:#}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Hub => hub::run(paths::socket(), paths::db()).await,
        Cmd::Up => {
            service::up()?;
            skill::on_up();
            Ok(())
        }
        Cmd::Down => service::down(),
        Cmd::Logs { lines } => service::logs(lines),
        Cmd::Ls { all, history, json } => {
            let v = api::call("inbox", json!({ "all": all, "history": history }))?;
            if json {
                return print_json(&v);
            }
            show::inbox(&list(&v["alerts"]), &list(&v["messages"]));
            Ok(())
        }
        Cmd::Send { to, subject, as_me, body: b } => {
            let v = api::call(
                "messages.send",
                json!({ "caller": caller(as_me), "to": to, "subject": subject, "body": body(b)? }),
            )?;
            println!("{}", show::sent(&v));
            Ok(())
        }
        Cmd::Reply { id, as_me, body: b } => {
            let v = api::call("messages.reply", json!({ "caller": caller(as_me), "id": id, "body": body(b)? }))?;
            println!("{}", show::sent(&v));
            Ok(())
        }
        Cmd::Inbox { all, as_me, json } => {
            let v = api::call("messages.list", json!({ "caller": caller(as_me), "all": all }))?;
            if json {
                return print_json(&v);
            }
            show::messages(&list(&v));
            Ok(())
        }
        Cmd::Read { id, as_me, json } => {
            let v = api::call("messages.read", json!({ "caller": caller(as_me), "id": id }))?;
            if json {
                return print_json(&v);
            }
            show::thread(&v["message"], &list(&v["thread"]));
            Ok(())
        }
        Cmd::Share { port, title, ask, options, stop, json } => {
            if stop {
                let v = api::call("previews.stop", json!({ "caller": caller(false), "port": port }))?;
                if json {
                    return print_json(&v);
                }
                let ids: Vec<String> = list(&v["stopped"]).iter().map(|i| format!("#{i}")).collect();
                println!("stopped {}", ids.join(", "));
                return Ok(());
            }
            let port = port.ok_or_else(|| anyhow::anyhow!("which port? rex-inbox share 3000"))?;
            let v = api::call(
                "previews.share",
                json!({ "caller": caller(false), "port": port, "title": title, "ask": ask, "options": options }),
            )?;
            if json {
                return print_json(&v);
            }
            let id = v["id"].as_i64().unwrap_or(0);
            println!("#{id} in the inbox: {}", v["url"].as_str().unwrap_or(""));
            println!("their answer comes back as a reply; to wait for it: rex-inbox wait --thread {id}");
            println!("if the app turns the request away for its host (Vite: \"Blocked request\"), allow .ts.net hosts");
            Ok(())
        }
        Cmd::Wait { thread, timeout, json } => {
            let v = api::call("messages.wait", json!({ "caller": caller(false), "thread": thread, "timeout": timeout }))?;
            if v["timeout"] == Value::Bool(true) {
                eprintln!("rex-inbox: no message after {}s", timeout.unwrap_or(0.0));
                std::process::exit(2);
            }
            if json {
                return print_json(&v);
            }
            show::message(&v["message"]);
            Ok(())
        }
        Cmd::Whoami { json } => {
            let v = api::call("whoami", json!({ "caller": caller(false) }))?;
            if json {
                return print_json(&v);
            }
            println!("{}", v["address"].as_str().unwrap_or("?"));
            Ok(())
        }
        Cmd::Name { name } => api::call("name", json!({ "caller": caller(false), "name": name })).map(|_| ()),
        Cmd::Start { on, codex, cwd, name, session, json, prompt } => {
            let host = on.unwrap_or_else(|| "local".into());
            let cwd = match cwd {
                Some(c) => Some(std::path::absolute(&c).ok().filter(|_| host == "local" && !c.starts_with('~')).map_or(c, |p| p.display().to_string())),
                None if host == "local" => Some(std::env::current_dir()?.display().to_string()),
                None => None,
            };
            let prompt = match prompt.as_slice() {
                [dash] if dash == "-" => Some(read_stdin()?),
                [] => None,
                words => Some(words.join(" ")),
            };
            let v = api::call(
                "agents.start",
                json!({
                    "host": host,
                    "program": if codex { "codex" } else { "claude" },
                    "cwd": cwd,
                    "name": name,
                    "session": session,
                    "prompt": prompt,
                }),
            )?;
            if json {
                return print_json(&v);
            }
            let s = |k: &str| v[k].as_str().unwrap_or("?").to_string();
            println!("Started {} as {} in {}", agents::display(if codex { "codex" } else { "claude-code" }), s("address"), paths::tilde(&s("cwd")));
            println!("Message it: rex-inbox send --to {} …", s("address"));
            Ok(())
        }
        Cmd::Hosts { json } => {
            let v = api::call("hosts.list", json!({}))?;
            if json {
                return print_json(&v);
            }
            show::hosts(&list(&v));
            Ok(())
        }
        Cmd::Agents { all, json } => {
            let v = api::call("panes.list", json!({ "all": all }))?;
            if json {
                return print_json(&v);
            }
            show::panes(&list(&v));
            Ok(())
        }
        Cmd::Show { id, screen, json } => {
            let mut v = api::call("show", json!({ "id": id }))?;
            if screen && v["type"] == "alert" {
                v["screen"] = api::call("screen", json!({ "id": id }))?["text"].clone();
            }
            if json {
                return print_json(&v);
            }
            match v["type"].as_str() {
                Some("alert") => show::alert_detail(&v["alert"]),
                _ => show::thread(&v["message"], &list(&v["thread"])),
            }
            if let Some(text) = v["screen"].as_str() {
                println!("\n{}", show::dim("── pane ──"));
                println!("{text}");
            }
            Ok(())
        }
        Cmd::Approve { id } => answer("approve", id),
        Cmd::Deny { id } => answer("deny", id),
        Cmd::Preview { id } => {
            let v = api::call("preview", json!({ "id": id }))?;
            println!("{}", v["text"].as_str().unwrap_or("").trim_end());
            Ok(())
        }
        Cmd::Open { id } => api::call("open", json!({ "id": id })).map(|_| ()),
        Cmd::Done { id } => api::call("done", json!({ "id": id })).map(|_| ()),
        Cmd::Snooze { id, duration } => api::call("alerts.snooze", json!({ "id": id, "for": duration })).map(|_| ()),
        Cmd::Reopen { id } => api::call("alerts.reopen", json!({ "id": id })).map(|_| ()),
        Cmd::Status { json } => {
            let v = api::call("status", json!({}))?;
            if json {
                return print_json(&v);
            }
            println!(
                "hub {} running for {} (pid {})",
                v["version"].as_str().unwrap_or("?"),
                time::age(v["started_at"].as_str().unwrap_or("")),
                v["pid"]
            );
            for h in list(&v["hosts"]) {
                let state = if h["connected"].as_bool() == Some(true) { "connected" } else { "disconnected" };
                println!(
                    "  {}: {state}, {} sessions, {} panes, {} agents",
                    h["host"].as_str().unwrap_or("?"),
                    h["sessions"],
                    h["panes"],
                    h["agents"]
                );
                if let Some(e) = h["error"].as_str().filter(|_| state == "disconnected") {
                    println!("    {e}");
                }
            }
            println!("  {} open items", v["open_alerts"]);
            if hooks::any_missing() {
                println!("  agents here don't say what they did when they finish: run rex-inbox hooks install");
            }
            if skill::any_missing() {
                println!("  agents here don't have the current rex-inbox skill: see rex-inbox skill");
            }
            if list(&v["hosts"]).len() > 1 && !has_rpc_action() {
                println!("  agents on other hosts can't reach this hub: Rex here lacks the inbox_rpc action.");
                println!("  Load rex/init.lua from rex-inbox in your Rex init.lua, then: rex config reload");
            }
            Ok(())
        }
        Cmd::Web => {
            let v = api::call("status", json!({}))?;
            let w = &v["web"];
            let Some(listen) = w["listen"].as_str() else {
                println!("The web app is off (REX_INBOX_HTTP=off).");
                return Ok(());
            };
            let port = listen.rsplit(':').next().unwrap_or("7430");
            println!("listening   http://{listen} (this Mac only)");
            match (w["owner"].as_str(), w["url"].as_str()) {
                (Some(owner), Some(url)) => {
                    let devices = w["devices"].as_u64().unwrap_or(0);
                    println!("owner       {owner} (the only Tailscale user it answers)");
                    println!("phone       {url}");
                    println!("devices     {devices} with notifications on");
                    let ready = web::readiness(port);
                    let mut todo = Vec::new();
                    if ready.magic_dns == Some(false) || ready.https == Some(false) {
                        let what = match (ready.magic_dns, ready.https) {
                            (Some(false), Some(false)) => "MagicDNS and HTTPS certificates are",
                            (Some(false), _) => "MagicDNS is",
                            _ => "HTTPS certificates are",
                        };
                        todo.push(format!(
                            "{what} off for your tailnet, so the phone can't open {url}.\n  \
                             Turn them on at https://login.tailscale.com/admin/dns"
                        ));
                    }
                    if ready.served == Some(false) {
                        todo.push(format!("Publish it on your tailnet (once; it stays on):\n  tailscale serve --bg {port}"));
                    }
                    println!();
                    if !todo.is_empty() {
                        println!("{}", todo.join("\n\n"));
                    } else if devices == 0 {
                        println!("Open {url} in Safari on your phone, Share > Add to Home Screen,");
                        println!("open it from the Home Screen, and turn on notifications.");
                    } else {
                        println!("Ready. Open Rex Inbox from your phone's Home Screen.");
                    }
                }
                _ => {
                    println!("tailscale   not running on this Mac, so the web app answers no one.");
                    println!();
                    println!("Install and sign in to Tailscale on this Mac and your phone (same tailnet):");
                    println!("  brew install --cask tailscale-app");
                    println!("In the Tailscale admin console, turn on MagicDNS and HTTPS certificates. Then:");
                    println!("  tailscale serve --bg {port}");
                    println!("and run rex-inbox web again for the phone's address.");
                }
            }
            Ok(())
        }
        Cmd::Watch => api::stream("watch", json!({}), |ev| {
            println!("{}", serde_json::to_string(&ev)?);
            Ok(true)
        }),
        Cmd::Hooks { action } => hooks::cli(action.as_deref()),
        Cmd::Skill { action } => skill::cli(action.as_deref()),
        Cmd::Hook { event } => match event.as_str() {
            "stop" => hooks::stop(),
            "send" => hooks::send(caller(false)),
            // Hooks must not fail an agent, whatever an older config asks.
            _ => Ok(()),
        },
        Cmd::Rpc { request } => {
            let input = match request {
                Some(r) => r,
                None => read_stdin()?,
            };
            let req: api::Request = serde_json::from_str(&input)?;
            if req.method == "hub.stop" || req.method == "watch" {
                anyhow::bail!("{} isn't answered here", req.method);
            }
            let resp = match api::call_local(&req.method, req.params) {
                Ok(v) => api::Response::ok(v),
                Err(e) => api::Response::err(format!("{e:#}")),
            };
            println!("{}", serde_json::to_string(&resp)?);
            Ok(())
        }
    }
}
