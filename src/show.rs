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

/// `session/pane`, with a host prefix for hosts other than this one. A
/// pane another tool runs goes by what it does there, such as
/// `api#12 · codex reviewer`.
pub fn place(v: &Value) -> String {
    let host = str(v, "host");
    let on_host = |p: String| if host.is_empty() || host == "local" { p } else { format!("{host}:{p}") };
    if let Some(subject) = v["subject"].as_str().filter(|s| !s.is_empty()) {
        return on_host(subject.to_string());
    }
    let session = match v["session_label"].as_str() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => short_id(str(v, "session_id")),
    };
    let pane = match v["pane_label"].as_str().or(v["label"].as_str()) {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => short_id(str(v, "block_id")),
    };
    on_host(format!("{session}/{pane}"))
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

pub fn inbox(alerts_list: &[Value], messages_list: &[Value]) {
    if alerts_list.is_empty() && messages_list.is_empty() {
        println!("Nothing needs you.");
        return;
    }
    alerts(alerts_list);
    if !messages_list.is_empty() {
        println!("{}", bold("Messages"));
        message_rows(messages_list, true);
    }
}

pub fn alerts(list: &[Value]) {
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

pub fn hosts(list: &[Value]) {
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|h| {
            let up = h["connected"].as_bool() == Some(true);
            let n = h["agents"].as_u64().unwrap_or(0);
            let state = if up { format!("{n} agent{}", if n == 1 { "" } else { "s" }) } else { "unreachable".into() };
            let sessions: Vec<&str> = h["sessions"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            let detail = if up { sessions.join(", ") } else { h["error"].as_str().unwrap_or_default().to_string() };
            vec![str(h, "host").to_string(), state, dim(&detail)]
        })
        .collect();
    table(&rows, "");
    if list.len() < 2 {
        println!("{}", dim("Add machines to Rex with rex hosts add LABEL ENDPOINT; the hub follows them."));
    }
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
    let id = &a["id"];
    let answers: Vec<String> = a["answers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| Some(format!("{}: rex-inbox {} {id}", x["label"].as_str()?, x["action"].as_str()?)))
        .collect();
    if !answers.is_empty() {
        println!("  {}", dim(&answers.join("   ")));
    }
    if let Some(r) = a["resolved_reason"].as_str() {
        println!("  {}", dim(&format!("resolved {} ago: {r}", age(str(a, "resolved_at")))));
    }
}

// ---- messages ----

fn who(addr: &str) -> String {
    if addr == "me" { "you".into() } else { addr.to_string() }
}

fn first_line(m: &Value) -> String {
    let subject = str(m, "subject");
    let body = str(m, "body").lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let s = if subject.is_empty() { body.to_string() } else { subject.to_string() };
    if s.chars().count() > 80 { format!("{}…", s.chars().take(79).collect::<String>()) } else { s }
}

fn message_rows(list: &[Value], to_me: bool) {
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|m| {
            let peer = if to_me || str(m, "to_addr") == "me" || m["to_block"].is_null() {
                format!("from {}", who(str(m, "from_addr")))
            } else {
                format!("from {} to {}", who(str(m, "from_addr")), who(str(m, "to_addr")))
            };
            vec![format!("#{}", m["id"]), str(m, "status").to_string(), peer, age(str(m, "created_at")), first_line(m)]
        })
        .collect();
    table(&rows, "  ");
}

pub fn messages(list: &[Value]) {
    if list.is_empty() {
        println!("No messages.");
        return;
    }
    message_rows(list, false);
}

pub fn sent(m: &Value) -> String {
    let to = who(str(m, "to_addr"));
    let how = match str(m, "status") {
        "queued" => "queued; it goes in when the agent is idle",
        _ => "sent",
    };
    format!("#{} to {to}: {how}", m["id"])
}

pub fn message(m: &Value) {
    let mut head = format!("#{} from {} to {}", m["id"], who(str(m, "from_addr")), who(str(m, "to_addr")));
    if let Some(r) = m["reply_to"].as_i64() {
        head.push_str(&format!(", replying to #{r}"));
    }
    println!("{}  {}", bold(&head), dim(&format!("{} ago, {}", age(str(m, "created_at")), str(m, "status"))));
    if let Some(s) = m["subject"].as_str() {
        println!("Subject: {s}");
    }
    println!();
    println!("{}", str(m, "body").trim_end());
}

pub fn thread(m: &Value, thread: &[Value]) {
    let earlier: Vec<&Value> = thread.iter().filter(|t| t["id"].as_i64() < m["id"].as_i64()).collect();
    for t in &earlier {
        println!("{}", dim(&format!("#{} from {}, {} ago: {}", t["id"], who(str(t, "from_addr")), age(str(t, "created_at")), first_line(t))));
    }
    if !earlier.is_empty() {
        println!();
    }
    message(m);
    let later: Vec<&Value> = thread.iter().filter(|t| t["id"].as_i64() > m["id"].as_i64()).collect();
    if !later.is_empty() {
        println!();
        for t in later {
            println!("{}", dim(&format!("#{} from {}, {} ago: {}", t["id"], who(str(t, "from_addr")), age(str(t, "created_at")), first_line(t))));
        }
    }
    if !str(m, "error").is_empty() {
        println!("{}", dim(&format!("error: {}", str(m, "error"))));
    }
}
