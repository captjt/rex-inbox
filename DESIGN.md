# rex-inbox design

Rex as a headless control plane for coding agents. Rex already runs the panes,
knows what every agent is doing (OSC 7501 program status), and joins machines
together (`rex hosts`, Tailscale). rex-inbox adds the layer on top: one inbox
for everything that needs you, messages between agents, and approvals you can
give from anywhere.

The idea of an inbox for your agents comes from
[Plannotator's Inbox](https://plannotator.ai/inbox), by
[@backnotprop](https://github.com/backnotprop). There, agents leave
questions, blockers and work, and you answer when you get to it. rex-inbox
fills its inbox from the program status that agents already report to Rex.

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
| Phone | A web app the hub serves, added to the Home Screen, published on the tailnet with `tailscale serve` (option B). Web Push for notifications. No Apple developer account, so no native iOS app. |
| Approvals | Guarded one-tap approve and deny (see below). |
| Repo | Open source, beside rex-reviewer. |

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

rex-inbox stands alone: it depends on no other tool, and works the same
without one. When rex-reviewer runs too, the two fit together (see "With
rex-reviewer" below). There is no TUI.

1. **Rex Inbox.app** (`mac/`), a SwiftUI menu bar app that talks to the hub
   over its socket.
   - The tray turns orange with a count while something needs you.
   - The Inbox tab has Needs you (Approve, Deny, Open, Snooze, Done),
     Messages (reply inline), Failed, and Finished.
   - The Agents tab lists every agent pane by session: click one to open
     it in Rex, or message it.
   - Notifications carry the answer: Approve and Deny for a Claude Code
     prompt, Reply for a message, Open for the rest. They are withdrawn once
     the item no longer needs you.
   - While the app is connected, the hub's own osascript notifications stay
     quiet.
2. The `rex-inbox` CLI, for agents and scripts, and for you in any terminal.
3. **The phone web app** (`web/`, built into the binary). See "Phone" below.
4. Later: Rex's own push for program status, when it ships.

## Phone

- **Reaching the hub.** The hub also listens on `127.0.0.1:7430` (set
  `REX_INBOX_HTTP`, or `off`). `tailscale serve --bg 7430` publishes it at
  `https://<mac>.<tailnet>.ts.net` with a real certificate, and adds the
  caller's `Tailscale-User-Login` header.
- **Who it answers.** Only the Mac's own Tailscale user, read from
  `tailscale status --json` (`REX_INBOX_OWNER` overrides). Requests must
  use the hub's own host names: loopback, or the tailnet name. That blocks
  DNS rebinding. Writes need an `X-Rex-Inbox` header and a same-site
  `Origin`, which blocks other sites from posting.
- **API.** `POST /api/call` runs the socket's methods, always as the person,
  from an allowlist: no naming panes, no focusing them, no stopping the
  hub. `GET /api/events` streams the hub's events (server-sent events).
- **The app.** Inbox and Agents tabs. Approve and Deny (with the same
  guard as the Mac), Reply, Snooze, Done, and Show pane (the last lines of
  a pane's screen) for any item or agent.
- **Notifications.** Web Push (RFC 8291 encryption, RFC 8292 VAPID).
  - The hub's VAPID key lives in `vapid.key` in the state dir; devices are
    in the `push_subs` table.
  - Pushes cover needs you, failed, and messages to you, after the same
    two-second grace. Tapping one opens that item.
  - iOS shows web notifications without action buttons, so you approve in
    the app.


## With rex-reviewer

Neither tool needs the other, and neither changes when the other is
missing. When both run, rex-inbox does the alerting, and rex-reviewer's menu
bar app stays its dashboard: rounds, the queue, the recap. They meet only in
what each already publishes. There are no imports and no shared code.

| rex-reviewer publishes | rex-inbox uses it to |
| --- | --- |
| One status record per PR on the watcher's pane: `app=rex-reviewer`, id `pr/<owner>/<name>/<pr>/<round>`. A review ready to post is `blocked` + `permission`. | Make each PR its own item. The answers are **Post**, **Dismiss**, and **Preview** rather than Approve and Deny. The item reads `name#pr · round r` rather than the watcher's pane. |
| Rex actions that take a record's id: `reviewer_post`, `reviewer_dismiss`, `reviewer_preview`, `reviewer_focus` with `item=`. They act only on the round the id names, and post only a round that waits for you. | Post, dismiss and preview. Open shows the PR's tab. |
| Its ledger: each PR's `session_id`, its reviewers' panes, and the checker's pane. | Fold those panes into the PR. They raise no Finished items and take no messages, they're left out of Agents, and their prompts and failures read `api#12 · codex reviewer`. |

- **Answers come from the hub.** Each alert carries `answers`: the methods
  a client may call and their labels. The Mac app, its notifications, the
  phone and the CLI show what the hub says, instead of each knowing which
  apps can be answered.
- **The guard is the same as for prompts.** The record must still be
  `blocked` on `permission`, with the same message and report time. Then
  rex-reviewer's action checks the round. After a post or a dismissal the
  item is set aside, until the watcher clears its record.
- **Another host.** The ledger is a local file. On another host, the agents
  in a session that holds a rex-reviewer pane count as its, named by their
  tab.
- **The badge.** rex-reviewer's app checks the hub's socket. While the hub
  answers, its icon turns orange only for a watcher that's down. rex-inbox
  can't see that case, because a stopped watcher reports nothing.

## Build steps

Each step is usable on its own.

1. Done: hub, mirror, alerts, store, `ls` (local host).
2. Done: messages (`send`, `inbox`, `read`, `reply`, `wait`), delivered into idle agent panes, and `me`.
3. Done: guarded approve and deny; `open`.
4. Done: notifications, LaunchAgent (`up`, `down`, `logs`), and the agent skill (in dotfiles, `agents/skills/rex-inbox`).
5. Done: the Mac menu bar app.
6. Done: the phone web app, HTTP API, and Web Push (waiting on Tailscale on the Mac to try on a phone).
7. Codex's prompt keys, checked against a live prompt.
8. Other hosts: done, a watcher per `rex hosts` entry, read again every 30
   seconds (`src/fleet.rs`). `REX_INBOX_HOSTS` adds hosts Rex doesn't list.
   Still to do: the `inbox_rpc` action, so agents there can use the CLI.
10. Done: start agents from the inbox (`agents.start`, `src/start.rs`) on any
    followed host: Claude Code or Codex in a new tab of the host's `inbox`
    session, in a folder checked through that host's `list_dir`.
9. Done: rex-reviewer's reviews as items, with its agents folded in (see "With rex-reviewer").

Tested on 2026-11-05: two Haiku agents traded messages through the hub, a
real Claude Code prompt was approved and another denied, and a prompt was
approved from the app.

## Known gaps

- The phone can't open a pane in the Rex iOS app (no documented deep link),
  so "Show pane" shows its screen instead.
- Claude Code's and Codex's folder-trust prompts at startup report no
  status, so they never reach the inbox. A pane running `claude` or `codex`
  is listed under Agents as `starting` until its agent reports, so an agent
  started from the inbox and stuck there can be found, but the prompt must
  be answered in Rex.
- Agents on other hosts can't use the `rex-inbox` CLI yet: it finds that
  machine's hub, or none. Messages to them say to answer in the pane.
- Codex's approval keys aren't verified yet, so Codex prompts get Open only.
- A message can land while you are typing into an idle agent's prompt. The
  two-second settle makes this unlikely, but not impossible.
- Default pane labels are all `shell`. Use `rex-inbox name` (or
  `rex-agent start NAME`) for panes others should reach.
- Without the app running, notifications fall back to osascript: they come
  from Script Editor, and clicking does nothing.
