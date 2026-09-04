// TaskDeck's service worker: keeps the phone page's shell so it opens with
// the server unreachable. The page then shows the last snapshot it kept and
// says as of when. Only the shell and the icon are cached, network-first so
// an update to the page arrives the next time it can; the API, the feed and
// the manifest are never cached — a stale answer to "what is on today" is
// worse than no answer, and the page has its own memory of the day.
//
// A service worker needs a secure context: https, or localhost. Over plain
// http on a LAN address the browser refuses to install it and the page works
// exactly as before, without the offline shell. `SERVER.md` shows how
// Tailscale gives the server https.
const CACHE = 'taskdeck-shell-v1';
const SHELL = ['/', '/icon.png'];

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE).then((cache) => cache.addAll(SHELL)).then(() => self.skipWaiting()),
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener('fetch', (event) => {
  const request = event.request;
  if (request.method !== 'GET') return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  const path = url.pathname;
  // Data is never served from here.
  if (path.startsWith('/api/') || path === '/calendar.ics' || path === '/manifest.webmanifest' || path === '/sw.js') return;

  const shellPath = (path === '/' || path === '/index.html' || request.mode === 'navigate') ? '/' : path;
  if (!SHELL.includes(shellPath)) return;

  event.respondWith(
    fetch(request)
      .then((response) => {
        // Keep the freshest copy, keyed without the token in the query.
        const copy = response.clone();
        caches.open(CACHE).then((cache) => cache.put(shellPath, copy)).catch(() => {});
        return response;
      })
      .catch(() => caches.match(shellPath).then((cached) => cached || Response.error())),
  );
});
