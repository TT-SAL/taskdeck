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

  // Cache first, then catch up. The shell is this app's own file and changes
  // only when the binary is rebuilt, so waiting on the network to hand back
  // the same bytes is a round trip spent for nothing — and on a phone on
  // mobile data that round trip is most of what "the app is slow to open"
  // means. Serve the copy on disk immediately, fetch in the background, and
  // let the new one be there next time.
  //
  // The trade is that a rebuilt page appears one launch late. That is the
  // right way round: this is a calendar someone opens to check a time, and a
  // second of waiting every single time costs more than a stale layout once.
  // Started here rather than after the cache lookup, for two reasons: it runs
  // alongside the lookup instead of after it, and `waitUntil` has to be called
  // while the event is still dispatching or the refresh can be cancelled the
  // moment the page is closed — which is exactly when someone glances at the
  // day and pockets the phone.
  const fresh = fetch(request)
    .then((response) => {
      // Only a good answer replaces it: an error page from a server
      // mid-restart must not become the shell.
      if (response.ok) {
        const copy = response.clone();
        caches.open(CACHE).then((cache) => cache.put(shellPath, copy)).catch(() => {});
      }
      return response;
    })
    .catch(() => Response.error());
  event.waitUntil(fresh.catch(() => {}));
  event.respondWith(caches.match(shellPath).then((cached) => cached || fresh));
});
