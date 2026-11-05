"use strict";

// Rex Inbox on the phone: the hub's methods over /api/call, live updates
// over /api/events, and Web Push for what needs you.

const state = {
  tab: "inbox",
  alerts: [],
  messages: [],
  panes: [],
  showFinished: false,
  replying: new Set(), // message ids with the reply box open
  composing: new Set(), // pane keys with the message box open
  drafts: {},
  busy: new Set(),
  pending: false, // a refresh waiting for a text box to lose focus
};

const main = document.getElementById("main");

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
    render();
  } catch (e) {
    document.getElementById("summary").textContent = "Not connected";
    notice(`<p>${esc(e.message)}</p>`);
  }
}

let timer = null;
function soon() {
  clearTimeout(timer);
  timer = setTimeout(() => {
    // Don't redraw under someone typing.
    if (document.activeElement && document.activeElement.tagName === "TEXTAREA") {
      state.pending = true;
      return;
    }
    refresh();
  }, 150);
}

function live() {
  const es = new EventSource("/api/events");
  es.onmessage = soon;
  es.onopen = soon;
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

function age(ts) {
  const d = when(ts);
  if (!d) return "";
  const s = Math.max(0, Math.floor((Date.now() - d) / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

const tilde = (p) => (p ? String(p).replace(/^\/Users\/[^/]+/, "~") : "");
const shortId = (id) => String(id || "").split(/[-:]/).pop().slice(-6);

function appName(a) {
  return { "claude-code": "Claude Code", codex: "Codex" }[a] || a || "A program";
}

function place(x) {
  const s = x.session_label || shortId(x.session_id);
  const p = x.pane_label || x.label || shortId(x.block_id);
  const base = `${s}/${p}`;
  return !x.host || x.host === "local" ? base : `${x.host}:${base}`;
}

function headline(a) {
  const app = appName(a.app);
  if (a.category === "failed") return `${app} failed`;
  if (a.category === "finished") return `${app} finished`;
  return { permission: `${app} needs your approval`, question: `${app} has a question`, auth: `${app} needs you to sign in` }[a.kind] || `${app} needs you`;
}

const what = (x) => [x.title, x.msg].filter(Boolean).join(": ");
const canAnswer = (a) => a.category === "needs_you" && a.kind === "permission" && a.app === "claude-code";
const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;

// ---- views ----

function render() {
  const needs = state.alerts.filter((a) => a.category === "needs_you");
  const failed = state.alerts.filter((a) => a.category === "failed");
  const finished = state.alerts.filter((a) => a.category === "finished");
  const parts = [];
  if (needs.length) parts.push(`${needs.length} need${needs.length === 1 ? "s" : ""} you`);
  if (state.messages.length) parts.push(plural(state.messages.length, "message"));
  if (failed.length) parts.push(`${failed.length} failed`);
  if (!parts.length) parts.push("All clear");
  parts.push(plural(state.panes.length, "agent"));
  document.getElementById("summary").textContent = parts.join(" · ");
  document.title = needs.length ? `(${needs.length}) Rex Inbox` : "Rex Inbox";

  main.innerHTML = state.tab === "inbox" ? inboxView(needs, failed, finished) : agentsView();
  for (const t of main.querySelectorAll("textarea[data-draft]")) t.value = state.drafts[t.dataset.draft] || "";
}

function section(title, count, inner) {
  return `<h2>${esc(title)} <span class="count">${count}</span></h2>${inner}`;
}

function inboxView(needs, failed, finished) {
  if (!state.alerts.length && !state.messages.length) {
    return `<div class="empty"><div class="big">✓</div><h3>Nothing needs you</h3>Blocked, failed, and finished agents show up here, with messages to you.</div>`;
  }
  let html = "";
  if (needs.length) html += section("Needs you", needs.length, needs.map(alertCard).join(""));
  if (state.messages.length) html += section("Messages", state.messages.length, state.messages.map(messageCard).join(""));
  if (failed.length) html += section("Failed", failed.length, failed.map(alertCard).join(""));
  if (finished.length) {
    html += `<h2><button data-act="toggle-finished">Finished <span class="count">${finished.length}</span> ${state.showFinished ? "▾" : "▸"}</button></h2>`;
    if (state.showFinished) html += finished.map(alertCard).join("");
  }
  return html;
}

function alertCard(a) {
  const busy = state.busy.has(a.id) ? "disabled" : "";
  const w = what(a);
  let body = "";
  if (w) body = a.category === "needs_you" ? `<div class="code">${esc(w)}</div>` : `<div class="what">${esc(w)}</div>`;
  const answer = canAnswer(a)
    ? `<button class="btn approve" data-act="approve" data-id="${a.id}" ${busy}>Approve</button>
       <button class="btn deny" data-act="deny" data-id="${a.id}" ${busy}>Deny</button>`
    : "";
  const snooze = a.category === "needs_you" ? `<button class="btn" data-act="snooze" data-id="${a.id}" ${busy}>Snooze 1h</button>` : "";
  return `<div class="card" id="item-${a.id}">
    <div class="row"><span class="icon c-${esc(a.category)}">●</span><span class="head">${esc(headline(a))}</span><span class="age">${age(a.created_at)}</span></div>
    <div class="meta">#${a.id} · ${esc(place(a))}${a.cwd ? " · " + esc(tilde(a.cwd)) : ""}</div>
    ${body}
    <div class="actions">${answer}<button class="btn" data-act="screen" data-id="${a.id}">Show pane</button><span class="spacer"></span>${snooze}<button class="btn" data-act="done" data-id="${a.id}" ${busy}>Done</button></div>
  </div>`;
}

function messageCard(m) {
  const from = m.from_block ? m.from_addr : "you";
  const busy = state.busy.has(m.id) ? "disabled" : "";
  const replying = state.replying.has(m.id);
  return `<div class="card" id="item-${m.id}">
    <div class="row"><span class="icon c-message">●</span><span class="head">${esc(from)}</span><span class="age">#${m.id} · ${age(m.created_at)}</span></div>
    ${m.subject ? `<div class="subject">${esc(m.subject)}</div>` : ""}
    <div class="body">${esc(m.body)}</div>
    ${replying ? `<textarea data-draft="m${m.id}" rows="2" placeholder="Reply to ${esc(from)}…"></textarea>` : ""}
    <div class="actions">
      <button class="btn primary" data-act="${replying ? "reply-send" : "reply-open"}" data-id="${m.id}" ${busy}>${replying ? "Send" : "Reply"}</button>
      <span class="spacer"></span>
      <button class="btn" data-act="done" data-id="${m.id}" ${busy}>Done</button>
    </div>
  </div>`;
}

function agentsView() {
  if (!state.panes.length) {
    return `<div class="empty"><h3>No agents running</h3>Claude Code and Codex panes in Rex show up here.</div>`;
  }
  const groups = {};
  for (const p of state.panes) (groups[p.session_label || shortId(p.session_id)] ||= []).push(p);
  return Object.keys(groups)
    .sort()
    .map((s) => section(s, groups[s].length, groups[s].map(paneCard).join("")))
    .join("");
}

function paneCard(p) {
  const key = `${p.host}|${p.block_id}`;
  const composing = state.composing.has(key);
  const state_ = p.state || "";
  const label = p.kind ? `${state_} · ${p.kind}` : state_ || "unknown";
  const activity = p.title || p.msg;
  const kids = p.working_children ? ` · +${plural(p.working_children, "subagent")}` : "";
  return `<div class="card">
    <div class="row"><span class="head">${esc(p.label || shortId(p.block_id))} <span class="age">${esc(appName(p.app))}${kids}</span></span><span class="pill s-${esc(state_)}">${esc(label)}</span><span class="age">${age(p.since)}</span></div>
    <div class="meta">${esc(tilde(p.cwd))}</div>
    ${activity ? `<div class="what">${esc(activity)}</div>` : ""}
    ${composing ? `<textarea data-draft="p${esc(key)}" rows="2" placeholder="Message ${esc(place(p))}…"></textarea>` : ""}
    <div class="actions">
      <button class="btn" data-act="pane-screen" data-host="${esc(p.host)}" data-block="${esc(p.block_id)}">Show pane</button>
      <button class="btn ${composing ? "primary" : ""}" data-act="${composing ? "compose-send" : "compose"}" data-key="${esc(key)}">${composing ? "Send" : "Message"}</button>
    </div>
  </div>`;
}

// ---- actions ----

function toast(text, bad = false) {
  const t = document.getElementById("toast");
  t.textContent = text;
  t.className = bad ? "bad" : "";
  t.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => (t.hidden = true), bad ? 6000 : 3500);
}

async function act(id, work) {
  state.busy.add(id);
  render();
  try {
    await work();
  } catch (e) {
    toast(e.message, true);
  } finally {
    state.busy.delete(id);
    await refresh();
  }
}

function showSheet(title, text) {
  document.getElementById("sheet-title").textContent = title;
  const pre = document.getElementById("sheet-body");
  pre.textContent = text;
  const d = document.getElementById("sheet");
  d.showModal();
  pre.scrollTop = pre.scrollHeight;
}

main.addEventListener("input", (e) => {
  if (e.target.dataset.draft) state.drafts[e.target.dataset.draft] = e.target.value;
});
main.addEventListener("focusout", () => {
  if (state.pending) {
    state.pending = false;
    soon();
  }
});

main.addEventListener("click", async (e) => {
  const b = e.target.closest("[data-act]");
  if (!b) return;
  const id = Number(b.dataset.id);
  const alert = state.alerts.find((a) => a.id === id);
  switch (b.dataset.act) {
    case "approve":
    case "deny":
      return act(id, async () => {
        const r = await call(b.dataset.act, { id, seen: alert && alert.msg });
        toast(`#${id} ${b.dataset.act === "approve" ? "approved" : "denied"}; the pane is now ${r.now}`);
      });
    case "snooze":
      return act(id, () => call("alerts.snooze", { id, for: "1h" }));
    case "done":
      return act(id, () => call("done", { id }));
    case "screen":
      try {
        showSheet(`#${id}`, (await call("screen", { id, lines: 60 })).text);
      } catch (err) {
        toast(err.message, true);
      }
      return;
    case "pane-screen":
      try {
        showSheet(b.dataset.block, (await call("panes.screen", { host: b.dataset.host, block_id: b.dataset.block, lines: 60 })).text);
      } catch (err) {
        toast(err.message, true);
      }
      return;
    case "toggle-finished":
      state.showFinished = !state.showFinished;
      return render();
    case "reply-open":
      state.replying.add(id);
      render();
      return main.querySelector(`textarea[data-draft="m${id}"]`)?.focus();
    case "reply-send": {
      const text = (state.drafts[`m${id}`] || "").trim();
      if (!text) return;
      return act(id, async () => {
        await call("messages.reply", { id, body: text });
        delete state.drafts[`m${id}`];
        state.replying.delete(id);
        toast("Sent; it goes in when the agent is idle");
      });
    }
    case "compose":
      state.composing.add(b.dataset.key);
      render();
      return main.querySelector(`textarea[data-draft="p${CSS.escape(b.dataset.key)}"]`)?.focus();
    case "compose-send": {
      const key = b.dataset.key;
      const text = (state.drafts[`p${key}`] || "").trim();
      if (!text) return;
      try {
        await call("messages.send", { to: key.split("|")[1], body: text });
        delete state.drafts[`p${key}`];
        state.composing.delete(key);
        toast("Sent; it goes in when the agent is idle");
        refresh();
      } catch (err) {
        toast(err.message, true);
      }
      return;
    }
  }
});

document.querySelector(".segmented").addEventListener("click", (e) => {
  const b = e.target.closest("button[data-tab]");
  if (!b) return;
  state.tab = b.dataset.tab;
  for (const x of document.querySelectorAll(".segmented button")) x.classList.toggle("on", x === b);
  render();
  window.scrollTo(0, 0);
});

document.getElementById("sheet").addEventListener("click", (e) => {
  if (e.target.matches("[data-close]") || e.target.id === "sheet") e.currentTarget.close();
});

// #item-N: from a notification, jump to that card.
function openItem(hash) {
  const m = /item-(\d+)/.exec(hash || "");
  if (!m) return;
  const id = Number(m[1]);
  state.tab = "inbox";
  for (const x of document.querySelectorAll(".segmented button")) x.classList.toggle("on", x.dataset.tab === "inbox");
  if (state.alerts.some((a) => a.id === id && a.category === "finished")) state.showFinished = true;
  render();
  const el = document.getElementById(`item-${id}`);
  if (el) {
    el.scrollIntoView({ behavior: "smooth", block: "start" });
    el.classList.add("flash");
  }
}
window.addEventListener("hashchange", () => openItem(location.hash));

// ---- notifications ----

function notice(html) {
  const n = document.getElementById("notice");
  n.innerHTML = html;
  n.hidden = !html;
}

const standalone = () => window.matchMedia("(display-mode: standalone)").matches || navigator.standalone === true;

function b64url(s) {
  const pad = "=".repeat((4 - (s.length % 4)) % 4);
  const raw = atob((s + pad).replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(raw, (c) => c.charCodeAt(0));
}

async function setupPush() {
  if (!("serviceWorker" in navigator)) return;
  const reg = await navigator.serviceWorker.register("/sw.js");
  navigator.serviceWorker.addEventListener("message", (e) => {
    if (e.data && e.data.open) {
      history.replaceState(null, "", e.data.open);
      openItem(e.data.open);
    }
  });
  if (!("PushManager" in window)) {
    if (!standalone()) notice(`<p>To get notifications, add this to your Home Screen: Share › Add to Home Screen, then open it from there.</p>`);
    return;
  }
  const sub = await reg.pushManager.getSubscription();
  if (sub && Notification.permission === "granted") {
    // Tell the hub again, in case it lost track.
    call("push.subscribe", { subscription: sub.toJSON(), agent: navigator.userAgent }).catch(() => {});
    return notice("");
  }
  if (Notification.permission === "denied") {
    return notice(`<p>Notifications are off for Rex Inbox. Turn them on in Settings › Notifications.</p>`);
  }
  notice(`<p>Get a notification when an agent needs you.</p><button class="btn primary" id="enable">Turn On</button>`);
  document.getElementById("enable").onclick = async () => {
    try {
      // Must run from the tap itself.
      const perm = await Notification.requestPermission();
      if (perm !== "granted") return setupPush();
      const { public_key } = await call("push.key");
      const s = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: b64url(public_key) });
      await call("push.subscribe", { subscription: s.toJSON(), agent: navigator.userAgent });
      notice("");
      const r = await call("push.test");
      toast(r.sent ? "Notifications are on" : "Subscribed, but the test didn't send");
    } catch (e) {
      toast(e.message, true);
    }
  };
}

// ---- start ----

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") soon();
});
setInterval(() => {
  if (document.visibilityState === "visible" && document.activeElement?.tagName !== "TEXTAREA") render();
}, 30000);

refresh().then(() => openItem(location.hash));
live();
setupPush().catch((e) => console.warn(e));
