"use strict";

// Rex Inbox on the phone: the hub's methods over /api/call, live updates
// over /api/events, and Web Push for what needs you. Cards answer what they
// can in place; a tap opens a sheet with the rest (the full prompt, the pane's
// screen, a review as it will post, a reply box).

const state = {
  tab: "inbox",
  filter: "all",
  alerts: [],
  messages: [],
  panes: [],
  conn: "wait", // wait | on | off
  loaded: false,
  showFinished: false,
  expanded: new Set(), // item ids showing all their text
  busy: new Map(), // item id -> the action running
  drafts: {},
  sheet: null, // what the sheet shows: {type: "item"|"message"|"pane"|"menu"|"new", ...}
  // The new-agent sheet's form; kept between openings, less the prompt once sent.
  draft: { host: "local", program: "claude", cwd: "", prompt: "", name: "", session: "" },
  hosts: [],
  hold: 0, // > 0 while a swipe or an animation runs: refreshes wait
  pending: false,
};

const $ = (id) => document.getElementById(id);
const main = $("main");
const sheet = $("sheet");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---- icons (24x24, stroked) ----

const PATHS = {
  check: '<path d="M5 12.5l4.5 4.5L19 7.5"/>',
  x: '<path d="M6.5 6.5l11 11M17.5 6.5l-11 11"/>',
  hand: '<path d="M18 11V6.5a1.75 1.75 0 0 0-3.5 0V11"/><path d="M14.5 10V4.75a1.75 1.75 0 0 0-3.5 0V10"/><path d="M11 10.5V6a1.75 1.75 0 0 0-3.5 0v8"/><path d="M18 8.5a1.75 1.75 0 0 1 3.5 0V14a8 8 0 0 1-8 8h-1.6c-2.4 0-3.9-.8-5.3-2.2l-3.4-3.5a1.8 1.8 0 0 1 2.6-2.5L7.5 15.5"/>',
  question: '<path d="M21 11.5a8.5 8.5 0 0 1-12.6 7.4L3.5 20.5l1.6-4.6A8.5 8.5 0 1 1 21 11.5z"/><path d="M9.8 9.2a2.3 2.3 0 0 1 4.4.8c0 1.5-2.2 1.9-2.2 3.2"/><path d="M12 16.3h.01"/>',
  key: '<circle cx="7.5" cy="15.5" r="4.5"/><path d="M10.7 12.3L20 3M16.5 6.5l3 3M14 9l2 2"/>',
  failed: '<path d="M8 2.5h8l5.5 5.5v8L16 21.5H8L2.5 16V8z"/><path d="M15 9l-6 6M9 9l6 6"/>',
  done: '<circle cx="12" cy="12" r="9.25"/><path d="M8 12.4l2.8 2.8L16.2 9.6"/>',
  mail: '<rect x="2.75" y="5" width="18.5" height="14" rx="3"/><path d="M3.5 7.5l8.5 6 8.5-6"/>',
  mailOpen: '<path d="M2.75 10l9.25-6.5 9.25 6.5v7.5a3 3 0 0 1-3 3H5.75a3 3 0 0 1-3-3z"/><path d="M3.5 10.5l8.5 6 8.5-6"/>',
  inbox: '<path d="M2.75 13.5h5l1.75 2.75h5l1.75-2.75h5"/><path d="M5.6 5.3A2 2 0 0 1 7.4 4.25h9.2a2 2 0 0 1 1.8 1.05l2.85 5.6v6.85a2 2 0 0 1-2 2H4.75a2 2 0 0 1-2-2v-6.85z"/>',
  agents: '<rect x="2.75" y="4" width="18.5" height="16" rx="3.5"/><path d="M7 9.5l3 2.5-3 2.5M12.5 15h4.5"/>',
  more: '<circle cx="5.5" cy="12" r="1.6" class="f"/><circle cx="12" cy="12" r="1.6" class="f"/><circle cx="18.5" cy="12" r="1.6" class="f"/>',
  clock: '<circle cx="12" cy="12" r="9.25"/><path d="M12 7v5.2l3.2 2"/>',
  doc: '<path d="M14 2.75H7.25a2 2 0 0 0-2 2v14.5a2 2 0 0 0 2 2h9.5a2 2 0 0 0 2-2V7.5z"/><path d="M14 2.75V7.5h4.75M8.75 12.5h6.5M8.75 16.5h4.5"/>',
  send: '<path d="M12 19.5V5M6 11l6-6 6 6"/>',
  reply: '<path d="M9.5 14.5L4.5 9.5l5-5"/><path d="M4.5 9.5h10a5.5 5.5 0 0 1 0 11h-3"/>',
  bell: '<path d="M6 9a6 6 0 1 1 12 0c0 6.5 2.75 8.5 2.75 8.5H3.25S6 15.5 6 9"/><path d="M10.2 21a2 2 0 0 0 3.6 0"/>',
  chevron: '<path d="M9.5 5.5l6.5 6.5-6.5 6.5"/>',
  refresh: '<path d="M20.25 12a8.25 8.25 0 1 1-2.4-5.8"/><path d="M20.25 3.75v5h-5"/>',
  wrap: '<path d="M3.5 6h17M3.5 12h14a3 3 0 0 1 0 6H12"/><path d="M14 15.5l-2.5 2.5 2.5 2.5M3.5 18h5"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  folder: '<path d="M2.75 7.25a2 2 0 0 1 2-2h4.6l2 2.5h7.9a2 2 0 0 1 2 2v8.5a2 2 0 0 1-2 2H4.75a2 2 0 0 1-2-2z"/>',
  up: '<path d="M12 19V5M6 11l6-6 6 6"/>',
  wifi: '<path d="M2.5 9a14 14 0 0 1 19 0M5.5 12.5a9.5 9.5 0 0 1 13 0M8.75 16a5 5 0 0 1 6.5 0"/><path d="M12 19.5h.01"/>',
};
function ic(name, cls = "") {
  const p = (PATHS[name] || "").replace(/class="f"/g, 'fill="currentColor" stroke="none"');
  return `<svg class="i ${cls}" viewBox="0 0 24 24" aria-hidden="true">${p}</svg>`;
}

// ---- the hub ----

async function call(method, params = {}) {
  const r = await fetch("/api/call", {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Rex-Inbox": "1" },
    body: JSON.stringify({ method, params }),
  });
  if (!r.ok) throw new Error((await r.text()) || r.statusText);
  const j = await r.json();
  if (!j.ok) throw new Error(j.error || "failed");
  return j.result;
}

async function refresh() {
  try {
    const [inbox, panes] = await Promise.all([call("inbox"), call("panes.list")]);
    state.alerts = inbox.alerts;
    state.messages = inbox.messages;
    state.panes = panes;
    state.loaded = true;
    state.problem = null;
    if (state.conn !== "on" && events && events.readyState === 1) state.conn = "on";
  } catch (e) {
    state.conn = "off";
    state.problem = e.message;
  }
  render();
}

let timer = null;
function soon(delay = 150) {
  clearTimeout(timer);
  timer = setTimeout(() => {
    // Don't redraw under a finger, or under someone typing in the list.
    if (state.hold > 0 || main.contains(document.activeElement)) {
      state.pending = true;
      return;
    }
    refresh();
  }, delay);
}
function release() {
  state.hold = Math.max(0, state.hold - 1);
  if (!state.hold && state.pending) {
    state.pending = false;
    soon();
  }
}

let events = null;
function live() {
  if (events && events.readyState !== 2) return;
  events = new EventSource("/api/events");
  events.onmessage = () => soon();
  events.onopen = () => {
    state.conn = "on";
    soon(0);
  };
  events.onerror = () => {
    state.conn = events.readyState === 2 ? "off" : "wait";
    paintSummary();
  };
}

// ---- formatting ----

function esc(s) {
  return String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

function when(ts) {
  if (!ts) return null;
  // Rex sends microseconds; keep milliseconds so every browser parses it.
  const d = new Date(String(ts).replace(/(\.\d{3})\d+/, "$1"));
  return isNaN(d) ? null : d;
}

function age(ts, long = false) {
  const d = when(ts);
  if (!d) return "";
  const s = Math.max(0, Math.floor((Date.now() - d) / 1000));
  let a;
  if (s < 60) a = long ? "just now" : "now";
  else if (s < 3600) a = `${Math.floor(s / 60)}m`;
  else if (s < 86400) a = `${Math.floor(s / 3600)}h`;
  else a = `${Math.floor(s / 86400)}d`;
  return long && s >= 60 ? `${a} ago` : a;
}

const tilde = (p) => (p ? String(p).replace(/^\/Users\/[^/]+/, "~") : "");
const shortId = (id) => String(id || "").split(/[-:]/).pop().slice(-6);
const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;

function appName(a) {
  return { "claude-code": "Claude Code", codex: "Codex", "rex-reviewer": "Rex Reviewer" }[a] || a || "A program";
}

// A pane another tool runs goes by what it does there, such as
// "api#12 · codex reviewer".
function place(x) {
  if (x.subject) return !x.host || x.host === "local" ? x.subject : `${x.host}:${x.subject}`;
  const s = x.session_label || shortId(x.session_id);
  const p = x.pane_label || x.label || shortId(x.block_id);
  const base = `${s}/${p}`;
  return !x.host || x.host === "local" ? base : `${x.host}:${base}`;
}

function headline(a) {
  const app = appName(a.app);
  if (a.category === "failed") return `${app} failed`;
  if (a.category === "finished") return `${app} finished`;
  if (a.app === "rex-reviewer" && a.kind === "permission") return "Review ready to post";
  return { permission: `${app} needs your approval`, question: `${app} has a question`, auth: `${app} needs you to sign in`, startup: `${app} is waiting to start` }[a.kind] || `${app} needs you`;
}

const what = (x) => [x.title, x.msg].filter(Boolean).join(": ");
// The hub says what an item can be answered with: approve and deny (named
// Approve and Deny, or Post and Dismiss for a review), and preview.
const answers = (a) => a.answers || [];
const decisions = (a) => answers(a).filter((x) => x.action !== "preview");
const canPreview = (a) => answers(a).some((x) => x.action === "preview");
const isReview = (a) => a.app === "rex-reviewer";
const isCommand = (a) => a.category === "needs_you" && a.kind === "permission" && !isReview(a);

function alertLook(a) {
  if (a.category === "failed") return ["failed", "t-red"];
  if (a.category === "finished") return ["done", "t-green"];
  return [{ permission: isReview(a) ? "doc" : "hand", question: "question", auth: "key", startup: "clock" }[a.kind] || "hand", "t-orange"];
}

const STATE_TINT = { blocked: "t-orange", error: "t-red", working: "t-blue", done: "t-green" };
const tint = (s) => STATE_TINT[s] || "t-gray";

function messageFrom(m) {
  return m.from_block ? m.from_addr : "you";
}

const paneKey = (p) => `${p.host}|${p.block_id}`;
const paneName = (p) => p.label || shortId(p.block_id);
const paneSession = (p) => p.session_label || shortId(p.session_id);
const isLocal = (x) => !x.host || x.host === "local";
// What the Agents list groups by: the session, and its host when it's another machine.
const paneGroup = (p) => (isLocal(p) ? paneSession(p) : `${p.host} · ${paneSession(p)}`);

// Markdown's common parts, for a review as it will post: paragraphs, bullets,
// headings, rules, bold, italics, code and links (shown as text).
function md(text) {
  const inline = (s) =>
    esc(s)
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>")
      .replace(/(^|[^*\w])\*([^*\n]+)\*/g, "$1<i>$2</i>")
      .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1");
  const out = [];
  let para = [];
  let list = [];
  const flush = () => {
    if (para.length) out.push(`<p>${para.map(inline).join(" ")}</p>`);
    if (list.length) out.push(`<ul>${list.map((l) => `<li>${inline(l)}</li>`).join("")}</ul>`);
    para = [];
    list = [];
  };
  const lines = String(text || "").split("\n");
  const lastRule = lines.map((l) => l.trim()).lastIndexOf("---");
  lines.forEach((raw, i) => {
    const l = raw.trimEnd();
    if (/^\s*(---|\*\*\*|___)\s*$/.test(l)) {
      flush();
      out.push(i === lastRule ? '<hr><div class="foot">' : "<hr>");
    } else if (/^#{1,6}\s/.test(l)) {
      flush();
      out.push(`<h4>${inline(l.replace(/^#+\s*/, ""))}</h4>`);
    } else if (/^\s*[-*+]\s+/.test(l)) {
      if (para.length) flush();
      list.push(l.replace(/^\s*[-*+]\s+/, ""));
    } else if (!l.trim()) {
      flush();
    } else if (list.length && /^\s{2,}/.test(raw)) {
      list[list.length - 1] += " " + l.trim();
    } else {
      if (list.length) flush();
      para.push(l);
    }
  });
  flush();
  if (lastRule >= 0) out.push("</div>");
  return out.join("");
}

// ---- the page ----

function counts() {
  const needs = state.alerts.filter((a) => a.category === "needs_you");
  const failed = state.alerts.filter((a) => a.category === "failed");
  const finished = state.alerts.filter((a) => a.category === "finished");
  return { needs, failed, finished, messages: state.messages };
}

function paintSummary() {
  const { needs, failed, messages } = counts();
  const dot = document.querySelector("#summary .dot");
  let text;
  if (state.conn === "off") {
    text = state.loaded ? "Can't reach your Mac" : "Not connected";
    dot.className = "dot off";
  } else if (!state.loaded) {
    text = "Connecting…";
    dot.className = "dot wait";
  } else {
    const parts = [];
    const working = state.panes.filter((p) => p.state === "working").length;
    if (state.tab === "agents") {
      parts.push(plural(state.panes.length, "agent"));
      if (working) parts.push(`${working} working`);
      const waiting = state.panes.filter((p) => p.state === "blocked").length;
      if (waiting) parts.push(`${waiting} waiting on you`);
    } else {
      if (needs.length) parts.push(`${needs.length} need${needs.length === 1 ? "s" : ""} you`);
      if (messages.length) parts.push(plural(messages.length, "message"));
      if (failed.length) parts.push(`${failed.length} failed`);
      if (!parts.length) parts.push("All clear", working ? `${working} working` : plural(state.panes.length, "agent"));
    }
    text = parts.join(" · ");
    dot.className = state.conn === "on" ? "dot" : "dot wait";
  }
  $("summary-text").textContent = text;
  document.title = needs.length ? `(${needs.length}) Rex Inbox` : "Rex Inbox";
}

function render() {
  const c = counts();
  const title = state.tab === "inbox" ? "Inbox" : "Agents";
  $("title").textContent = title;
  $("topbar-title").textContent = title;
  paintSummary();
  paintTabbar(c);
  paintFilters(c);
  paintNotice();
  if (!main.contains(document.activeElement)) {
    main.innerHTML = state.tab === "inbox" ? inboxView(c) : agentsView();
    markClamped(main);
  }
  if (state.sheet) refreshSheet();
}

function paintTabbar(c) {
  const bar = $("tabbar");
  if (!bar.firstChild) {
    bar.innerHTML = [
      ["inbox", "inbox", "Inbox"],
      ["agents", "agents", "Agents"],
    ]
      .map(([tab, icon, label]) => `<button role="tab" data-tab="${tab}">${ic(icon)}<span>${label}</span><b class="count" hidden></b></button>`)
      .join("");
  }
  for (const b of bar.querySelectorAll("button")) {
    const on = b.dataset.tab === state.tab;
    b.classList.toggle("on", on);
    b.setAttribute("aria-selected", on);
  }
  const badge = bar.querySelector('[data-tab="inbox"] .count');
  const n = c.needs.length + c.messages.length;
  badge.hidden = !n;
  badge.textContent = n > 99 ? "99+" : n;
}

const FILTERS = [
  ["all", "All"],
  ["needs", "Needs You"],
  ["messages", "Messages"],
  ["failed", "Failed"],
  ["finished", "Finished"],
];

function paintFilters(c) {
  const el = $("filters");
  const n = { needs: c.needs.length, messages: c.messages.length, failed: c.failed.length, finished: c.finished.length };
  n.all = n.needs + n.messages + n.failed + n.finished;
  const kinds = FILTERS.filter(([k]) => k !== "all" && n[k] > 0).length;
  if (state.filter !== "all" && !n[state.filter]) state.filter = "all";
  el.hidden = state.tab !== "inbox" || kinds < 2;
  if (el.hidden) return;
  el.innerHTML = FILTERS.filter(([k]) => k === "all" || n[k] > 0 || k === state.filter)
    .map(([k, label]) => `<button class="chip ${state.filter === k ? "on" : ""}" data-filter="${k}">${label} <span class="n">${n[k]}</span></button>`)
    .join("");
}

function inboxView(c) {
  if (!state.loaded) return state.conn === "off" ? offline() : "";
  if (!state.alerts.length && !state.messages.length) {
    const working = state.panes.filter((p) => p.state === "working").length;
    const detail = working
      ? `${plural(working, "agent")} working. You'll hear when one needs you.`
      : "Blocked, failed and finished agents show up here, with messages to you.";
    return empty("done", "t-green", "Nothing needs you", detail);
  }
  const list = (items, card) => items.map(card).join("");
  switch (state.filter) {
    case "needs":
      return list(c.needs, alertCard);
    case "messages":
      return list(c.messages, messageCard);
    case "failed":
      return list(c.failed, alertCard);
    case "finished":
      return list(c.finished, alertCard);
  }
  let html = "";
  if (c.needs.length) html += section("Needs you", c.needs.length) + list(c.needs, alertCard);
  if (c.messages.length) html += section("Messages", c.messages.length) + list(c.messages, messageCard);
  if (c.failed.length) html += section("Failed", c.failed.length) + list(c.failed, alertCard);
  if (c.finished.length) {
    html += `<h2><button data-act="toggle-finished" class="${state.showFinished ? "open" : ""}">Finished <span class="n">${c.finished.length}</span>${ic("chevron")}</button></h2>`;
    if (state.showFinished) html += list(c.finished, alertCard);
  }
  return html;
}

const section = (title, n) => `<h2>${esc(title)} <span class="n">${n}</span></h2>`;

function empty(icon, tone, title, detail) {
  return `<div class="empty"><div class="halo ${tone}">${ic(icon)}</div><h3>${esc(title)}</h3><p>${esc(detail)}</p></div>`;
}

function offline() {
  return empty("wifi", "t-gray", "Can't reach your Mac", "Check that the Mac is awake and on your tailnet, and that the rex-inbox hub is running. This page reconnects by itself.");
}

function busyLabel(id, action, label, icon) {
  return state.busy.get(id) === action ? '<span class="spin"></span>' : `${icon ? ic(icon) : ""}${esc(label)}`;
}

function decisionButtons(a, size = "") {
  const busy = state.busy.has(a.id) ? "disabled" : "";
  return decisions(a)
    .map((x) => {
      const approve = x.action === "approve";
      return `<button class="btn wide ${size} ${approve ? "approve" : "deny"}" data-act="answer" data-action="${esc(x.action)}" data-id="${a.id}" ${busy}>${busyLabel(a.id, x.action, x.label, approve ? "check" : "x")}</button>`;
    })
    .join("");
}

// What an item says: its title (less the PR the meta line already names) over
// its message, in a box when it waits on you (a command in monospace).
function alertBody(a, clamp) {
  let title = a.title || "";
  const ref = String(a.subject || "").split(/\s/)[0];
  if (ref && title.startsWith(ref + " ")) title = title.slice(ref.length + 1);
  const text = a.msg || title;
  if (!text) return "";
  const cut = clamp && !state.expanded.has(a.id) ? "clamp" : "";
  const mark = clamp ? `data-clamp="${a.id}"` : "";
  const head = a.msg && title ? `<div class="subject">${esc(title)}</div>` : "";
  return (
    head +
    (a.category === "needs_you"
      ? `<div class="what ${isCommand(a) ? "code" : ""} ${cut}" ${mark}>${esc(text)}</div>`
      : `<div class="prose ${cut}" ${mark}>${esc(text)}</div>`)
  );
}

function alertCard(a) {
  const [icon, tone] = alertLook(a);
  const busy = state.busy.has(a.id) ? "disabled" : "";
  const body = alertBody(a, true);
  let actions;
  if (decisions(a).length) {
    actions = decisionButtons(a) + `<button class="btn circle" data-act="menu" data-id="${a.id}" aria-label="More">${ic("more")}</button>`;
  } else if (a.category === "needs_you") {
    actions = `<button class="btn wide quiet" data-act="open" data-id="${a.id}">${ic("agents")}Show pane</button><button class="btn circle" data-act="menu" data-id="${a.id}" aria-label="More">${ic("more")}</button>`;
  } else {
    actions = `<button class="btn wide quiet" data-act="open" data-id="${a.id}">${ic("agents")}Show pane</button><button class="btn wide" data-act="done" data-id="${a.id}" ${busy}>${busyLabel(a.id, "done", "Done", "check")}</button>`;
  }
  return swipeable(a.id, a.category === "needs_you", `
    <article class="card" id="item-${a.id}" data-open="${a.id}">
      <div class="top">
        <div class="badge-icon ${tone}">${ic(icon)}</div>
        <div class="titles">
          <div class="head"><b>${esc(headline(a))}</b><span class="age">${age(a.created_at)}</span></div>
          <div class="meta">${esc(place(a))}${a.cwd ? " · " + esc(tilde(a.cwd)) : ""}</div>
        </div>
      </div>
      ${body}
      <div class="actions">${actions}</div>
    </article>`);
}

function messageCard(m) {
  const from = messageFrom(m);
  const unread = m.status === "unread";
  const open = state.expanded.has(m.id);
  const busy = state.busy.has(m.id) ? "disabled" : "";
  const meta = [m.reply_to ? `replying to #${m.reply_to}` : "", `#${m.id}`].filter(Boolean).join(" · ");
  return swipeable(m.id, false, `
    <article class="card" id="item-${m.id}" data-open="${m.id}">
      <div class="top">
        <div class="badge-icon ${unread ? "t-blue" : "t-gray"}">${ic(unread ? "mail" : "mailOpen")}</div>
        <div class="titles">
          <div class="head"><b>${esc(from)}</b><span class="age">${age(m.created_at)}</span></div>
          <div class="meta">${esc(meta)}</div>
        </div>
      </div>
      ${m.subject ? `<div class="subject">${esc(m.subject)}</div>` : ""}
      <div class="prose ${open ? "" : "clamp"}" data-clamp="${m.id}" ${m.subject ? 'style="margin-top:4px"' : ""}>${esc(m.body)}</div>
      <div class="actions">
        <button class="btn wide primary" data-act="reply" data-id="${m.id}">${ic("reply")}Reply</button>
        <button class="btn wide" data-act="done" data-id="${m.id}" ${busy}>${busyLabel(m.id, "done", "Done", "check")}</button>
      </div>
    </article>`);
}

function swipeable(id, snooze, card) {
  const acts =
    (snooze ? `<button class="s-snooze" data-act="snooze" data-for="1h" data-id="${id}">${ic("clock")}1 hour</button>` : "") +
    `<button class="s-done" data-act="done" data-id="${id}">${ic("check")}Done</button>`;
  return `<div class="swipe" id="w-${id}" data-swipe="${id}"><div class="swipe-acts">${acts}</div>${card}</div>`;
}

// Clamped text gets a "More" when it's cut off.
function markClamped(root) {
  for (const el of root.querySelectorAll("[data-clamp].clamp")) {
    if (el.scrollHeight > el.clientHeight + 2) {
      el.insertAdjacentHTML("afterend", `<button class="more" data-act="expand" data-id="${el.dataset.clamp}">More</button>`);
    }
  }
}

function agentsView() {
  if (!state.loaded) return state.conn === "off" ? offline() : "";
  const start = `<div class="actions new-agent"><button class="btn wide primary" data-act="new-agent">${ic("plus")}New Agent</button></div>`;
  if (!state.panes.length) return start + empty("agents", "t-gray", "No agents running", "Claude Code and Codex panes in Rex show up here.");
  const groups = {};
  const local = {};
  for (const p of state.panes) {
    (groups[paneGroup(p)] ||= []).push(p);
    local[paneGroup(p)] = isLocal(p);
  }
  // This Mac's sessions first, then other hosts'.
  const order = Object.keys(groups).sort((a, b) => (local[a] === local[b] ? a.localeCompare(b) : local[a] ? -1 : 1));
  return start + order.map((s) => section(s, groups[s].length) + `<div class="group">${groups[s].map(agentRow).join("")}</div>`).join("");
}

function agentRow(p) {
  const activity = p.title || p.msg;
  const kids = p.working_children ? ` · +${plural(p.working_children, "subagent")}` : "";
  const label = p.kind && p.state === "blocked" ? p.kind : p.state || "unknown";
  return `<button class="arow" data-act="pane" data-key="${esc(paneKey(p))}">
    <div class="badge-icon ${tint(p.state)}">${ic("agents")}</div>
    <div class="main">
      <div class="name">${esc(paneName(p))}<small>${esc(appName(p.app))}${kids}</small></div>
      <div class="sub">${esc(activity || tilde(p.cwd))}</div>
    </div>
    <div class="side"><span class="state ${tint(p.state)}">${esc(label)}</span><span class="age">${age(p.since)}</span></div>
    <span class="chev">${ic("chevron")}</span>
  </button>`;
}

// ---- notice: notifications, or a problem ----

let noticeHtml = "";
function notice(html) {
  noticeHtml = html;
  paintNotice();
}
function paintNotice() {
  const el = $("notice");
  if (el.innerHTML !== noticeHtml) el.innerHTML = noticeHtml;
}

// ---- sheets ----

let scrollY0 = 0;
function lock(on) {
  const b = document.body;
  if (on && !b.classList.contains("locked")) {
    // Keep the list where it was under the sheet; iOS ignores overflow: hidden.
    scrollY0 = window.scrollY;
    b.style.position = "fixed";
    b.style.top = `-${scrollY0}px`;
    b.style.left = b.style.right = "0";
    b.classList.add("locked");
  } else if (!on && b.classList.contains("locked")) {
    b.classList.remove("locked");
    b.style.position = b.style.top = b.style.left = b.style.right = "";
    window.scrollTo(0, scrollY0);
  }
}

function openSheet(spec) {
  state.sheet = spec;
  sheet.style.removeProperty("--drag");
  sheet.innerHTML = sheetHtml(spec);
  markClamped(sheet);
  lock(true);
  $("scrim").classList.add("show");
  requestAnimationFrame(() => sheet.classList.add("show"));
  sheetLoad(spec);
  if (spec.focus) setTimeout(() => sheet.querySelector("textarea")?.focus(), 380);
}

function closeSheet() {
  if (!state.sheet) return;
  document.activeElement?.blur?.();
  state.sheet = null;
  sheet.classList.remove("show");
  $("scrim").classList.remove("show");
  lock(false);
  setTimeout(() => {
    if (!state.sheet) sheet.innerHTML = "";
  }, 400);
  if (state.pending) {
    state.pending = false;
    soon();
  }
}

const findAlert = (id) => state.alerts.find((a) => a.id === id);
const findMessage = (id) => state.messages.find((m) => m.id === id);
const findPane = (key) => state.panes.find((p) => paneKey(p) === key);

function sheetHead(icon, tone, title, sub) {
  return `<div class="grab"></div>
    <div class="sheet-head">
      ${icon ? `<div class="badge-icon ${tone}">${ic(icon)}</div>` : ""}
      <div class="titles"><b id="sheet-title">${esc(title)}</b>${sub ? `<span>${esc(sub)}</span>` : ""}</div>
      <button class="btn circle small" data-close aria-label="Close">${ic("x")}</button>
    </div>`;
}

function sheetHtml(s) {
  if (s.type === "item") {
    const a = findAlert(s.id) || s.alert;
    const [icon, tone] = alertLook(a);
    let body = alertBody(a, false);
    if (canPreview(a)) {
      body += `<div class="label">The review, as it will post</div><div class="md" id="preview"><span class="hint">Loading…</span></div>`;
    } else {
      body += termBlock();
    }
    return `${sheetHead(icon, tone, headline(a), [place(a), age(a.created_at, true)].join(" · "))}
      <div class="sheet-body">${body}</div>
      <div class="sheet-foot" id="sheet-foot">${itemFoot(s)}</div>`;
  }
  if (s.type === "message") {
    const m = findMessage(s.id) || s.message;
    const from = messageFrom(m);
    return `${sheetHead("mail", "t-blue", from, [`#${m.id}`, age(m.created_at, true)].join(" · "))}
      <div class="sheet-body">
        ${m.subject ? `<div class="subject" style="margin-top:0">${esc(m.subject)}</div>` : ""}
        <div class="prose" ${m.subject ? 'style="margin-top:4px"' : ""}>${esc(m.body)}</div>
      </div>
      <div class="sheet-foot" id="sheet-foot">${composer(`m${m.id}`, `Reply to ${from}…`, "reply-send", m.id)}</div>`;
  }
  if (s.type === "pane") {
    const p = findPane(s.key) || s.pane;
    const activity = p.title || p.msg;
    return `${sheetHead("agents", tint(p.state), paneName(p), paneSub(p))}
      <div class="sheet-body">
        ${activity ? `<div class="prose" style="margin-top:0">${esc(activity)}</div>` : ""}
        <div class="meta" style="margin-top:6px">${esc(paneSession(p))} · ${esc(tilde(p.cwd))}</div>
        ${termBlock()}
      </div>
      <div class="sheet-foot" id="sheet-foot">${composer(`p${s.key}`, `Message ${paneName(p)}…`, "pane-send", s.key)}<div class="hint">It goes in when the agent is idle.</div></div>`;
  }
  if (s.type === "new") {
    return `${sheetHead("plus", "t-blue", "New Agent", "Claude Code or Codex, on any Rex host")}
      <div class="sheet-body" id="new-body">${newBody()}</div>
      <div class="sheet-foot" id="sheet-foot">${newFoot()}</div>`;
  }
  if (s.type === "menu") {
    const a = findAlert(s.id) || s.alert;
    const rows = [];
    rows.push(canPreview(a) ? ["doc", "Preview the review", "open"] : ["agents", "Show the pane", "open"]);
    const group2 = [];
    if (a.category === "needs_you") {
      group2.push(["clock", "Snooze 1 hour", "snooze", "1h"], ["clock", "Snooze 4 hours", "snooze", "4h"], ["clock", "Snooze until tomorrow", "snooze", "1d"]);
    }
    const group3 = [["check", "Mark done", "done"]];
    const menu = (items) =>
      `<div class="menu">${items.map(([icon, label, act, f]) => `<button data-act="${act}" data-id="${a.id}" ${f ? `data-for="${f}"` : ""}>${ic(icon)}${esc(label)}</button>`).join("")}</div>`;
    return `${sheetHead(...alertLook(a), headline(a), place(a))}
      <div class="sheet-body">${menu(rows)}${group2.length ? menu(group2) : ""}${menu(group3)}</div>`;
  }
  return "";
}

// ---- the new-agent sheet ----

const hostTitle = (h) => (h === "local" ? "This Mac" : h);
const chosenHost = () => state.hosts.find((h) => h.host === state.draft.host);
// A path with the host's home as ~.
function tildeOn(path, h) {
  const home = h && h.home;
  return home && path && path.startsWith(home) ? "~" + path.slice(home.length) : path || "";
}

function newBody() {
  const d = state.draft;
  const h = chosenHost();
  const s = state.sheet || {};
  const chip = (on, act, value, label, off = false) =>
    `<button class="chip ${on ? "on" : ""}" data-act="${act}" data-value="${esc(value)}" ${off ? "disabled" : ""}>${esc(label)}</button>`;
  let html = `<div class="label" style="margin-top:0">Machine</div><div class="chips">`;
  html += state.hosts.length
    ? state.hosts.map((x) => chip(x.host === d.host, "na-host", x.host, x.connected ? hostTitle(x.host) : `${hostTitle(x.host)} · unreachable`)).join("")
    : `<span class="hint">Loading…</span>`;
  html += `</div>`;
  if (h && !h.connected) html += `<div class="field-note bad">${esc(h.error || "Rex there can't be reached.")}</div>`;
  html += `<div class="label">Agent</div><div class="chips">${chip(d.program === "claude", "na-prog", "claude", "Claude Code")}${chip(d.program === "codex", "na-prog", "codex", "Codex")}</div>`;
  html += `<div class="label">Folder<span class="grow"></span><button data-act="na-browse">${ic("folder")}${s.browsing ? "Hide" : "Browse"}</button></div>
    <input class="field" data-field="cwd" value="${esc(d.cwd)}" placeholder="~" autocapitalize="off" autocorrect="off" spellcheck="false">`;
  if (s.browsing) {
    const l = s.listing;
    if (s.browseError) html += `<div class="field-note bad">${esc(s.browseError)}</div>`;
    else if (!l) html += `<div class="field-note">Loading…</div>`;
    else {
      html += `<div class="menu browse"><button data-act="na-dir" data-value="${esc(parentDir(l.path))}" ${l.path === "/" ? "disabled" : ""}>${ic("up")}<span class="path">${esc(tildeOn(l.path, h))}</span></button>`;
      html += l.dirs.map((x) => `<button data-act="na-dir" data-value="${esc(joinDir(l.path, x))}">${ic("folder")}${esc(x)}</button>`).join("");
      html += l.dirs.length ? "" : `<div class="field-note">No folders in here.</div>`;
      html += `</div>`;
    }
  } else if (h && h.folders && h.folders.length) {
    html += `<div class="chips scroll">${h.folders.map((f) => chip(f === d.cwd, "na-folder", f, tildeOn(f, h))).join("")}</div>`;
  }
  html += `<div class="label">Prompt</div>
    <textarea class="field" rows="4" data-field="prompt" placeholder="What should it do? Leave empty and it waits for you.">${esc(d.prompt)}</textarea>`;
  html += `<div class="label">Name and session</div>
    <div class="pair"><input class="field" data-field="name" value="${esc(d.name)}" placeholder="${esc(d.program)}" autocapitalize="off" autocorrect="off" spellcheck="false">
    <input class="field" data-field="session" value="${esc(d.session)}" placeholder="inbox" autocapitalize="off" autocorrect="off" spellcheck="false"></div>
    <div class="field-note">A new tab in this session, made if it doesn't exist. Other agents message it by its name.</div>`;
  return html;
}

function newFoot() {
  const s = state.sheet || {};
  const h = chosenHost();
  const ok = h && h.connected && !s.busy;
  return `${s.error ? `<div class="field-note bad">${esc(s.error)}</div>` : ""}<div class="actions"><button class="btn wide primary" data-act="na-start" ${ok ? "" : "disabled"}>${s.busy ? '<span class="spin"></span>' : ic("send")}Start</button></div>`;
}

function paintNew() {
  if (!state.sheet || state.sheet.type !== "new") return;
  const body = $("new-body");
  const foot = $("sheet-foot");
  if (body) {
    const y = body.scrollTop;
    body.innerHTML = newBody();
    body.scrollTop = y;
  }
  if (foot) foot.innerHTML = newFoot();
}

const parentDir = (p) => p.replace(/\/[^/]+\/?$/, "") || "/";
const joinDir = (dir, name) => (dir.endsWith("/") ? dir : dir + "/") + name;

async function browse(path) {
  const s = state.sheet;
  try {
    const l = await call("dirs.list", { host: state.draft.host, path });
    if (state.sheet !== s) return;
    if (l.status !== "ok") {
      s.browseError = `${tildeOn(l.path, chosenHost())} isn't a folder there.`;
    } else {
      s.listing = l;
      s.browseError = null;
      state.draft.cwd = tildeOn(l.path, chosenHost());
    }
  } catch (e) {
    s.browseError = e.message;
  }
  paintNew();
}

async function startAgent() {
  const s = state.sheet;
  const d = state.draft;
  s.busy = true;
  s.error = null;
  paintNew();
  try {
    const r = await call("agents.start", {
      host: d.host,
      program: d.program,
      cwd: d.cwd.trim(),
      prompt: d.prompt,
      name: d.name.trim() || null,
      session: d.session.trim() || null,
    });
    d.prompt = "";
    d.name = "";
    haptic();
    toast(`Started ${appName(d.program === "codex" ? "codex" : "claude-code")} as ${r.address}`);
    closeSheet();
    state.tab = "agents";
    refresh();
  } catch (e) {
    s.busy = false;
    s.error = e.message;
    paintNew();
  }
}

function paneSub(p) {
  const label = p.kind && p.state === "blocked" ? `${p.state} · ${p.kind}` : p.state || "unknown";
  return [appName(p.app), label, age(p.since)].filter(Boolean).join(" · ");
}

function termBlock() {
  return `<div class="label">Pane<span class="grow"></span><button data-act="wrap">${ic("wrap")}Wrap</button><button data-act="screen-refresh">${ic("refresh")}Refresh</button></div>
    <pre class="term loading" id="term">Loading…</pre>`;
}

function composer(draft, placeholder, act, id) {
  const has = (state.drafts[draft] || "").trim();
  return `<div class="composer">
    <textarea rows="1" data-draft="${esc(draft)}" placeholder="${esc(placeholder)}" enterkeyhint="enter">${esc(state.drafts[draft] || "")}</textarea>
    <button class="btn circle" data-act="${act}" data-id="${esc(id)}" aria-label="Send" ${has ? "" : "disabled"}>${state.busy.has(id) ? '<span class="spin"></span>' : ic("send")}</button>
  </div>`;
}

function itemFoot(s) {
  const a = findAlert(s.id);
  if (!a) return `<div class="gone">${ic("done")}This no longer waits for you.</div>`;
  const busy = state.busy.has(a.id) ? "disabled" : "";
  let html = "";
  if (decisions(a).length) html += `<div class="actions">${decisionButtons(a)}</div>`;
  const snooze =
    a.category === "needs_you"
      ? [["1h", "1h"], ["4h", "4h"], ["1d", "Tomorrow"]]
          .map(([f, l]) => `<button class="btn small quiet" data-act="snooze" data-for="${f}" data-id="${a.id}" ${busy}>${l}</button>`)
          .join("")
      : "";
  html += `<div class="actions">${snooze ? `<span class="lead" aria-label="Snooze">${ic("clock")}</span>${snooze}` : ""}<span class="grow"></span><button class="btn small" data-act="done" data-id="${a.id}" ${busy}>${busyLabel(a.id, "done", "Done", "check")}</button></div>`;
  return html;
}

// After a refresh: the sheet's footer follows the item; its body stays put.
function refreshSheet() {
  const s = state.sheet;
  if (sheet.contains(document.activeElement) && document.activeElement.tagName === "TEXTAREA") return;
  if (s.type === "item") {
    const foot = $("sheet-foot");
    if (foot) foot.innerHTML = itemFoot(s);
  } else if (s.type === "pane") {
    const p = findPane(s.key);
    const sub = sheet.querySelector(".sheet-head .titles span");
    if (p && sub) sub.textContent = paneSub(p);
  } else if (s.type === "menu" && !findAlert(s.id)) {
    closeSheet();
  } else if (s.type === "message" && !findMessage(s.id) && !state.busy.has(s.id)) {
    const foot = $("sheet-foot");
    if (foot) foot.innerHTML = `<div class="gone">${ic("done")}Answered or filed.</div>`;
  }
}

async function sheetLoad(s, quiet = false) {
  if (s.type === "new") {
    try {
      state.hosts = await call("hosts.list");
      if (!state.hosts.some((h) => h.host === state.draft.host)) state.draft.host = "local";
    } catch (e) {
      s.error = e.message;
    }
    return paintNew();
  }
  try {
    if (s.type === "item") {
      const a = findAlert(s.id) || s.alert;
      if (canPreview(a)) {
        const { text } = await call("preview", { id: s.id });
        if (state.sheet === s) $("preview").innerHTML = md(text);
        return;
      }
      const { text } = await call("screen", { id: s.id, lines: 80 });
      showTerm(s, text, quiet);
    } else if (s.type === "pane") {
      const p = findPane(s.key) || s.pane;
      const { text } = await call("panes.screen", { host: p.host, block_id: p.block_id, lines: 80 });
      showTerm(s, text, quiet);
    }
  } catch (e) {
    if (state.sheet !== s) return;
    const el = $("term") || $("preview");
    if (el) {
      el.classList.add("loading");
      el.textContent = e.message;
    }
  }
}

function showTerm(s, text, quiet) {
  if (state.sheet !== s) return;
  const el = $("term");
  if (!el) return;
  const atEnd = el.scrollTop + el.clientHeight >= el.scrollHeight - 8;
  el.classList.remove("loading");
  el.textContent = String(text || "").replace(/\s+$/, "") || "(empty)";
  // The prompt is at the bottom.
  if (!quiet || atEnd) el.scrollTop = el.scrollHeight;
}

// Drag the sheet down by its handle or title to close it.
(() => {
  let y0 = null;
  let dy = 0;
  let t0 = 0;
  sheet.addEventListener("touchstart", (e) => {
    if (!e.target.closest(".grab, .sheet-head") || e.target.closest("button")) return;
    y0 = e.touches[0].clientY;
    dy = 0;
    t0 = Date.now();
    sheet.classList.add("dragging");
  }, { passive: true });
  sheet.addEventListener("touchmove", (e) => {
    if (y0 === null) return;
    dy = Math.max(0, e.touches[0].clientY - y0);
    sheet.style.setProperty("--drag", `${dy}px`);
  }, { passive: true });
  sheet.addEventListener("touchend", () => {
    if (y0 === null) return;
    y0 = null;
    sheet.classList.remove("dragging");
    const fast = dy / Math.max(1, Date.now() - t0) > 0.6;
    if (dy > 110 || (fast && dy > 30)) closeSheet();
    else sheet.style.setProperty("--drag", "0px");
  });
})();

// The keyboard covers the bottom of the screen; keep the sheet above it.
if (window.visualViewport) {
  const vv = window.visualViewport;
  const fit = () => {
    const kb = Math.max(0, window.innerHeight - vv.height - vv.offsetTop);
    document.documentElement.style.setProperty("--kb", `${Math.round(kb)}px`);
    document.body.classList.toggle("kb", kb > 80);
  };
  vv.addEventListener("resize", fit);
  vv.addEventListener("scroll", fit);
}

// ---- swipe a card left: Done (and Snooze, for what needs you) ----

let swipeClose = () => {};

(() => {
  let s = null; // {wrap, card, x0, y0, dx, axis, width, open}
  let opened = null; // a swipe left open
  const settle = (wrap, card) => {
    card.style.transform = "";
    setTimeout(() => {
      if (!card.style.transform) wrap.classList.remove("swiping");
    }, 320);
  };
  const closeOpen = () => {
    if (!opened) return;
    settle(opened.wrap, opened.card);
    opened = null;
    release();
  };
  main.addEventListener("touchstart", (e) => {
    const wrap = e.target.closest("[data-swipe]");
    if (opened && (!wrap || wrap !== opened.wrap)) closeOpen();
    if (!wrap || e.target.closest("button, textarea, .swipe-acts")) return;
    const card = wrap.querySelector(".card");
    const t = e.touches[0];
    const base = opened && opened.wrap === wrap ? -opened.width : 0;
    s = { wrap, card, x0: t.clientX - base, y0: t.clientY, dx: base, axis: null, width: wrap.querySelector(".swipe-acts").children.length * 84 };
  }, { passive: true });
  main.addEventListener("touchmove", (e) => {
    if (!s) return;
    const t = e.touches[0];
    const dx = t.clientX - s.x0;
    const dy = t.clientY - s.y0;
    if (!s.axis) {
      if (Math.abs(dx) < 8 && Math.abs(dy) < 8) return;
      s.axis = Math.abs(dx) > Math.abs(dy) * 1.2 ? "x" : "y";
      if (s.axis === "x") {
        state.hold++;
        s.wrap.classList.add("swiping");
        s.card.classList.add("dragging");
        if (opened && opened.wrap === s.wrap) {
          opened = null;
          release();
        }
      }
    }
    if (s.axis !== "x") return;
    e.preventDefault();
    // Rubber band past the full width; none to the right.
    let x = Math.min(0, dx);
    const full = s.wrap.clientWidth;
    if (-x > full * 0.55) x = -(full * 0.55 + (-x - full * 0.55) * 0.4);
    s.dx = x;
    s.card.style.transform = `translateX(${x}px)`;
    s.wrap.classList.toggle("armed", -x > full * 0.5);
  }, { passive: false });
  const end = () => {
    if (!s) return;
    const cur = s;
    s = null;
    if (cur.axis !== "x") return;
    cur.card.classList.remove("dragging");
    const full = cur.wrap.clientWidth;
    if (-cur.dx > full * 0.5) {
      // A full swipe: done.
      cur.card.style.transform = `translateX(-${full}px)`;
      release();
      haptic();
      return file(Number(cur.wrap.dataset.swipe), "done");
    }
    if (-cur.dx > 60) {
      cur.card.style.transform = `translateX(-${cur.width}px)`;
      opened = { wrap: cur.wrap, card: cur.card, width: cur.width };
      return; // the hold stays until it closes
    }
    settle(cur.wrap, cur.card);
    release();
  };
  main.addEventListener("touchend", end);
  main.addEventListener("touchcancel", end);
  window.addEventListener("scroll", closeOpen, { passive: true });
  swipeClose = closeOpen;
})();

// ---- actions ----

function haptic() {
  // iOS 18 gives a tap of haptics for a switch toggled through its label.
  try {
    let l = $("haptic");
    if (!l) {
      document.body.insertAdjacentHTML("beforeend", '<label id="haptic" style="position:fixed;opacity:0;pointer-events:none;width:1px;height:1px;overflow:hidden"><input type="checkbox" switch></label>');
      l = $("haptic");
    }
    l.click();
    navigator.vibrate?.(8);
  } catch {}
}

function toast(text, bad = false) {
  const t = $("toast");
  t.innerHTML = `${ic(bad ? "failed" : "done")}<span>${esc(text)}</span>`;
  t.classList.toggle("bad", bad);
  t.classList.add("show");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => t.classList.remove("show"), bad ? 6000 : 3000);
}

// Collapse a card away, then let the refresh drop it.
async function leave(id) {
  const w = $(`w-${id}`);
  if (!w) return;
  state.hold++;
  w.style.height = `${w.offsetHeight}px`;
  w.getBoundingClientRect();
  w.classList.add("leaving");
  w.style.height = "0px";
  await sleep(280);
  release();
}

async function act(id, action, work) {
  state.busy.set(id, action);
  render();
  let ok = false;
  try {
    await work();
    ok = true;
    haptic();
  } catch (e) {
    toast(e.message, true);
  }
  state.busy.delete(id);
  if (ok) await leave(id);
  await refresh();
  return ok;
}

function answer(id, action) {
  const a = findAlert(id);
  const label = (answers(a || {}).find((x) => x.action === action) || {}).label || action;
  return act(id, action, async () => {
    const r = await call(action, { id, seen: a && a.msg });
    toast(r.summary || `${label} · the pane is now ${r.now}`);
  });
}

function file(id, how, f) {
  swipeClose();
  if (state.sheet && state.sheet.type === "menu") closeSheet();
  return act(id, how, async () => {
    if (how === "snooze") {
      await call("alerts.snooze", { id, for: f });
      toast(`Snoozed until ${f === "1d" ? "tomorrow" : f === "4h" ? "4 hours from now" : "an hour from now"}`);
    } else {
      await call("done", { id });
    }
  });
}

async function sendDraft(key, draftKey, work) {
  const text = (state.drafts[draftKey] || "").trim();
  if (!text) return;
  state.busy.set(key, "send");
  refreshComposer(draftKey);
  try {
    await work(text);
    delete state.drafts[draftKey];
    haptic();
    toast("Sent. It goes in when the agent is idle.");
    closeSheet();
  } catch (e) {
    toast(e.message, true);
  }
  state.busy.delete(key);
  refreshComposer(draftKey);
  refresh();
}

function refreshComposer(draftKey) {
  const t = sheet.querySelector(`textarea[data-draft="${CSS.escape(draftKey)}"]`);
  if (!t) return;
  const b = t.parentElement.querySelector("button");
  const key = b.dataset.id;
  const busy = state.busy.has(isNaN(key) ? key : Number(key));
  b.disabled = busy || !(state.drafts[draftKey] || "").trim();
  b.innerHTML = busy ? '<span class="spin"></span>' : ic("send");
}

function autosize(t) {
  t.style.height = "auto";
  t.style.height = `${Math.min(t.scrollHeight, window.innerHeight * 0.4)}px`;
}

document.addEventListener("input", (e) => {
  const t = e.target;
  if (t.dataset && t.dataset.field) {
    state.draft[t.dataset.field] = t.value;
    return;
  }
  if (!t.dataset || !t.dataset.draft) return;
  state.drafts[t.dataset.draft] = t.value;
  autosize(t);
  refreshComposer(t.dataset.draft);
});

document.addEventListener("click", async (e) => {
  if (e.target.closest("[data-close]") || e.target.id === "scrim") return closeSheet();
  const chip = e.target.closest("[data-filter]");
  if (chip) {
    state.filter = chip.dataset.filter;
    render();
    return chip.scrollIntoView({ inline: "nearest", block: "nearest", behavior: "smooth" });
  }
  const tab = e.target.closest("#tabbar [data-tab]");
  if (tab) {
    if (state.tab === tab.dataset.tab) return window.scrollTo({ top: 0, behavior: "smooth" });
    state.tab = tab.dataset.tab;
    render();
    return window.scrollTo(0, 0);
  }
  const b = e.target.closest("[data-act]");
  if (!b) {
    // A tap on a card opens it.
    const card = e.target.closest("[data-open]");
    if (card && !window.getSelection()?.toString()) openItem(Number(card.dataset.open));
    return;
  }
  const id = Number(b.dataset.id);
  switch (b.dataset.act) {
    case "answer":
      if (state.sheet) closeSheet();
      return answer(id, b.dataset.action);
    case "done":
      if (state.sheet && state.sheet.type !== "menu") closeSheet();
      return file(id, "done");
    case "snooze":
      if (state.sheet && state.sheet.type !== "menu") closeSheet();
      return file(id, "snooze", b.dataset.for || "1h");
    case "menu":
      return openSheet({ type: "menu", id, alert: findAlert(id) });
    case "open":
      return openItem(id);
    case "reply":
      return openSheet({ type: "message", id, message: findMessage(id), focus: true });
    case "expand":
      state.expanded.add(id);
      b.previousElementSibling?.classList.remove("clamp");
      return b.remove();
    case "toggle-finished":
      state.showFinished = !state.showFinished;
      return render();
    case "pane":
      return openSheet({ type: "pane", key: b.dataset.key, pane: findPane(b.dataset.key) });
    case "wrap":
      return $("term")?.classList.toggle("wrap");
    case "screen-refresh":
      return state.sheet && sheetLoad(state.sheet, true);
    case "reply-send":
      return sendDraft(id, `m${id}`, (text) => call("messages.reply", { id, body: text }));
    case "pane-send": {
      const key = b.dataset.id;
      return sendDraft(key, `p${key}`, (text) => call("messages.send", { to: key.split("|")[1], body: text }));
    }
    case "enable":
      return enablePush();
    case "new-agent":
      return openSheet({ type: "new" });
    case "na-host":
      if (state.draft.host !== b.dataset.value) {
        Object.assign(state.draft, { host: b.dataset.value, cwd: "", session: "" });
        Object.assign(state.sheet, { listing: null, browsing: false, error: null });
      }
      return paintNew();
    case "na-prog":
      state.draft.program = b.dataset.value;
      return paintNew();
    case "na-folder":
      state.draft.cwd = b.dataset.value;
      return paintNew();
    case "na-browse":
      state.sheet.browsing = !state.sheet.browsing;
      paintNew();
      return state.sheet.browsing && browse(state.draft.cwd.trim() || "~");
    case "na-dir":
      return browse(b.dataset.value);
    case "na-start":
      return startAgent();
  }
});

// Open an item's sheet: from a card, or from a notification (#item-N).
function openItem(id) {
  const a = findAlert(id);
  const m = findMessage(id);
  if (a) openSheet({ type: "item", id, alert: a });
  else if (m) openSheet({ type: "message", id, message: m });
}

function openFromHash(hash) {
  const m = /item-(\d+)/.exec(hash || "");
  if (!m) return;
  const id = Number(m[1]);
  history.replaceState(null, "", location.pathname);
  if (state.tab !== "inbox" || state.filter !== "all") {
    state.tab = "inbox";
    state.filter = "all";
  }
  if (findAlert(id)?.category === "finished") state.showFinished = true;
  render();
  const el = $(`item-${id}`);
  if (el) {
    el.scrollIntoView({ block: "center" });
    el.classList.add("flash");
    openItem(id);
  } else {
    toast(`#${id} no longer waits for you`);
  }
}
window.addEventListener("hashchange", () => openFromHash(location.hash));

// ---- the large title gives way to a bar ----

let ticking = false;
window.addEventListener("scroll", () => {
  if (ticking) return;
  ticking = true;
  requestAnimationFrame(() => {
    ticking = false;
    const h = $("title");
    document.body.classList.toggle("scrolled", h.getBoundingClientRect().bottom < $("topbar").offsetHeight - 4);
  });
}, { passive: true });

// ---- notifications ----

const standalone = () => window.matchMedia("(display-mode: standalone)").matches || navigator.standalone === true;

function b64url(s) {
  const pad = "=".repeat((4 - (s.length % 4)) % 4);
  const raw = atob((s + pad).replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(raw, (c) => c.charCodeAt(0));
}

const note = (icon, tone, title, text, button = "") =>
  `<div class="note"><div class="badge-icon ${tone}">${ic(icon)}</div><p><b>${esc(title)}</b><span>${esc(text)}</span></p>${button}</div>`;

let registration = null;
async function setupPush() {
  if (!("serviceWorker" in navigator)) return;
  registration = await navigator.serviceWorker.register("/sw.js");
  navigator.serviceWorker.addEventListener("message", (e) => {
    if (e.data && e.data.open) openFromHash(e.data.open);
  });
  if (!("PushManager" in window)) {
    if (!standalone()) notice(note("bell", "t-blue", "Get notifications", "Share › Add to Home Screen, then open Rex Inbox from there."));
    return;
  }
  const sub = await registration.pushManager.getSubscription();
  if (sub && Notification.permission === "granted") {
    // Tell the hub again, in case it lost track.
    call("push.subscribe", { subscription: sub.toJSON(), agent: navigator.userAgent }).catch(() => {});
    return notice("");
  }
  if (Notification.permission === "denied") {
    return notice(note("bell", "t-gray", "Notifications are off", "Turn them on in Settings › Notifications › Inbox."));
  }
  notice(note("bell", "t-blue", "Notifications", "Know when an agent needs you.", '<button class="btn primary small" data-act="enable">Turn On</button>'));
}

async function enablePush() {
  try {
    // Must run from the tap itself.
    const perm = await Notification.requestPermission();
    if (perm !== "granted") return setupPush();
    const { public_key } = await call("push.key");
    const s = await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: b64url(public_key) });
    await call("push.subscribe", { subscription: s.toJSON(), agent: navigator.userAgent });
    notice("");
    const r = await call("push.test");
    toast(r.sent ? "Notifications are on" : "Subscribed, but the test didn't send", !r.sent);
  } catch (e) {
    toast(e.message, true);
  }
}

// ---- start ----

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState !== "visible") return;
  live(); // iOS drops the stream in the background
  soon(0);
});
setInterval(() => {
  if (document.visibilityState === "visible" && !state.hold && !main.contains(document.activeElement)) render();
}, 30000);

render();
refresh().then(() => openFromHash(location.hash));
live();
setupPush().catch((e) => console.warn(e));
