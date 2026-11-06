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
mod hub;
mod messages;
mod methods;
mod mirror;
mod notify;
mod paths;
mod push;
mod reviewer;
mod rex;
mod service;
mod show;
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
    /// Read one request ({"method", "params"}) from stdin and print the answer.
    Rpc,
}

fn print_json(v: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

/// Where the CLI runs, from the variables Rex sets in every pane.
fn caller(as_me: bool) -> Value {
    json!({
        "host": "local",
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
        Cmd::Hub => hub::run(paths::socket(), paths::db(), vec![rex::Rex::local()]).await,
        Cmd::Up => service::up(),
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
            }
            println!("  {} open items", v["open_alerts"]);
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
                    println!("owner       {owner} (the only Tailscale user it answers)");
                    println!("phone       {url}");
                    println!("devices     {} with notifications on", w["devices"]);
                    println!();
                    println!("To publish it on your tailnet (once; it stays on):");
                    println!("  tailscale serve --bg {port}");
                    println!("Then open {url} in Safari on your phone, Share > Add to Home Screen,");
                    println!("open it from the Home Screen, and turn on notifications.");
                }
                _ => {
                    println!("tailscale   not running on this Mac, so the web app answers no one.");
                    println!();
                    println!("Install and sign in to Tailscale on this Mac and your phone (same tailnet):");
                    println!("  brew install --cask tailscale");
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
        Cmd::Rpc => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let req: api::Request = serde_json::from_str(&input)?;
            let resp = match api::call(&req.method, req.params) {
                Ok(v) => api::Response::ok(v),
                Err(e) => api::Response::err(format!("{e:#}")),
            };
            println!("{}", serde_json::to_string(&resp)?);
            Ok(())
        }
    }
}
