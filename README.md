# rex-inbox

Everything your Rex agents need from you, and messages between them.

[Rex](https://www.superlogical.com/rex) already runs your agents' panes and
knows what each one is doing: Claude Code and Codex report `working`,
`blocked`, `done` and `error` on their own (OSC 7501). rex-inbox turns that
into one inbox, lets agents message each other and you, and lets you
answer permission prompts without going to the pane.

```
$ rex-inbox ls
Needs you
  #17  permission  Claude Code  solar-isle/alpha  ~/git/app  12s  approve Bash: rm approved.txt
Finished
  #12              Claude Code  restless-cliff/shell  ~/git/web  4m
Messages
  #7   unread  from solar-isle/planner  1m  Pick a cache

$ rex-inbox approve 17
#17 approved; the pane is now working
```

## Install

```sh
cargo install --path .
rex-inbox up          # the hub, as a LaunchAgent (logs: rex-inbox logs)
```

macOS only for now. The hub talks to Rex only through the `rex` CLI.

## Commands

| | |
| --- | --- |
| `ls [--all] [--history]` | What needs you: blocked, failed and finished agents, and messages to you. |
| `agents [--all]` | Every agent pane, where it is, and what it's doing. |
| `show N [--screen]` | One item; `--screen` adds the bottom of its pane. |
| `approve N`, `deny N` | Answer a permission prompt, only if it's still the one `N` shows. |
| `open N` | Bring the item's pane to the front in Rex. |
| `done N`, `snooze N 1h`, `reopen N` | File an item. |
| `send --to NAME [--subject S] TEXT\|-` | Message an agent's pane (`reviewer`, `session/pane`, a block id) or `me`. |
| `reply N TEXT\|-` | Answer a message; it goes to the other side. |
| `inbox`, `read N`, `wait [--thread N] [--timeout S]` | An agent's own messages. |
| `name NAME`, `whoami` | Name this pane so others can reach it. |
| `status`, `watch`, `up`, `down`, `logs` | The hub. |

## How it works

- **Alerts** mirror program status. A `blocked` record becomes **needs you**,
  `error` becomes **failed**, and a root `done` becomes **finished**. Each
  clears itself when the record changes or the pane closes. Rex drops `done`
  once you visit the pane.
- **Messages** queue in the hub. A message is pasted into an agent's prompt
  (then Enter) once the pane has been `idle` or `done` for two seconds. Plain
  shell panes are never typed into.
- **Approve and deny** re-read the prompt from Rex first. Keys go only if
  it's the same `blocked` permission record, with the same message and
  report time, and the only prompt in that pane. Claude Code gets `1` and
  Escape. Other agents can only be opened, until their prompts are checked.
- **Notifications** cover needs you, failed, and messages to you. Each
  waits two seconds, and is dropped if you already answered.

State lives in `~/.local/state/rex-inbox` (SQLite and the hub's socket).
See [DESIGN.md](DESIGN.md) for the decisions and what comes next.
