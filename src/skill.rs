//! The rex-inbox skill: how agents learn to message each other, reach you,
//! and share what they serve. It ships in the binary (skills/rex-inbox) and
//! goes where Claude Code and Codex read skills, `~/.claude/skills` and
//! `~/.codex/skills`, written through a symlink to wherever it points.
//!
//! `rex-inbox up` installs it when it's missing and keeps an installed copy
//! up to date, by the marker line in it. A copy without the marker is
//! yours: only `rex-inbox skill install` replaces it.

use anyhow::{Context, Result, anyhow, bail};
use std::path::{Path, PathBuf};

const SKILL: &str = include_str!("../skills/rex-inbox/SKILL.md");
/// The line that says rex-inbox keeps this copy up to date.
const MARKER: &str = "<!-- rex-inbox installs this skill";

pub struct Agent {
    pub name: &'static str,
    /// The agent's config folder, which must exist for it to be set up here.
    pub dir: PathBuf,
}

impl Agent {
    fn file(&self) -> PathBuf {
        self.dir.join("skills").join("rex-inbox").join("SKILL.md")
    }
}

pub fn agents() -> Vec<Agent> {
    let home = crate::paths::home();
    let claude = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| home.join(".claude"));
    let codex = std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".codex"));
    vec![Agent { name: "Claude Code", dir: claude }, Agent { name: "Codex", dir: codex }]
}

#[derive(Debug, PartialEq, Eq)]
pub enum State {
    /// The agent isn't set up on this machine.
    Absent,
    Missing,
    Current,
    /// Ours, from another rex-inbox.
    Outdated,
    /// A rex-inbox skill without the marker: someone's own copy.
    Own,
}

pub fn state(agent: &Agent) -> Result<State> {
    if !agent.dir.is_dir() {
        return Ok(State::Absent);
    }
    let file = agent.file();
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(State::Missing),
        Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
    };
    Ok(if text == SKILL {
        State::Current
    } else if text.contains(MARKER) {
        State::Outdated
    } else {
        State::Own
    })
}

/// Write the skill, through a symlink to the file it points at, by way of
/// a temporary file beside it.
fn write(file: &Path) -> Result<PathBuf> {
    let target = resolve(file);
    let dir = target.parent().ok_or_else(|| anyhow!("{} has no folder", target.display()))?;
    std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
    let tmp = dir.join(".SKILL.md.rex-inbox.tmp");
    std::fs::write(&tmp, SKILL).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &target).with_context(|| format!("replacing {}", target.display()))?;
    Ok(target)
}

/// Where a skill file really is: its folder may be a symlink (to dotfiles,
/// or one copy both agents share), and so may the file.
fn resolve(file: &Path) -> PathBuf {
    if let Ok(p) = file.canonicalize() {
        return p;
    }
    match (file.parent().and_then(|d| d.canonicalize().ok()), file.file_name()) {
        (Some(dir), Some(name)) => dir.join(name),
        _ => file.to_path_buf(),
    }
}

/// Install for every agent here. `force` replaces a copy without the
/// marker too. Returns each agent's state before.
fn install(force: bool) -> Result<Vec<(&'static str, State, PathBuf)>> {
    let mut done: Vec<PathBuf> = Vec::new();
    let mut out = Vec::new();
    for a in agents() {
        let before = state(&a)?;
        let file = a.file();
        let write_it = match before {
            State::Missing | State::Outdated => true,
            State::Own => force,
            State::Absent | State::Current => false,
        };
        // Two agents can share one copy through symlinks: write it once.
        if write_it && !done.contains(&resolve(&file)) {
            done.push(write(&file)?);
        }
        out.push((a.name, before, file));
    }
    Ok(out)
}

/// For `rex-inbox up`: install where it's missing, update our own copies,
/// and say so; leave anyone's own copy alone. Never fails `up`.
pub fn on_up() {
    match install(false) {
        Ok(list) => {
            for (name, before, file) in list {
                let file = crate::paths::tilde(&file.to_string_lossy());
                match before {
                    State::Missing => println!("{name}: installed the rex-inbox skill ({file})"),
                    State::Outdated => println!("{name}: updated the rex-inbox skill ({file})"),
                    _ => {}
                }
            }
        }
        Err(e) => eprintln!("rex-inbox: the skill: {e:#}"),
    }
}

/// Whether an agent here lacks the current skill, for `status` to mention.
pub fn any_missing() -> bool {
    agents().iter().any(|a| matches!(state(a), Ok(State::Missing | State::Outdated | State::Own)))
}

/// `rex-inbox skill [install | remove]`.
pub fn cli(action: Option<&str>) -> Result<()> {
    match action {
        None | Some("status") => {
            for a in agents() {
                let file = crate::paths::tilde(&a.file().to_string_lossy());
                let what = match state(&a)? {
                    State::Absent => "not on this machine".to_string(),
                    State::Missing => "no rex-inbox skill: run rex-inbox skill install".to_string(),
                    State::Current => format!("up to date ({file})"),
                    State::Outdated => format!("from another rex-inbox ({file}): run rex-inbox skill install"),
                    State::Own => format!("your own copy ({file}); rex-inbox skill install replaces it"),
                };
                println!("{:<12} {what}", a.name);
            }
        }
        Some("install") => {
            for (name, before, file) in install(true)? {
                let file = crate::paths::tilde(&file.to_string_lossy());
                let what = match before {
                    State::Absent => "not on this machine".to_string(),
                    State::Current => format!("already up to date ({file})"),
                    State::Missing => format!("installed {file}"),
                    State::Outdated | State::Own => format!("updated {file}"),
                };
                println!("{name:<12} {what}");
            }
            println!("Agents load it when they next start.");
        }
        Some("remove") => {
            for a in agents() {
                let file = a.file();
                let shown = crate::paths::tilde(&file.to_string_lossy());
                if matches!(state(&a)?, State::Absent | State::Missing) {
                    println!("{:<12} had no rex-inbox skill", a.name);
                    continue;
                }
                // A symlinked folder (to dotfiles, say) goes as a link; what
                // it points at stays.
                let dir = file.parent().expect("skill folder");
                if dir.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
                    std::fs::remove_file(dir).with_context(|| format!("removing {}", dir.display()))?;
                } else {
                    std::fs::remove_file(&file).with_context(|| format!("removing {}", file.display()))?;
                    let _ = std::fs::remove_dir(dir);
                }
                println!("{:<12} removed {shown}", a.name);
            }
        }
        Some(other) => bail!("skill takes install, remove, or nothing (to show it), not {other}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(dir: &Path) -> Agent {
        Agent { name: "Test", dir: dir.to_path_buf() }
    }

    #[test]
    fn the_skill_says_how_to_share() {
        assert!(SKILL.starts_with("---\nname: rex-inbox\n"));
        assert!(SKILL.contains(MARKER));
        assert!(SKILL.contains("rex-inbox share"));
    }

    #[test]
    fn states_follow_what_is_there() {
        let root = std::env::temp_dir().join(format!("rex-inbox-skill-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let a = agent(&root.join("claude"));
        assert_eq!(state(&a).unwrap(), State::Absent);
        std::fs::create_dir_all(&a.dir).unwrap();
        assert_eq!(state(&a).unwrap(), State::Missing);
        write(&a.file()).unwrap();
        assert_eq!(state(&a).unwrap(), State::Current);
        std::fs::write(a.file(), SKILL.replace("rex-inbox share", "rex-inbox shr")).unwrap();
        assert_eq!(state(&a).unwrap(), State::Outdated, "ours, older");
        std::fs::write(a.file(), "---\nname: rex-inbox\n---\nmine\n").unwrap();
        assert_eq!(state(&a).unwrap(), State::Own, "no marker: someone's own");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn it_writes_through_a_symlinked_folder() {
        let root = std::env::temp_dir().join(format!("rex-inbox-skill-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let shared = root.join("agents/skills/rex-inbox");
        std::fs::create_dir_all(&shared).unwrap();
        let a = agent(&root.join("claude"));
        std::fs::create_dir_all(a.dir.join("skills")).unwrap();
        std::os::unix::fs::symlink(&shared, a.dir.join("skills/rex-inbox")).unwrap();
        write(&a.file()).unwrap();
        assert_eq!(std::fs::read_to_string(shared.join("SKILL.md")).unwrap(), SKILL);
        assert!(a.dir.join("skills/rex-inbox").symlink_metadata().unwrap().file_type().is_symlink(), "the link stays a link");
        let _ = std::fs::remove_dir_all(&root);
    }
}
