# Contributing to rex-inbox

Thanks for looking. rex-inbox is small, macOS only for now, and needs
[Rex](https://www.superlogical.com/rex), which is in private beta. Read the
[README](README.md) for what it does and [DESIGN.md](DESIGN.md) for why it's
built the way it is. This page covers how to work on it.

Open an issue before a large change, so we can agree on the shape first.
Small fixes can go straight to a pull request.

## Setting up

You need what the README's [Requirements](README.md#requirements) list:
Rust (edition 2024), Swift 6 for the Mac app, and Rex. Screenshots also need
`jq`, `sqlite3`, Node 22 or later, and Google Chrome.

```sh
git clone https://github.com/captjt/rex-inbox
cd rex-inbox
cargo build              # the hub and the CLI, one binary
cargo test
swift build --package-path mac
```

To run your build as your everyday rex-inbox: `cargo install --path .`,
then `rex-inbox up` to restart the hub on it, and `mac/build.sh` for the
app.

## Where things are

| Path | What's there |
| --- | --- |
| `src/hub.rs`, `src/mirror.rs` | The hub: follows each Rex host's events and mirrors its sessions, panes and program status. |
| `src/store.rs` | SQLite: alerts, messages, the event log. Schema changes go in `migrate`. |
| `src/methods.rs`, `src/messages.rs` | The hub's methods, which the CLI, the apps and other hosts all call. |
| `src/main.rs`, `src/show.rs` | The CLI and how it prints. |
| `src/web.rs`, `src/webapp.rs`, `web/` | The phone web app: plain HTML, CSS and JavaScript, built into the binary. No build step and no dependencies. |
| `mac/` | The menu bar app, in SwiftUI. It talks to the hub over its socket. |
| `src/preview.rs` | Previews: apps agents share on the tailnet. |
| `src/approve.rs`, `src/reviewer.rs` | Guarded approvals, and rex-reviewer's reviews as items. |
| `src/hooks.rs`, `src/skill.rs`, `skills/rex-inbox/` | The agents' Stop hook, and the skill that teaches them rex-inbox. |
| `rex/init.lua` | The `inbox_rpc` action, for agents on other hosts. |
| `scripts/` | The made-up world for screenshots and demos (below). |

## Trying a change

**A hub of your own.** Your everyday hub keeps running; a second one, in
the foreground, keeps its state, socket and web port apart:

```sh
export REX_INBOX_STATE=/tmp/rex-inbox-dev REX_INBOX_SOCKET=/tmp/rex-inbox-dev.sock \
  REX_INBOX_HTTP=127.0.0.1:7440 REX_INBOX_NOTIFY=off
cargo run -- hub          # in one terminal
cargo run -- status       # in another, with the same variables
```

It follows your real Rex. Don't run `rex-inbox up` with these set:
`up` writes `REX_INBOX_STATE`, `REX_INBOX_SOCKET`, `REX_INBOX_NOTIFY` and
`REX_INBOX_REX` into your LaunchAgent, so your everyday hub would keep them.

**A made-up world.** `scripts/screenshots.sh --serve` runs a hub against a
fake Rex (`scripts/fake-rex`) with a project of stand-in agents, blocked
prompts, a review, messages and a preview. It serves `web/` from disk, so
reloading `http://127.0.0.1:7439/` shows a change to the phone app without a
rebuild. It never touches your own Rex, inbox or `tailscale serve`.

**In your own Rex.** `scripts/demo.sh up` fills your inbox with the same
kind of project as real Rex panes, and `scripts/demo.sh down` removes it.

**The Mac app** renders itself to a PNG against whatever hub
`REX_INBOX_SOCKET` names, with no window on screen:

```sh
swift build -c release --package-path mac
mac/.build/release/RexInbox --snapshot out.png --tab agents --appearance dark
```

### Environment variables

| Variable | What it does |
| --- | --- |
| `REX_INBOX_STATE` | Where the database lives (`~/.local/state/rex-inbox`). |
| `REX_INBOX_SOCKET` | The hub's socket (in the state folder). Keep it short: unix socket paths stop at about 100 bytes. |
| `REX_INBOX_HTTP` | The web app's address (`127.0.0.1:7430`), or `off`. |
| `REX_INBOX_NOTIFY` | `off` stops the hub's own Mac notifications. |
| `REX_INBOX_REX` | The `rex` binary the hub runs, such as `scripts/fake-rex`. |
| `REX_INBOX_WORLD` | The world file `scripts/fake-rex` answers from. |
| `REX_INBOX_HOSTS` | More hosts to follow, as `label=server,label`. |
| `REX_INBOX_VIA` | The Rex server the CLI reaches the hub through, from another host; `local` keeps to the socket. |
| `REX_INBOX_TAILSCALE` | The `tailscale` binary, or a stand-in so a test hub never changes your `tailscale serve`. |
| `REX_INBOX_OWNER` | The Tailscale login the web app answers. |
| `REX_INBOX_WEB_DEV` | Skips the web app's owner check, for local development only. |
| `REX_INBOX_WEB_DIR` | Serves the web app from this folder instead of the binary. |

## Before you open a pull request

```sh
cargo clippy --all-targets      # no warnings
cargo test
swift build --package-path mac
node --check web/app.js
```

- **Tests.** Unit tests sit beside the code in `#[cfg(test)]` modules, and
  their names read as sentences: `a_renamed_session_relabels_its_panes_items`.
  Add one for a behavior change, especially in parsing, the store, and
  anything that decides what reaches an agent's pane.
- **Look at it.** For a change to either app, run it and look, in light and
  dark, and the phone app at phone width (`scripts/phone-shot.mjs` takes a
  phone-sized shot of any page). Say in the pull request what you checked.
- **Screenshots.** When a change shows in the README's images, run
  `scripts/screenshots.sh` and commit the new `docs/images/`. Check them
  before committing: they're generated, and a broken one is easy to miss.
- **Formatting.** There's no `rustfmt.toml` yet, and the code doesn't match
  `rustfmt`'s defaults. Match the code around you, and don't run `cargo fmt`
  over the tree: it rewrites lines you didn't change.

## How the code is written

- **The hub stays a thin layer over Rex.** It talks to Rex only through the
  documented `rex` CLI, and keeps its own state in plain SQLite, so it's easy
  to drop when Rex grows these features itself (see DESIGN.md).
- **Keep the guards.** The web app answers only the Mac's Tailscale user.
  An approval sends its key only while the prompt is still the one you saw.
  A message goes into an agent's pane only when it's idle, never into a
  plain shell. Changes near these need a test, and a word about them in the
  pull request.
- **Never answer for the user.** Agents can message, wait and share, but
  approve and deny are for the person.
- **No new dependencies without a reason.** The web app and the Mac app have
  none; the hub has a few.
- **Comments say why**, at the top of each module and wherever the code
  alone wouldn't tell you.

## Words

What rex-inbox says is part of how it works, so it's written with care:
error messages, the CLI's output, the apps' labels, the skill, and the docs.

- Plain words and short sentences. Name things the way the apps do: items,
  Needs you, Finished, previews, panes, hosts.
- An error says what happened and what to do next: "nothing answers on port
  4999 on this Mac; start the app first", not "connection refused".
- The README is for people using rex-inbox, DESIGN.md records decisions and
  how the pieces fit, and comments are for the next person reading the code.

## The agent skill

`skills/rex-inbox/SKILL.md` is how Claude Code and Codex learn rex-inbox.
It's built into the binary, and `rex-inbox up` installs it and keeps
installed copies current. When you add or change something agents use,
update it in the same pull request. Keep its marker line (the comment under
the frontmatter): `up` refreshes only copies that carry it.

## Commits

- The subject is one plain sentence saying what changed for the person
  using rex-inbox, with no prefix and no closing period: "Answering a
  preview finishes it", "Agents on other hosts reach the hub through Rex".
- The body says why, then how, in a few short paragraphs.
- One change per commit. A change that touches the hub, both apps and the
  docs is still one commit.

## Releases

Releases are cut from `main`:

1. Set the version in `Cargo.toml`, then `cargo build` to update
   `Cargo.lock`. Commit both as "Version X.Y.Z".
2. Tag it: `git tag -a vX.Y.Z -m "rex-inbox X.Y"`, and push the tag.
3. `gh release create vX.Y.Z --title "rex-inbox X.Y"`, with notes in the
   shape of earlier releases: a line on what's new, then What's new,
   Changes (when something works differently), Upgrade, and Known gaps.

## Reporting a problem

Open an issue with what you did, what you expected, and what happened, plus
`rex-inbox status` and the end of `rex-inbox logs`. Leave out anything
private: they can name your sessions, folders and tailnet.

For a security problem, such as a way past the web app's owner check, a way
to approve or deny a prompt you didn't see, or a preview reachable by
someone it shouldn't be, don't open a public issue. Contact the maintainer,
captjt, privately first.

## License

rex-inbox is under the [MIT license](LICENSE). By contributing, you agree
your contribution is under it too.
