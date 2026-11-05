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
