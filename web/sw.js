// Rex Inbox service worker: shows pushes from the hub, opens the item on tap.
self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (e) => e.waitUntil(self.clients.claim()));

self.addEventListener("push", (e) => {
  let d = {};
  try { d = e.data ? e.data.json() : {}; } catch { d = { body: e.data && e.data.text() }; }
  e.waitUntil(self.registration.showNotification(d.title || "Rex Inbox", {
    body: d.body || "",
    tag: d.tag,
    data: { url: d.url || "/" },
    icon: "/icon-180.png",
  }));
});

self.addEventListener("notificationclick", (e) => {
  e.notification.close();
  const url = (e.notification.data && e.notification.data.url) || "/";
  e.waitUntil((async () => {
    const wins = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
    for (const w of wins) {
      w.postMessage({ open: url });
      return w.focus();
    }
    return self.clients.openWindow(url);
  })());
});
