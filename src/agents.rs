//! What rex-inbox knows about each coding agent.

/// Agents that take typed messages and report their state to Rex.
pub fn is_known(app: &str) -> bool {
    matches!(app, "claude-code" | "codex")
}

/// A short display name.
pub fn display(app: &str) -> &str {
    match app {
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        other => other,
    }
}

/// Keys that answer a permission prompt: `(approve, deny)`. Only agents whose
/// prompt was checked live get keys; the rest can only be opened.
///
/// Claude Code (2.1.318): "Do you want to proceed?" with "1. Yes" first and
/// "Esc to cancel". A digit picks its option outright, whatever is
/// highlighted, so approve is `1`, never Enter.
pub fn prompt_keys(app: &str) -> Option<(&'static [&'static str], &'static [&'static str])> {
    match app {
        "claude-code" => Some((&["1"], &["escape"])),
        _ => None,
    }
}
