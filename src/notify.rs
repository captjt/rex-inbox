//! Mac notifications for what needs you: a blocked or failed agent, and
//! messages to you. `REX_INBOX_NOTIFY=off` turns them off.
//!
//! An alert waits a moment first, and is dropped if it resolved meanwhile
//! (you answered in the pane, or the agent moved on). Bursts are capped.
//! With terminal-notifier on PATH, clicking a notification runs
//! `rex-inbox open N`; otherwise it goes through osascript.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;

use crate::hub::{HubEvent, Shared};

const GRACE: Duration = Duration::from_secs(2);
const BURST: usize = 5;
const WINDOW: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Limiter {
    recent: VecDeque<Instant>,
    held: usize,
}

impl Limiter {
    /// `Some(n)` to post, with `n` notifications held back before it.
    fn admit(&mut self) -> Option<usize> {
        let now = Instant::now();
        while self.recent.front().is_some_and(|t| now - *t > WINDOW) {
            self.recent.pop_front();
        }
        if self.recent.len() >= BURST {
            self.held += 1;
            return None;
        }
        self.recent.push_back(now);
        Some(std::mem::take(&mut self.held))
    }
}

struct Note {
    id: i64,
    title: String,
    subtitle: String,
    body: String,
}

pub async fn run(shared: Shared) {
    if std::env::var("REX_INBOX_NOTIFY").is_ok_and(|v| v == "off" || v == "0") {
        return;
    }
    let limiter = Arc::new(Mutex::new(Limiter::default()));
    let mut rx = shared.lock().events.subscribe();
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(RecvError::Lagged(_)) => continue,
            Err(RecvError::Closed) => return,
        };
        match ev {
            HubEvent::AlertOpened { alert } if alert.category == "needs_you" || alert.category == "failed" => {
                let (shared, limiter) = (shared.clone(), limiter.clone());
                tokio::spawn(async move {
                    tokio::time::sleep(GRACE).await;
                    let open = shared.lock().store.alert(alert.id).ok().flatten().is_some_and(|a| a.status == "open");
                    if !open {
                        return;
                    }
                    let app = alert.app.as_deref().map(crate::agents::display).unwrap_or("A program").to_string();
                    let title = match (alert.category.as_str(), alert.kind.as_deref()) {
                        ("failed", _) => format!("{app} failed"),
                        (_, Some("permission")) => format!("{app} needs your approval"),
                        (_, Some("question")) => format!("{app} has a question"),
                        (_, Some("auth")) => format!("{app} needs you to sign in"),
                        (_, Some("startup")) => format!("{app} is waiting to start"),
                        _ => format!("{app} needs you"),
                    };
                    let place = crate::show::place(&serde_json::to_value(&alert).unwrap_or_default());
                    let body = [alert.title.as_deref(), alert.msg.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(": ");
                    post(&shared, &limiter, Note { id: alert.id, title, subtitle: format!("#{} · {place}", alert.id), body }).await;
                });
            }
            HubEvent::MessageNew { message } if message.to_me() => {
                let first = message.body.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string();
                let body = match &message.subject {
                    Some(s) => format!("{s}: {first}"),
                    None => first,
                };
                let note = Note {
                    id: message.id,
                    title: message.headline(),
                    subtitle: format!("#{} · reply with rex-inbox reply {}", message.id, message.id),
                    body,
                };
                post(&shared, &limiter, note).await;
            }
            _ => {}
        }
    }
}

async fn post(shared: &Shared, limiter: &Mutex<Limiter>, mut n: Note) {
    // An app is showing its own notifications.
    if shared.lock().notifiers > 0 {
        return;
    }
    let Some(held) = limiter.lock().unwrap_or_else(|e| e.into_inner()).admit() else { return };
    if held > 0 {
        n.subtitle.push_str(&format!(" (+{held} more in rex-inbox ls)"));
    }
    let n = Note { body: clip(&n.body, 240), ..n };
    if let Err(e) = show(&n).await {
        say!("notification: {e:#}");
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max - 1).collect::<String>()) }
}

fn on_path(name: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(name)).find(|p| p.is_file())
}

async fn show(n: &Note) -> anyhow::Result<()> {
    let status = if let Some(tn) = on_path("terminal-notifier") {
        let exe = std::env::current_exe()?;
        Command::new(tn)
            .args(["-title", &n.title, "-subtitle", &n.subtitle, "-message", &n.body, "-sound", "default"])
            .args(["-group", &format!("rex-inbox-{}", n.id)])
            .args(["-execute", &format!("'{}' open {}", exe.display(), n.id)])
            .status()
            .await?
    } else {
        // Text goes in as arguments, never into the script.
        Command::new("osascript")
            .args(["-e", "on run argv", "-e"])
            .arg("display notification (item 1 of argv) with title (item 2 of argv) subtitle (item 3 of argv) sound name \"default\"")
            .args(["-e", "end run", &n.body, &n.title, &n.subtitle])
            .status()
            .await?
    };
    if !status.success() {
        anyhow::bail!("notifier exited with {status}");
    }
    Ok(())
}
