// TaskDeck's service worker: keeps the phone page's shell so it opens with
// the server unreachable. The page then shows the last snapshot it kept and
// says as of when. Only the shell and the icon are cached, and cache-first —
// see the fetch handler for why, and for the one-launch lag that buys; the
// API, the feed and the manifest are never cached — a stale answer to "what is on today" is
// worse than no answer, and the page has its own memory of the day.
//
// A service worker needs a secure context: https, or localhost. Over plain
// http on a LAN address the browser refuses to install it and the page works
// exactly as before, without the offline shell. `SERVER.md` shows how
// Tailscale gives the server https.
const CACHE = 'taskdeck-shell-v2';
// The background photo lives in its own cache, apart from the shell. It is the
// one big thing here — tens of kilobytes against the page's forty — and it
// changes on its own schedule, so keeping the two separate means a shell
// version bump does not throw the picture away and vice versa.
const PHOTO = 'taskdeck-photo-v1';
const KEEP = [CACHE, PHOTO];
// The page and nothing else. The icon used to be here, which cost 660 KB of
// cache for a picture only the operating system ever looks at, and only when
// the app is installed. The page's own icon links point at the scaled 50 KB
// file, which the browser's ordinary cache handles well enough for something
// drawn at sixteen pixels.
const SHELL = ['/'];

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE).then((cache) => cache.addAll(SHELL)).then(() => self.skipWaiting()),
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(keys.filter((key) => !KEEP.includes(key)).map((key) => caches.delete(key))))
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

  // The background, cache-first and kept. Its name carries the content's own
  // hash, so a cached copy can never be the wrong picture — which is what makes
  // this safe to answer from disk without asking the server anything at all.
  // Without it the photo lives only in the browser's ordinary HTTP cache, which
  // is evictable, and the first thing a phone low on space throws away; the
  // page then opens over a flat ground with the server unreachable, having kept
  // everything else it needed.
  if (path.startsWith('/bg-')) {
    event.respondWith(photo(request, path));
    return;
  }

  const shellPath = (path === '/' || path === '/index.html' || request.mode === 'navigate') ? '/' : path;
  if (!SHELL.includes(shellPath)) return;
  // `request.mode === 'navigate'` is true for a top-level load of ANY path on
  // this origin — typing an icon or a `/bg-<hash>.jpg` URL into the address bar
  // included. Those map to '/' above, so without the content-type check below
  // one such navigation would store a PNG as the app shell and every later
  // open would render the image instead of the page.

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
      // Only a good answer replaces it, and only an answer that is actually
      // the page: an error page from a server mid-restart must not become the
      // shell, and neither must an image someone navigated to directly.
      const kind = response.headers.get('content-type') || '';
      if (response.ok && (shellPath !== '/' || kind.includes('text/html'))) {
        const copy = response.clone();
        caches.open(CACHE).then((cache) => cache.put(shellPath, copy)).catch(() => {});
      }
      return response;
    })
    .catch(() => Response.error());
  event.waitUntil(fresh.catch(() => {}));
  event.respondWith(caches.match(shellPath).then((cached) => cached || fresh));
});

async function photo(request, path) {
  const cache = await caches.open(PHOTO);
  // Keyed by path alone. The URL carries the token in its query, and a token
  // that is re-minted must not orphan a picture that has not changed.
  //
  // Keying by path also steps around `Vary: Accept`: the server sends AVIF or
  // JPEG by what the request asked for, and a path key keeps whichever this
  // browser was given the first time. That is right for a phone, which does not
  // change its mind about AVIF between openings, and the hash in the name means
  // the bytes are the same picture either way.
  const hit = await cache.match(path);
  if (hit) return hit;
  let response;
  try {
    response = await fetch(request);
  } catch (_) {
    // Offline and nothing kept: the page falls back to its flat ground, which
    // is what it did before any of this. Not an error worth throwing.
    return Response.error();
  }
  if (response.ok) {
    // One picture at a time. The name holds the content's hash, so a different
    // name is a different photo and the old one is dead weight — this is where
    // every background ever set would otherwise pile up.
    for (const key of await cache.keys()) {
      if (new URL(key.url).pathname !== path) await cache.delete(key);
    }
    await cache.put(path, response.clone());
  }
  return response;
}
