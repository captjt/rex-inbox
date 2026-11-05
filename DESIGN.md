# rex-inbox design

Rex as a headless control plane for coding agents. Rex already runs the panes,
knows what every agent is doing (OSC 7501 program status), and joins machines
together (`rex hosts`, Tailscale). rex-inbox adds the layer on top: one inbox
for everything that needs you, messages between agents, and approvals you can
give from anywhere.

Rex's docs list "a unified priority inbox view" and mobile push for program
status as future features. rex-inbox keeps its state in plain SQLite and talks
to Rex only through the documented `rex` CLI, so it stays a thin layer that is
easy to drop when Rex ships its own.

## Decisions (2026-11-05)

| Question | Decision |
| --- | --- |
| Language | Rust, one binary (`rex-inbox`): hub daemon, CLI, and later the TUI. |
| Platform | macOS first, local only. No cross-compiling until this is proven. |
| Machines | One hub on the home Rex server. Every record carries a `host` so other Rex hosts can be added later without a schema change. |
| Phone push | None for now. Mac notifications, plus the Rex iOS app over Rex's own Tailscale once that's on. |
| Approvals | Guarded one-tap approve and deny (see below). |
| Repo | On GitHub at captjt/rex-inbox. |

## Pieces

```
 agents (Claude Code, Codex, scripts)          you (Mac, phone via Rex iOS)
   │ OSC 7501 status     │ rex-inbox send/read     │ rex-inbox ls / tui / menu bar
   ▼                     ▼                         ▼
 Rex server  ──events──▶  rex-inbox hub  ◀──unix socket (JSON lines)──
   ▲                       │  SQLite store (~/.local/state/rex-inbox)
   └──paste / keys─────────┘  macOS notifications
```

- **Hub** (`rex-inbox hub`): a long-running process, kept alive by a
  LaunchAgent (`rex-inbox up`). For each Rex host it follows
  `rex -S <host> events --all --json` and keeps a mirror of every session,
  window and pane (labels, cwd, foreground process) and every pane's program
  status records. It reconnects with backoff and resnapshots on reconnect
  (the terminal block's `program_status` method returns current records).
- **Store**: SQLite at `~/.local/state/rex-inbox/inbox.db`: alerts, messages,
  and an append-only event log for history.
- **API**: JSON lines over a unix socket (`~/.local/state/rex-inbox/hub.sock`).
  One request per connection: `{"method": "...", "params": {...}}` in,
  `{"ok": true, "result": ...}` out. Streaming methods (`watch`, `wait`) write
  one line per update. The CLI, the TUI and the menu bar all use it. An HTTP
  and server-sent events listener for native apps comes later and calls the
  same handlers.
- **Remote agents (later)**: the home server's `init.lua` defines one action,
  `inbox_rpc`, that pipes `{"method","params"}` to `rex-inbox rpc`. On a
  machine without a local hub, the CLI sends the same request through
  `rex -S <home> do inbox_rpc`. Rex handles identity and transport, so no
  ports are opened and remote machines need only `rex` and the CLI.

## Alerts (from program status, no agent changes)

Each alert is keyed by `host / block / record id` and mirrors a live record:

| Record state | Alert | Notes |
| --- | --- | --- |
| `blocked` | **needs you** | `kind` = permission, question or auth; `msg` says what. |
| `error` | **failed** | |
| `done` | **finished** | Rex drops `done` once you go back to the pane and press a key, so these clear themselves. |
| `idle`, `working`, removed, pane closed | alert resolves | Kept in history with `resolved_at`. |

Child records (Claude Code subagents report one per task, with a `title`) roll
up into their pane: a pane shows its most urgent record.

## Messages

- **Addresses**: `me` is you. An agent is a pane: its label within the
  sender's session (`reviewer`), `session/pane`, `host:session/pane`, or a
  block id. Ambiguous labels are an error that lists the candidates.
- **Sending**: `rex-inbox send --to reviewer|me [--subject …] [--reply-to N] body|-`.
  The sender's address comes from `REX_SESSION` and `REX_BLOCK`.
- **Delivery to agents**: messages queue in the hub. A message is delivered
  into the recipient's pane when the pane's root record is `idle` or `done`
  and its app is a known agent (`claude-code`, `codex`). Never into a plain
  shell, where typed text would run as a command. Delivery is a paste event
  plus Enter, the same as `rex-agent prompt`. The pasted text is a short
  notice with the message (or a pointer, `rex-inbox read N`, when it is long).
  Agents can also pull: `rex-inbox inbox`, `read N`, `wait`.
- **Messages to `me`** show in the inbox beside alerts and raise a Mac
  notification. Your reply is delivered into the sender's pane the same way.

## Guarded approve

An alert for a `blocked` + `permission` record offers Approve and Deny.

1. The alert stores the record's `updated_at` and `msg`, plus a capture of the
   pane's screen at the time, so you see what you are approving.
2. On Approve, the hub reads the pane's `program_status` again. It sends the
   key only if the same record is still `blocked`, `kind=permission`, with
   the same `updated_at` and `msg`. Otherwise it refuses and shows the new
   state.
3. Keys are per app (`claude-code`, `codex`), verified against live prompts.
   Unknown apps get Open only.
4. After sending, the hub waits a few seconds for the record to leave
   `blocked` and reports whether it did.

## Surfaces

1. `rex-inbox ls | show N | open N | approve N | deny N | done N | snooze N 1h`.
2. A TUI (`rex-inbox tui`) that runs in a Rex pane: on the Mac and, through
   the Rex iOS app, on the phone.
3. Mac notifications for new **needs you** items and messages to `me`
   (rate-limited).
4. The menu bar app moves here from rex-reviewer, as "Rex Inbox", with
   notification actions for Approve and Deny.
5. Native Mac and iOS apps, on the HTTP API.

## Build steps

Each step is usable on its own.

1. Hub, mirror, alerts, store, `ls` (local host).
2. Messages: `send`, `inbox`, `read`, `reply`, `wait`, delivery into idle agent panes, `me`.
3. Guarded approve and deny; `open` (focus the pane in the Rex app).
4. Mac notifications, LaunchAgent (`up`, `down`), TUI.
5. Skill for agents (`agents/skills/rex-inbox` in dotfiles) and the allowlist.
6. Menu bar move from rex-reviewer; rex-reviewer feeds its attention items in.
7. Other hosts: a watcher per `rex hosts` entry and the `inbox_rpc` action.
