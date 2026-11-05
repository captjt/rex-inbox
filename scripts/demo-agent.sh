#!/bin/bash
# A stand-in agent for scripts/demo.sh: it reports a program status (OSC
# 7501) and a working folder (OSC 7), the way Claude Code and Codex do, and
# then waits. Any key "answers" it: 1 approves, anything else denies.
#
#   demo-agent.sh STATE APP KIND MSG TITLE FOLDER [CHILD_TITLES|...]
set -u
state=$1 app=$2 kind=$3 msg=$4 title=$5 folder=$6 children=${7:-}

b64() { printf '%s' "$1" | base64; }
report() { # state [kind] [msg] [id] [title]
  local f="state=$1:app=$app"
  [ -n "${2:-}" ] && f="$f:kind=$2"
  [ -n "${3:-}" ] && f="$f:msg=$(b64 "$3")"
  [ -n "${5:-}" ] && f="$f:title=$(b64 "$5")"
  [ -n "${4:-}" ] && f="id=$4:$f"
  printf '\033]7501;%s\033\\' "$f"
}

printf '\033]7;file://localhost%s\033\\' "$folder"
clear
printf 'A demo agent for a Rex Inbox screenshot (%s, %s).\n' "$app" "$state"
[ -n "$msg" ] && printf '\n  %s\n' "$msg"
printf '\nRemove the demo with: scripts/demo.sh down\n'

report "$state" "$kind" "$msg" "" "$title"
i=0
IFS='|' read -ra kids <<<"$children"
# bash 3.2 (macOS) calls an empty array unbound under set -u.
for k in ${kids[@]+"${kids[@]}"}; do
  [ -n "$k" ] && report working "" "" "task$i" "$k"
  i=$((i + 1))
done

while true; do
  # read is a builtin, so the agent stays the pane's only process.
  if ! IFS= read -rsn1 key; then
    read -rt 86400 _ </dev/zero 2>/dev/null || true
    continue
  fi
  if [ "$state" = blocked ]; then
    if [ "$key" = 1 ]; then echo "Approved."; else echo "Denied."; fi
    report working
    read -rt 2 -n1 _ || true
    report done "" "Finished after your answer"
    state=done
  fi
done
