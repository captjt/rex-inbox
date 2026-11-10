//! What an agent said when it finished, from its own Stop hook.
//!
//! Claude Code and Codex report *that* a turn ended (program status, the
//! Finished item), but not what it did. Both run a Stop hook as a turn ends,
//! with the agent's last message on stdin as `last_assistant_message`.
//! `rex-inbox hook stop` hands that to the hub (`turn.done`), which keeps a
//! short form of it on the pane until its next turn starts, and puts it on
//! the pane's Finished item: in `ls`, the apps, and the notification.
//!
//! `rex-inbox hooks install` adds the hook to `~/.claude/settings.json` and
//! `~/.codex/hooks.json`, beside whatever hooks are there. Run it on each
//! machine whose agents should report.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::hub::Shared;
use crate::messages::Caller;

/// How long a summary may be. Long enough for "what changed, what's left".
const SUMMARY_CHARS: usize = 300;

/// The hook's own timeout in the agents' configs, in seconds. It returns at
/// once and sends in the background, so this only bounds a stuck start.
const HOOK_TIMEOUT: u64 = 10;

// ---- the hub's side ----

/// `turn.done`: params `caller`, `text` (the agent's last message).
pub fn turn_done(shared: &Shared, params: &Value) -> Result<Value> {
    let mut caller = Caller::from_params(params);
    let mut core = shared.lock();
    caller.locate(&core);
    let block = caller.block.clone().ok_or_else(|| anyhow!("turn.done comes from an agent's pane"))?;
    let host = caller.host.clone();
    let summary = summarize(params["text"].as_str().unwrap_or_default());
    let pane = core
        .hosts
        .get_mut(&host)
        .and_then(|m| m.panes.get_mut(&block))
        .ok_or_else(|| anyhow!("no pane {block} on any host rex-inbox follows"))?;
    if pane.summary == summary {
        return Ok(json!({ "summary": summary }));
    }
    pane.summary = summary.clone();
    core.reconcile(&host, [block]);
    Ok(json!({ "summary": summary }))
}

/// A one-paragraph form of an agent's last message: code blocks and tables
/// left out, Markdown marks taken off, lines joined as sentences, and cut
/// at a word near `SUMMARY_CHARS`. None when nothing is left.
pub fn summarize(text: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || line.is_empty() || line.starts_with('|') || line.starts_with("---") {
            continue;
        }
        let line = line.trim_start_matches('#').trim_start_matches('>').trim_start();
        let line = strip_bullet(line);
        let line: String = line.replace("**", "").replace("__", "").replace('`', "");
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            continue;
        }
        parts.push(line);
    }
    let mut out = String::new();
    for p in parts {
        if !out.is_empty() {
            let ends = out.ends_with(['.', '!', '?', ':', ';', ',']);
            out.push_str(if ends { " " } else { ". " });
        }
        out.push_str(&p);
        if out.chars().count() > SUMMARY_CHARS {
            break;
        }
    }
    if out.is_empty() {
        return None;
    }
    if out.chars().count() > SUMMARY_CHARS {
        let cut: String = out.chars().take(SUMMARY_CHARS).collect();
        let cut = match cut.rfind(' ') {
            Some(i) if i > SUMMARY_CHARS / 2 => &cut[..i],
            _ => cut.as_str(),
        };
        out = format!("{}…", cut.trim_end_matches([' ', ',', ';', ':', '.', '-']));
    }
    Some(out)
}

fn strip_bullet(line: &str) -> &str {
    for b in ["- [ ] ", "- [x] ", "- ", "* ", "+ ", "• "] {
        if let Some(rest) = line.strip_prefix(b) {
            return rest;
        }
    }
    // "1. " or "12) "
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        let rest = &line[digits..];
        if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return r;
        }
    }
    line
}

// ---- the agent's side: `rex-inbox hook stop` ----

/// Run as an agent's Stop hook. Prints nothing and always succeeds, so it
/// never holds up or changes the agent: it reads the hook's input, starts
/// `hook send` in the background with the message, and returns.
pub fn stop() -> Result<()> {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let Ok(v) = serde_json::from_str::<Value>(&input) else { return Ok(()) };
    // Outside Rex there's no pane to put it on. A hook that made the agent
    // go on (stop_hook_active) means this isn't the end of its turn yet.
    if std::env::var_os("REX_BLOCK").is_none() || v["stop_hook_active"].as_bool() == Some(true) {
        return Ok(());
    }
    let text = v["last_assistant_message"].as_str().unwrap_or_default();
    if text.trim().is_empty() {
        return Ok(());
    }
    use std::os::unix::process::CommandExt;
    let child = std::process::Command::new(std::env::current_exe()?)
        .args(["hook", "send"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        // Its own process group: the agent ending the hook's doesn't end it.
        .process_group(0)
        .spawn();
    if let Ok(mut child) = child
        && let Some(mut stdin) = child.stdin.take()
    {
        let _ = stdin.write_all(text.as_bytes());
    }
    Ok(())
}

/// `hook send`: the message on stdin, to the hub. Failures go to
/// `hook.log` in the state dir, since nobody sees this process's output.
pub fn send(caller: Value) -> Result<()> {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;
    if let Err(e) = crate::api::call("turn.done", json!({ "caller": caller, "text": text })) {
        let dir = crate::paths::state_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("hook.log")) {
            let _ = writeln!(f, "{} turn.done: {e:#}", crate::time::now());
        }
    }
    Ok(())
}

// ---- installing the hook: `rex-inbox hooks` ----

pub struct Agent {
    pub name: &'static str,
    /// The agent's config folder, which must exist for it to be set up here.
    pub dir: PathBuf,
    /// The file with its hooks.
    pub file: PathBuf,
}

/// The agents whose hooks rex-inbox knows how to set, on this machine.
pub fn agents() -> Vec<Agent> {
    let home = crate::paths::home();
    let claude = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| home.join(".claude"));
    let codex = std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".codex"));
    vec![
        Agent { name: "Claude Code", file: claude.join("settings.json"), dir: claude },
        Agent { name: "Codex", file: codex.join("hooks.json"), dir: codex },
    ]
}

#[derive(Debug, PartialEq, Eq)]
pub enum State {
    /// The agent isn't set up on this machine.
    Absent,
    Missing,
    Installed,
    /// Installed, but runs another rex-inbox than this one.
    Stale(String),
}

/// The command the hooks run: this rex-inbox, by its full path.
fn command() -> Result<String> {
    let exe = std::env::current_exe()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let exe = exe.to_string_lossy();
    Ok(if exe.contains([' ', '\'', '"']) { format!("'{}' hook stop", exe.replace('\'', r"'\''")) } else { format!("{exe} hook stop") })
}

fn is_ours(hook: &Value) -> bool {
    hook["command"].as_str().is_some_and(|c| c.contains("rex-inbox") && c.trim_end().ends_with("hook stop"))
}

fn read(file: &Path) -> Result<Value> {
    match std::fs::read_to_string(file) {
        Ok(s) if s.trim().is_empty() => Ok(json!({})),
        Ok(s) => serde_json::from_str(&s).with_context(|| format!("{} isn't valid JSON", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e).with_context(|| format!("reading {}", file.display())),
    }
}

/// Where our hook is in a config: (group, hook) indexes in `hooks.Stop`.
fn find(config: &Value) -> Option<(usize, usize)> {
    config["hooks"]["Stop"]
        .as_array()?
        .iter()
        .enumerate()
        .find_map(|(g, group)| group["hooks"].as_array()?.iter().position(is_ours).map(|h| (g, h)))
}

pub fn state(agent: &Agent) -> Result<State> {
    if !agent.dir.is_dir() {
        return Ok(State::Absent);
    }
    let config = read(&agent.file)?;
    let Some((g, h)) = find(&config) else { return Ok(State::Missing) };
    let have = config["hooks"]["Stop"][g]["hooks"][h]["command"].as_str().unwrap_or_default().to_string();
    Ok(if have == command()? { State::Installed } else { State::Stale(have) })
}

/// Add the hook (or point it at this rex-inbox), leaving every other hook
/// and setting as it was. Our group goes last in `Stop`, so the hooks
/// already there keep their places (Codex trusts hooks by place).
pub fn install(agent: &Agent) -> Result<State> {
    let before = state(agent)?;
    if matches!(before, State::Absent | State::Installed) {
        return Ok(before);
    }
    let mut config = read(&agent.file)?;
    let cmd = command()?;
    match find(&config) {
        Some((g, h)) => config["hooks"]["Stop"][g]["hooks"][h]["command"] = json!(cmd),
        None => {
            let root = config.as_object_mut().ok_or_else(|| anyhow!("{} isn't a JSON object", agent.file.display()))?;
            let hooks = root.entry("hooks").or_insert_with(|| json!({}));
            let stop = hooks
                .as_object_mut()
                .ok_or_else(|| anyhow!("hooks in {} isn't an object", agent.file.display()))?
                .entry("Stop")
                .or_insert_with(|| json!([]));
            stop.as_array_mut()
                .ok_or_else(|| anyhow!("hooks.Stop in {} isn't a list", agent.file.display()))?
                .push(json!({ "hooks": [{ "type": "command", "command": cmd, "timeout": HOOK_TIMEOUT }] }));
        }
    }
    write(&agent.file, &config)?;
    Ok(before)
}

/// Take our hook out, and the group and `Stop` list if that empties them.
pub fn remove(agent: &Agent) -> Result<bool> {
    if state(agent)? == State::Absent || find(&read(&agent.file)?).is_none() {
        return Ok(false);
    }
    let mut config = read(&agent.file)?;
    let Some(stop) = config["hooks"]["Stop"].as_array_mut() else { return Ok(false) };
    for group in stop.iter_mut() {
        if let Some(hooks) = group["hooks"].as_array_mut() {
            hooks.retain(|h| !is_ours(h));
        }
    }
    stop.retain(|g| g["hooks"].as_array().is_none_or(|h| !h.is_empty()));
    if stop.is_empty()
        && let Some(hooks) = config["hooks"].as_object_mut()
    {
        hooks.remove("Stop");
    }
    write(&agent.file, &config)?;
    Ok(true)
}

/// Write a config in place: through a symlink to the file it points at, by
/// way of a temporary file beside it so a failure leaves the old one whole.
fn write(file: &Path, config: &Value) -> Result<()> {
    let target = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let dir = target.parent().ok_or_else(|| anyhow!("{} has no folder", target.display()))?;
    let tmp = dir.join(format!(".{}.rex-inbox.tmp", target.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
    let mut text = serde_json::to_string_pretty(config)?;
    text.push('\n');
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    if let Ok(meta) = std::fs::metadata(&target) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, &target).with_context(|| format!("replacing {}", target.display()))?;
    Ok(())
}

/// `rex-inbox hooks [install | remove]`.
pub fn cli(action: Option<&str>) -> Result<()> {
    let agents = agents();
    match action {
        None | Some("status") => {
            for a in &agents {
                let what = match state(a)? {
                    State::Absent => "not on this machine".to_string(),
                    State::Missing => "no Stop hook: run rex-inbox hooks install".to_string(),
                    State::Installed => format!("reports when it finishes ({})", crate::paths::tilde(&a.file.to_string_lossy())),
                    State::Stale(c) => format!("runs another rex-inbox ({c}): run rex-inbox hooks install"),
                };
                println!("{:<12} {what}", a.name);
            }
        }
        Some("install") => {
            for a in &agents {
                let file = crate::paths::tilde(&a.file.to_string_lossy());
                match install(a)? {
                    State::Absent => println!("{:<12} not on this machine", a.name),
                    State::Installed => println!("{:<12} already reports when it finishes", a.name),
                    State::Missing => {
                        println!("{:<12} added the Stop hook to {file}", a.name);
                        if a.name == "Codex" {
                            println!("{:<12} Codex asks you to trust the new hook once: answer it when it starts", "");
                        }
                    }
                    State::Stale(_) => println!("{:<12} pointed the Stop hook in {file} at this rex-inbox", a.name),
                }
            }
            println!("Agents already running pick it up when they next start.");
        }
        Some("remove") => {
            for a in &agents {
                let file = crate::paths::tilde(&a.file.to_string_lossy());
                if remove(a)? {
                    println!("{:<12} removed the Stop hook from {file}", a.name);
                } else {
                    println!("{:<12} had no rex-inbox hook", a.name);
                }
            }
        }
        Some(other) => bail!("hooks takes install, remove, or nothing (to show them), not {other}"),
    }
    Ok(())
}

/// Whether any agent here lacks the hook, for `status` to mention.
pub fn any_missing() -> bool {
    agents().iter().any(|a| matches!(state(a), Ok(State::Missing | State::Stale(_))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_read_as_one_paragraph() {
        let text = "## Done\n\nFixed the flaky **retry** test in `api/retry.rs`.\n\n- Tests pass\n- Not committed\n\n```rust\nfn x() {}\n```\n| a | b |\n";
        assert_eq!(summarize(text).unwrap(), "Done. Fixed the flaky retry test in api/retry.rs. Tests pass. Not committed");
        assert_eq!(summarize("   \n```\ncode\n```\n"), None);
        let long = "word ".repeat(200);
        let s = summarize(&long).unwrap();
        assert!(s.ends_with('…'));
        assert!(s.chars().count() <= SUMMARY_CHARS + 1);
        assert_eq!(summarize("1. First\n2) Second:\n- [x] third").unwrap(), "First. Second: third");
    }

    #[test]
    fn install_keeps_other_hooks_and_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("rex-inbox-hooks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hooks.json");
        let before = json!({
            "model": "x",
            "hooks": {
                "SessionStart": [{ "hooks": [{ "type": "command", "command": "rex __internal session-start" }] }],
                "Stop": [{ "hooks": [{ "type": "command", "command": "plannotator" }] }]
            }
        });
        std::fs::write(&file, serde_json::to_string_pretty(&before).unwrap()).unwrap();
        let agent = Agent { name: "Codex", dir: dir.clone(), file: file.clone() };

        assert_eq!(state(&agent).unwrap(), State::Missing);
        assert_eq!(install(&agent).unwrap(), State::Missing);
        assert_eq!(state(&agent).unwrap(), State::Installed);
        assert_eq!(install(&agent).unwrap(), State::Installed);
        let after = read(&file).unwrap();
        let stop = after["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], "plannotator");
        assert!(is_ours(&stop[1]["hooks"][0]));
        assert_eq!(after["hooks"]["SessionStart"], before["hooks"]["SessionStart"]);
        // Keys stay in the order they were in.
        let keys: Vec<&String> = after.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["model", "hooks"]);

        assert!(remove(&agent).unwrap());
        assert_eq!(read(&file).unwrap(), before);
        assert!(!remove(&agent).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
