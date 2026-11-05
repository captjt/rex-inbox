//! rex-inbox: an inbox and message bus for coding agents running in Rex.

mod agents;
mod api;
mod deliver;
mod hub;
mod methods;
mod mirror;
mod notify;
mod paths;
mod rex;
mod show;
mod store;
mod time;

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
    /// Show one item.
    Show {
        id: i64,
        #[arg(long)]
        json: bool,
    },
    /// Bring an item's pane to the front in Rex.
    Open { id: i64 },
    /// Dismiss an item until its pane's status changes.
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
    /// Print the hub's events as JSON lines until interrupted.
    Watch,
    /// Read one request ({"method", "params"}) from stdin and print the answer.
    Rpc,
}

fn print_json(v: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
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
        Cmd::Ls { all, history, json } => {
            let v = api::call("alerts.list", json!({ "all": all || history, "history": history }))?;
            if json { print_json(&v) } else { Ok(show::alerts(&list(&v))) }
        }
        Cmd::Agents { all, json } => {
            let v = api::call("panes.list", json!({ "all": all }))?;
            if json { print_json(&v) } else { Ok(show::panes(&list(&v))) }
        }
        Cmd::Show { id, json } => {
            let v = api::call("show", json!({ "id": id }))?;
            if json {
                return print_json(&v);
            }
            if v["type"] == "alert" {
                show::alert_detail(&v["alert"]);
            }
            Ok(())
        }
        Cmd::Open { id } => api::call("open", json!({ "id": id })).map(|_| ()),
        Cmd::Done { id } => api::call("alerts.dismiss", json!({ "id": id })).map(|_| ()),
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
