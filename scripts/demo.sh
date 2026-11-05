#!/bin/bash
# Fill the inbox with a made-up project, for screenshots and demos:
# a Rex session "acme-api" of stand-in agents, plus messages to you.
# Nothing in it names a real repo, path or person.
#
#   scripts/demo.sh up     # start it (needs the hub running)
#   scripts/demo.sh down   # remove it
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
agent="$here/demo-agent.sh"
session=acme-api
folder="$HOME/code/acme-api"
db="${REX_INBOX_STATE:-$HOME/.local/state/rex-inbox}/inbox.db"

block_of() { # label
  rex api call -s "$session" session.list_blocks | jq -r --arg l "$1" '.blocks[] | select(.label == $l) | .block_id' | head -1
}

spawn() { # label STATE APP KIND MSG TITLE [CHILDREN]
  local label=$1
  shift
  rex run -s "$session" --label "$label" --focus=false -- "$agent" "$1" "$2" "$3" "$4" "$5" "$folder" "${6:-}" >/dev/null
}

backdate() { # table id minutes
  sqlite3 "$db" "UPDATE $1 SET created_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-$3 minutes') WHERE id = $2;"
}

alert_id() { # label
  rex-inbox ls --json | jq -r --arg l "$1" '[.alerts[] | select(.session_label == "acme-api" and .pane_label == $l)][0].id'
}

up() {
  rex-inbox status >/dev/null
  if rex ls 2>/dev/null | grep -q "$session"; then
    echo "the demo is already up; run: $0 down" >&2
    exit 1
  fi
  rex new "$session" --window planner -- "$agent" blocked claude-code permission \
    "approve Bash: npm run db:migrate -- --env staging" "" "$folder" >/dev/null
  rex api call -s "$session" session.set_block_label "$(jq -nc --arg b "$(rex api call -s "$session" session.list_blocks | jq -r '.blocks[0].block_id')" '{block_id: $b, label: "planner"}')" >/dev/null
  spawn reviewer blocked codex question "Which branch should this PR target: main or release/2.4?" ""
  spawn deploy blocked claude-code auth "Sign in to the GitHub CLI so I can push release/2.4" ""
  spawn tests error codex "" "3 tests fail in checkout.spec.ts after the cart refactor" ""
  spawn docs done claude-code "" "Updated the API reference for /v2/orders" ""
  spawn frontend working claude-code "" "" "Refactoring the checkout form" "Write form validation tests|Update Storybook stories"
  spawn search working codex "" "Reindexing 18,204 products" ""
  sleep 2

  REX_SESSION=$(rex api call session.list | jq -r --arg s "$session" '.sessions[] | select(.label == $s) | .session_id')
  export REX_SESSION
  REX_BLOCK=$(block_of planner) rex-inbox send --to me --subject "Pick a cache for sessions" \
    "Redis or an in-memory LRU for the session cache? I recommend the LRU: one process, no new service, and the sessions fit in memory. I'll keep building the orders API meanwhile." >/dev/null
  REX_BLOCK=$(block_of frontend) rex-inbox send --to me --subject "The checkout redesign is ready for a look" \
    "The new checkout form is on the checkout-v2 branch. Run npm run dev and open /checkout. The old form stays behind the legacy_checkout flag until you say otherwise." >/dev/null
  sleep 1

  # Spread the ages out, so it reads like an afternoon, not a second.
  backdate alerts "$(alert_id planner)" 2
  backdate alerts "$(alert_id reviewer)" 7
  backdate alerts "$(alert_id deploy)" 16
  backdate alerts "$(alert_id tests)" 11
  backdate alerts "$(alert_id docs)" 24
  for id in $(rex-inbox ls --json | jq -r '.messages[] | select(.from_addr | startswith("acme-api/")) | .id'); do
    case $(rex-inbox show "$id" --json | jq -r '.message.from_addr') in
      */planner) backdate messages "$id" 4 ;;
      *) backdate messages "$id" 13 ;;
    esac
  done
  # One more pane last, so the app refreshes and picks up the ages.
  spawn billing idle claude-code "" "" ""
  echo "demo is up: rex-inbox ls (remove it with: $0 down)"
}

down() {
  for id in $(rex-inbox ls --all --json | jq -r '.messages[] | select(.from_addr | startswith("acme-api/")) | .id'); do
    rex-inbox done "$id" >/dev/null || true
  done
  rex kill "$session" >/dev/null 2>&1 || true
  echo "demo removed"
}

case "${1:-}" in
  up) up ;;
  down) down ;;
  *) echo "usage: $0 up|down" >&2; exit 2 ;;
esac
