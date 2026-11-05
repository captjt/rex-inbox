//! Where rex-inbox keeps its state.

use std::path::PathBuf;

/// `$REX_INBOX_STATE`, or `~/.local/state/rex-inbox`.
pub fn state_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("REX_INBOX_STATE") {
        return PathBuf::from(dir);
    }
    home().join(".local/state/rex-inbox")
}

/// `$REX_INBOX_SOCKET`, or `hub.sock` in the state dir. Unix socket paths
/// are limited to about 100 bytes.
pub fn socket() -> PathBuf {
    if let Some(p) = std::env::var_os("REX_INBOX_SOCKET") {
        return PathBuf::from(p);
    }
    state_dir().join("hub.sock")
}

pub fn db() -> PathBuf {
    state_dir().join("inbox.db")
}

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/".into()))
}

/// Replace the home directory with `~` for display.
pub fn tilde(path: &str) -> String {
    let home = home();
    let home = home.to_string_lossy();
    match path.strip_prefix(home.as_ref()) {
        Some("") => "~".to_string(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}
