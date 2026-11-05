//! Human-readable output for the CLI.

use serde_json::Value;
use std::io::IsTerminal;

use crate::paths::tilde;
use crate::time::age;

fn str<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

pub fn bold(s: &str) -> String {
    if std::io::stdout().is_terminal() { format!("\x1b[1m{s}\x1b[0m") } else { s.to_string() }
}

pub fn dim(s: &str) -> String {
    if std::io::stdout().is_terminal() { format!("\x1b[2m{s}\x1b[0m") } else { s.to_string() }
}

/// Print rows as aligned columns; the last column is not padded.
pub fn table(rows: &[Vec<String>], indent: &str) {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0; cols];
    for r in rows {
        for (i, c) in r.iter().enumerate() {
            widths[i] = widths[i].max(c.chars().count());
        }
    }
    for r in rows {
        let mut line = String::from(indent);
        for (i, c) in r.iter().enumerate() {
            if i + 1 == r.len() {
                line.push_str(c);
            } else {
                line.push_str(c);
                line.push_str(&" ".repeat(widths[i] - c.chars().count() + 2));
            }
        }
        println!("{}", line.trim_end());
    }
}

/// `session/pane`, with a host prefix for hosts other than this one.
pub fn place(v: &Value) -> String {
    let host = str(v, "host");
    let session = match v["session_label"].as_str() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => short_id(str(v, "session_id")),
    };
    let pane = match v["pane_label"].as_str().or(v["label"].as_str()) {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => short_id(str(v, "block_id")),
    };
    let p = format!("{session}/{pane}");
    if host.is_empty() || host == "local" { p } else { format!("{host}:{p}") }
}

pub fn short_id(id: &str) -> String {
    let tail = id.rsplit(['-', ':']).next().unwrap_or(id);
    tail.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect()
}

fn app(v: &Value) -> String {
    v["app"].as_str().map(|a| crate::agents::display(a).to_string()).unwrap_or_else(|| "-".into())
}

fn what(v: &Value) -> String {
    let msg = str(v, "msg");
    let title = str(v, "title");
    match (title.is_empty(), msg.is_empty()) {
        (false, false) => format!("{title}: {msg}"),
        (false, true) => title.to_string(),
        _ => msg.to_string(),
    }
}

pub fn alerts(list: &[Value]) {
    if list.is_empty() {
        println!("Nothing needs you.");
        return;
    }
    for (category, heading) in [("needs_you", "Needs you"), ("failed", "Failed"), ("finished", "Finished")] {
        let rows: Vec<Vec<String>> = list
            .iter()
            .filter(|a| str(a, "category") == category)
            .map(|a| {
                let mut kind = str(a, "kind").to_string();
                if str(a, "status") != "open" {
                    kind = format!("{kind} ({})", str(a, "status")).trim().to_string();
                }
                vec![
                    format!("#{}", a["id"]),
                    kind,
                    app(a),
                    place(a),
                    a["cwd"].as_str().map(tilde).unwrap_or_default(),
                    age(str(a, "created_at")),
                    what(a),
                ]
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        println!("{}", bold(heading));
        table(&rows, "  ");
    }
}

pub fn panes(list: &[Value]) {
    if list.is_empty() {
        println!("No agents are running.");
        return;
    }
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|p| {
            let mut state = str(p, "state").to_string();
            if state.is_empty() {
                state = "-".into();
            }
            if let Some(k) = p["kind"].as_str() {
                state = format!("{state} ({k})");
            }
            let n = p["working_children"].as_u64().unwrap_or(0);
            if n > 0 {
                state = format!("{state} +{n}");
            }
            vec![
                state,
                app(p),
                place(p),
                p["cwd"].as_str().map(tilde).unwrap_or_default(),
                p["since"].as_str().map(age).unwrap_or_default(),
                what(p),
            ]
        })
        .collect();
    table(&rows, "");
}

pub fn alert_detail(a: &Value) {
    println!("{} #{}  {}", bold(&str(a, "category").replace('_', " ")), a["id"], dim(str(a, "status")));
    let rows = vec![
        vec!["where".into(), place(a)],
        vec!["folder".into(), a["cwd"].as_str().map(tilde).unwrap_or_default()],
        vec!["agent".into(), app(a)],
        vec!["state".into(), format!("{} {}", str(a, "state"), str(a, "kind")).trim().to_string()],
        vec!["what".into(), what(a)],
        vec!["since".into(), format!("{} ago", age(str(a, "created_at")))],
    ];
    table(&rows.into_iter().filter(|r| !r[1].is_empty()).collect::<Vec<_>>(), "  ");
    if let Some(r) = a["resolved_reason"].as_str() {
        println!("  {}", dim(&format!("resolved {} ago: {r}", age(str(a, "resolved_at")))));
    }
}
