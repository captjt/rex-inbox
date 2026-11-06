//! What rex-inbox knows about each coding agent, and the other programs
//! whose records it can answer.

use crate::reviewer::{self, Answer};

/// Agents that take typed messages and report their state to Rex.
pub fn is_known(app: &str) -> bool {
    matches!(app, "claude-code" | "codex")
}

/// A short display name.
pub fn display(app: &str) -> &str {
    match app {
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        reviewer::APP => "Rex Reviewer",
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

const APPROVE_DENY: [Answer; 2] = [Answer { action: "approve", label: "Approve" }, Answer { action: "deny", label: "Deny" }];

/// The answers an open item offers, besides Open, Snooze and Done: the
/// methods a client calls and the labels it shows. The hub decides, so every
/// client offers the same ones.
pub fn answers(status: &str, category: &str, kind: Option<&str>, app: Option<&str>, record_id: &str) -> Vec<Answer> {
    if status == "resolved" || category != "needs_you" || kind != Some("permission") {
        return Vec::new();
    }
    if reviewer::is_item(app, record_id) {
        return reviewer::ANSWERS.to_vec();
    }
    match app {
        Some(a) if prompt_keys(a).is_some() => APPROVE_DENY.to_vec(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(v: Vec<Answer>) -> Vec<&'static str> {
        v.into_iter().map(|a| a.label).collect()
    }

    #[test]
    fn answers_by_app() {
        let p = Some("permission");
        assert_eq!(labels(answers("open", "needs_you", p, Some("claude-code"), "")), ["Approve", "Deny"]);
        assert_eq!(labels(answers("open", "needs_you", p, Some("rex-reviewer"), "pr/acme/api/12/2")), ["Post", "Dismiss", "Preview"]);
        assert!(answers("open", "needs_you", p, Some("codex"), "").is_empty());
        assert!(answers("open", "needs_you", Some("question"), Some("claude-code"), "").is_empty());
        assert!(answers("resolved", "needs_you", p, Some("claude-code"), "").is_empty());
        assert!(answers("open", "failed", None, Some("rex-reviewer"), "pr/acme/api/12/2").is_empty());
        // The watcher's own record is not a PR.
        assert!(answers("open", "needs_you", p, Some("rex-reviewer"), "").is_empty());
    }
}
