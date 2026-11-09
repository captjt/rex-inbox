// Screenshot the web app the way a phone shows it, for scripts/screenshots.sh.
// It starts headless Chrome, sizes the page like an iPhone (390x844 at 2x),
// picks a light or dark appearance, and saves a PNG. Needs Node 22 or later
// (for its WebSocket) and Google Chrome. With SCRIPT, it runs that in the
// page once loaded (to open a sheet, say) before the shot.
//
//   node scripts/phone-shot.mjs URL OUT.png light|dark [SCRIPT]
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const [url, out, scheme = "dark", script] = process.argv.slice(2);
if (!url || !out) {
  console.error("usage: node scripts/phone-shot.mjs URL OUT.png light|dark [SCRIPT]");
  process.exit(2);
}
const chrome = process.env.CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const port = 9400 + Math.floor(Math.random() * 400);
const profile = mkdtempSync(join(tmpdir(), "rex-inbox-shot-"));
const proc = spawn(chrome, ["--headless=new", "--disable-gpu", "--hide-scrollbars", `--user-data-dir=${profile}`, `--remote-debugging-port=${port}`, "about:blank"], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

try {
  let targets;
  for (let i = 0; i < 50 && !targets; i++) {
    await sleep(200);
    targets = await fetch(`http://127.0.0.1:${port}/json`).then((r) => r.json()).catch(() => undefined);
  }
  const page = targets?.find((t) => t.type === "page");
  if (!page) throw new Error("Chrome didn't start");
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r, j) => { ws.addEventListener("open", r); ws.addEventListener("error", j); });
  let n = 0;
  const waiting = new Map();
  ws.addEventListener("message", (e) => {
    const m = JSON.parse(e.data);
    if (m.id && waiting.has(m.id)) { waiting.get(m.id)(m); waiting.delete(m.id); }
  });
  const send = (method, params = {}) => new Promise((r) => { const id = ++n; waiting.set(id, r); ws.send(JSON.stringify({ id, method, params })); });

  await send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true });
  await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
  await send("Page.enable");
  await send("Page.navigate", { url });
  await sleep(2500);
  if (script) {
    await send("Runtime.evaluate", { expression: script });
    await sleep(1500);
  }
  const shot = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(out, Buffer.from(shot.result.data, "base64"));
  ws.close();
} finally {
  proc.kill();
  await sleep(300);
  rmSync(profile, { recursive: true, force: true });
}
