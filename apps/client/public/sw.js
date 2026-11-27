/* Routune service worker: shows Web Push notifications (PRD BL1, T4.2.4).
 * The server sends JSON `{ title, body, url, tag }` (server/crates/notifications/src/push.rs);
 * a click focuses an open tab of the app, or opens one, at `url`. Nothing is cached: the app
 * works only online (ADR-0003). */

self.addEventListener("install", () => {
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});

self.addEventListener("push", (event) => {
  let message = {};
  try {
    message = event.data ? event.data.json() : {};
  } catch {
    message = {};
  }
  const title = typeof message.title === "string" && message.title ? message.title : "Routune";
  event.waitUntil(
    self.registration.showNotification(title, {
      body: typeof message.body === "string" ? message.body : "",
      tag: typeof message.tag === "string" ? message.tag : undefined,
      icon: "/favicon.ico",
      data: { url: typeof message.url === "string" ? message.url : "/" },
    }),
  );
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const path = (event.notification.data && event.notification.data.url) || "/";
  // Only paths of this app: never navigate elsewhere.
  const target = new URL(path.startsWith("/") ? path : "/", self.location.origin).href;
  event.waitUntil(
    self.clients.matchAll({ type: "window", includeUncontrolled: true }).then((windows) => {
      for (const client of windows) {
        if (new URL(client.url).origin === self.location.origin && "focus" in client) {
          return client.focus().then((focused) => (focused ? focused.navigate(target) : null));
        }
      }
      return self.clients.openWindow(target);
    }),
  );
});
