#!/bin/bash
# Render the README's screenshots and the social preview card from a made-up
# world, so nothing of yours shows up in them:
#
# - a Rex session "acme-api" of agents: one has a question, one failed, one
#   finished, one waits on your answer to its message, and others work;
# - rex-reviewer reviewing two PRs of acme/storefront: one ready to post, and
#   one whose Claude Code reviewer asks to run the tests;
# - a message to you.
#
# The hub follows that world through scripts/fake-rex, with a state folder,
# socket and port of its own. Your own Rex, hub and inbox are left alone.
# Needs jq, sqlite3, Node 22 or later, and Google Chrome. Run it again after
# changing a layout; upload docs/images/social-preview.png in the repo's
# Settings, under Social preview.
#
#   scripts/screenshots.sh           # render the screenshots
#   scripts/screenshots.sh --serve   # keep the made-up world's hub running,
#                                    # serving web/ from disk, to work on the
#                                    # phone app (Ctrl-C stops it)
set -euo pipefail
serve=""
[ "${1:-}" = --serve ] && serve=1

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
images="$root/docs/images"
demo=$(mktemp -d)
sock="/tmp/rex-inbox-shots-$$.sock"
port=7439
hub=""
cleanup() {
  [ -n "$hub" ] && kill "$hub" 2>/dev/null || true
  rm -rf "$demo" "$sock"
}
trap cleanup EXIT

cargo build --release --quiet --manifest-path "$root/Cargo.toml"
inbox="$root/target/release/rex-inbox"
if [ -z "$serve" ]; then
  swift build -c release --package-path "$root/mac" >/dev/null
  app="$(swift build -c release --package-path "$root/mac" --show-bin-path)/RexInbox"
fi
mkdir -p "$images"

# --- The world -------------------------------------------------------------

cat >"$demo/world.jq" <<'EOF'
# Times are in minutes before now.
def ago($m): (now - $m * 60) | floor | todate;

def pane($label; $cwd; $process; $records):
  {block_id: "block:\($label)", label: $label, cwd: $cwd, process: $process, records: $records};

# A root record, and a child one (a subagent, or one of rex-reviewer's PRs).
def root($state; $app; $m): {state: $state, app: $app, updated_at: ago($m)};
def child($id; $state; $app; $title; $m): {id: $id, state: $state, app: $app, title: $title, updated_at: ago($m)};

{
  sessions: [
    {session_id: "session:acme-api", label: "acme-api", windows: [
      {window_id: "window:planner", label: "planner", blocks: [
        pane("planner"; $api; "claude"; [root("idle"; "claude-code"; 4)]),
        pane("orders"; $api; "codex"; [root("blocked"; "codex"; 12)
          + {kind: "question", msg: "Which branch should this PR target: main or release/2.4?"}])
      ]},
      {window_id: "window:checkout", label: "checkout", blocks: [
        pane("frontend"; $api; "claude"; [root("working"; "claude-code"; 3) + {msg: "Refactoring the checkout form"},
          child("task1"; "working"; "claude-code"; "Write form validation tests"; 2),
          child("task2"; "working"; "claude-code"; "Update the Storybook stories"; 2)]),
        pane("tests"; $api; "codex"; [root("error"; "codex"; 9) + {msg: "3 tests fail in checkout.spec.ts after the cart refactor"}])
      ]},
      {window_id: "window:docs", label: "docs", blocks: [
        pane("docs"; $api; "claude"; [root("done"; "claude-code"; 20) + {msg: "Updated the API reference for /v2/orders"}]),
        pane("search"; $api; "codex"; [root("working"; "codex"; 5) + {msg: "Reindexing 18,204 products"}]),
        pane("billing"; $api; "claude"; [root("idle"; "claude-code"; 31)])
      ]}
    ]},
    {session_id: "session:rex-reviewer", label: "rex-reviewer", windows: [
      {window_id: "window:control", label: "control", blocks: [
        pane("watcher"; $reviewer; "rex"; [root("idle"; "rex-reviewer"; 40),
          child("pr/acme/storefront/212/2"; "blocked"; "rex-reviewer"; "storefront#212 Retry failed webhook deliveries with backoff"; 3)
            + {kind: "permission", msg: "Ready to post: Request changes (1 blocking, 2 non-blocking). Retries never stop when the endpoint answers 410 Gone."}])
      ]},
      {window_id: "window:storefront-212", label: "storefront-212", blocks: [
        pane("storefront-212"; $store; "codex"; [root("done"; "codex"; 9)]),
        pane("storefront-212-claude"; $store; "claude"; [root("done"; "claude-code"; 7)]),
        pane("storefront-212-check"; $store; "claude"; [root("idle"; "claude-code"; 3)])
      ]},
      {window_id: "window:storefront-214", label: "storefront-214", blocks: [
        pane("storefront-214"; $store; "codex"; [root("working"; "codex"; 4) + {msg: "Reviewing the cart's discount rules"}]),
        pane("storefront-214-claude"; $store; "claude"; [root("blocked"; "claude-code"; 1)
          + {kind: "permission", msg: "Bash: pnpm test --filter cart"}])
      ]}
    ]}
  ],
  previews: {
    "pr/acme/storefront/212/2": "**Request changes.** Retries never stop when the endpoint answers 410 Gone, so a deleted webhook is retried forever.\n\n**Blocking**\n\n- `deliver.ts`: treat 410 as final, and disable the endpoint.\n\n**Non-blocking**\n\n- The backoff has no jitter, so retries from one outage arrive together.\n- `retries.test.ts` never checks the delay between attempts.\n\n---\nCodex and Claude Code on behalf of @you · checked by Claude Code"
  }
}
EOF
jq -n --arg api "$HOME/code/acme-api" --arg store "$HOME/code/storefront" --arg reviewer "$HOME/src/rex-reviewer" \
  -f "$demo/world.jq" >"$demo/world.json"

# rex-reviewer's ledger: which panes review which PR.
mkdir -p "$demo/reviewer/repos/acme/storefront"
echo '{"schema": 1}' >"$demo/reviewer/state.json"
jq -n '{repo: "acme/storefront", pr: 212, round: 2, state: "reviewing", phase: "awaiting",
  session_id: "session:rex-reviewer", pane: "storefront-212", checker_pane: "storefront-212-check",
  reviewers: {codex: {pane: "storefront-212"}, claude: {pane: "storefront-212-claude"}}}' \
  >"$demo/reviewer/repos/acme/storefront/pr-212.json"
jq -n '{repo: "acme/storefront", pr: 214, round: 1, state: "reviewing", phase: "reviewer",
  session_id: "session:rex-reviewer", pane: "storefront-214",
  reviewers: {codex: {pane: "storefront-214"}, claude: {pane: "storefront-214-claude"}}}' \
  >"$demo/reviewer/repos/acme/storefront/pr-214.json"

# --- The hub, following the world --------------------------------------------

export REX_INBOX_STATE="$demo/state" REX_INBOX_SOCKET="$sock"
[ -n "$serve" ] && export REX_INBOX_WEB_DIR="$root/web"
REX_INBOX_REX="$here/fake-rex" REX_INBOX_WORLD="$demo/world.json" REX_REVIEWER_STATE_DIR="$demo/reviewer" \
  REX_INBOX_HTTP="127.0.0.1:$port" REX_INBOX_WEB_DEV=1 REX_INBOX_NOTIFY=off \
  "$inbox" hub >"$demo/hub.log" 2>&1 &
hub=$!
disown "$hub"
for _ in $(seq 1 50); do
  [ "$("$inbox" ls --json 2>/dev/null | jq '.alerts | length' 2>/dev/null || echo 0)" -ge 5 ] && break
  sleep 0.2
done

REX_SESSION=session:acme-api REX_BLOCK=block:planner "$inbox" send --to me --subject "Pick a cache for sessions" \
  "Redis or an in-memory LRU for the session cache? I recommend the LRU: one process, no new service, and the sessions fit in memory. I'll keep building the orders API meanwhile." >/dev/null

# Spread the ages out, so it reads like an afternoon, not a second.
db="$demo/state/inbox.db"
backdate() { # table id minutes
  sqlite3 "$db" "UPDATE $1 SET created_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-$3 minutes') WHERE id = $2;"
}
items=$("$inbox" ls --json)
alert() { jq -r --arg l "$1" '[.alerts[] | select(.pane_label == $l)][0].id' <<<"$items"; }
message() { jq -r --arg s "$1" '[.messages[] | select(.subject | startswith($s))][0].id' <<<"$items"; }
backdate alerts "$(alert watcher)" 3
backdate alerts "$(alert storefront-214-claude)" 1
backdate alerts "$(alert orders)" 12
backdate alerts "$(alert tests)" 9
backdate alerts "$(alert docs)" 20
backdate messages "$(message "Pick a cache")" 4

if [ -n "$serve" ]; then
  echo "The made-up world's web app: http://127.0.0.1:$port/ (web/ from disk; Ctrl-C stops it)"
  while kill -0 "$hub" 2>/dev/null; do sleep 1; done
  exit
fi

# --- The screenshots -------------------------------------------------------

for look in light dark; do
  "$app" --snapshot "$images/inbox-$look.png" --appearance "$look"
  "$app" --snapshot "$images/agents-$look.png" --tab agents --appearance "$look"
  node "$here/phone-shot.mjs" "http://127.0.0.1:$port/" "$images/phone-$look.png" "$look"
done
echo "wrote $images/{inbox,agents,phone}-{light,dark}.png"

# The social preview card shows inbox-dark.png, so render it again too.
chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
"$chrome" --headless=new --disable-gpu --hide-scrollbars --window-size=1280,640 \
  --screenshot="$images/social-preview.png" "file://$images/social-preview.html" >/dev/null 2>&1
echo "wrote $images/social-preview.png"
