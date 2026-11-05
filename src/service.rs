//! Running the hub as a LaunchAgent: `up`, `down`, `logs`.

use anyhow::{Context, Result, bail};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::Command;

use crate::paths::home;

pub const LABEL: &str = "io.github.captjt.rex-inbox";

fn plist() -> PathBuf {
    home().join(format!("Library/LaunchAgents/{LABEL}.plist"))
}

pub fn log() -> PathBuf {
    home().join("Library/Logs/rex-inbox.log")
}

fn domain() -> Result<String> {
    Ok(format!("gui/{}", std::fs::metadata(home())?.uid()))
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn hub_answers() -> bool {
    crate::api::call("status", serde_json::json!({})).is_ok()
}

pub fn up() -> Result<()> {
    let exe = std::env::current_exe()?.canonicalize()?;
    if exe.components().any(|c| c.as_os_str() == "target") {
        eprintln!("rex-inbox: note: running from a build folder ({}); `cargo install --path .` gives a stable path", exe.display());
    }
    let label_running = Command::new("launchctl").args(["print", &format!("{}/{LABEL}", domain()?)]).output()?.status.success();
    if hub_answers() && !label_running {
        // A hub started by hand (rex-inbox hub): hand over to launchd.
        let _ = crate::api::call("hub.stop", serde_json::json!({}));
        for _ in 0..30 {
            if !hub_answers() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if hub_answers() {
            bail!("another hub is running and didn't stop; stop it, then run rex-inbox up");
        }
    }
    // A LaunchAgent's PATH is short: add rex, Homebrew, and cargo.
    let rex_dir = crate::rex::rex_bin().parent().map(|p| p.display().to_string()).unwrap_or_default();
    let path = [
        rex_dir.as_str(),
        "/opt/homebrew/bin",
        "/usr/local/bin",
        &home().join(".cargo/bin").display().to_string(),
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ]
    .join(":");
    let mut env = format!("    <key>PATH</key><string>{}</string>\n", xml(&path));
    for var in ["REX_INBOX_STATE", "REX_INBOX_SOCKET", "REX_INBOX_NOTIFY", "REX_INBOX_REX"] {
        if let Ok(v) = std::env::var(var) {
            env.push_str(&format!("    <key>{var}</key><string>{}</string>\n", xml(&v)));
        }
    }
    let log = log();
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>hub</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
{env}  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>ProcessType</key><string>Interactive</string>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        exe = xml(&exe.display().to_string()),
        log = xml(&log.display().to_string()),
    );
    if let Some(dir) = plist().parent() {
        std::fs::create_dir_all(dir)?;
    }
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(plist(), body).with_context(|| format!("writing {}", plist().display()))?;
    let domain = domain()?;
    // Replace a running one, so `up` also picks up a new binary.
    let _ = Command::new("launchctl").args(["bootout", &format!("{domain}/{LABEL}")]).output();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let out = Command::new("launchctl").args(["bootstrap", &domain, &plist().display().to_string()]).output()?;
    if !out.status.success() {
        bail!("launchctl bootstrap: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    for _ in 0..50 {
        if hub_answers() {
            println!("rex-inbox hub is up ({LABEL}); logs: {}", crate::paths::tilde(&log.display().to_string()));
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    bail!("the hub didn't answer within 5s; see {}", log.display())
}

pub fn down() -> Result<()> {
    let out = Command::new("launchctl").args(["bootout", &format!("{}/{LABEL}", domain()?)]).output()?;
    if !out.status.success() && !hub_answers() {
        println!("rex-inbox hub wasn't running");
        return Ok(());
    }
    println!("rex-inbox hub stopped");
    Ok(())
}

pub fn logs(lines: usize) -> Result<()> {
    let text = std::fs::read_to_string(log()).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    for l in &all[all.len().saturating_sub(lines)..] {
        println!("{l}");
    }
    Ok(())
}
