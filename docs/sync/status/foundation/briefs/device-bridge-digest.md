# Brief: code digest for `device.md` and `bridge.md` (a fact sheet, not a spec)

Write TWO files, both in `docs/sync/status/foundation/digests/`:
`device-digest.md` and `bridge-digest.md`. Change nothing in the repository. No git state changes. Do not compile.

Purpose: the drafters of `docs/sync/spec/device.md` and `docs/sync/spec/bridge.md` need exact facts about
today's code, each with `file:line`. Facts only: no design, no opinions. Quote names, constants, routes,
header names, storage keys and JSON shapes verbatim. Use Grep and ranged reads; `src/phone.html` is 288 KB
and `src/phone.rs` 168 KB, so never read them whole. DOCUMENTATION.md §21–§23 and MOBILE.md explain intent;
the code wins where they differ.

## `device-digest.md` (today's runtime around the board)
1. **Folders and locks**: `paths.rs` (`AppDirs`, how the data directory is resolved on each OS, the lock file
   and what a contended lock does today, DOCUMENTATION.md §4.1), the first-contact set-aside, `CLIENT_MARKER`
   and `forget_replica` in `sync.rs`.
2. **Atomic file helpers**: `oversafe_activesave` and the other save functions (temp file, fsync, rename,
   directory fsync), exactly as they work, and the order `retire`, `Restore` and `ForgetArchived` write in.
3. **The outbox today** (`sync.rs`): `Queued`, persistence, the request key, retry and error mapping
   (which statuses keep, which drop), the listener, backoff constants and timeouts.
4. **ICS fetching** (`subscriptions.rs`): the HTTP client configuration (https only, redirects, timeout, body
   cap), `checked_url`, the refresh cadence, the window (days back and forward), what is cached on disk and
   how, the overlay type, its digest and `covers`, error strings that include URLs, the auto-rename from the
   feed's own name (`ui.rs` around the `better_name` call), the quarantine of a panicking or bad feed if any.
5. **Weather** (`weather.rs`): the Open-Meteo request (URL, parameters, precision of coordinates), cadence,
   the report type, where the latest report is held.
6. **Images** (`phone.rs` backdrop code, `color.rs`): how an uploaded picture is decoded (limits, formats
   guessed), cropped, re-encoded (AVIF, JPEG), stored (`phone_background.img`), and served; the Look dials.
7. **Random values and tokens**: `generate_token`, `random_bytes` and the non-Unix fallback (`phone.rs`).
8. **Configuration**: every key of `userconfig.toml` (`initialization.rs`) with its type, default and
   meaning, marked per-device or board-like; `colorschemes.json`.
9. **Threads and wake-ups**: which background threads exist, `phone::Wake`, how the UI learns of changes.
10. **Process entry points**: what `main.rs` and `server_main.rs` do at start and exit that a new runtime
    must also do (flags, single-instance behaviour, shutdown saves).

## `bridge-digest.md` (the phone page and the server that answers it)
1. **Every network request the page makes**: for `api()` (around phone.html:1240) list every call site with
   method, path, query, body, headers, the response fields it reads, and what it does on each failure.
   Then every request outside `api()`: the photo, the sky SVGs, Open-Meteo, the manifest, the service
   worker registration, anything else (`fetch(`, `XMLHttpRequest`, `new Image`, `src=`, `href=`, `import(`,
   `navigator.sendBeacon`, `WebSocket`, `EventSource`).
2. **Every op the page posts to `/api/command`** (the 17 verbs): the exact JSON it sends for each, where it
   is built, what it expects back, and how Undo is expressed. How `session` values are used.
3. **Every route the server answers** (`phone.rs` `route()` and helpers): method, path, auth (the token
   header and query), request and response shapes, status codes, caching headers, the CSP and other response
   headers verbatim (around phone.rs:1209-1219), the request-key replay cache, `/api/wait` semantics,
   `/api/look`, `/api/background`, `/api/weather`, `/api/state`, `/api/board`, `/calendar.ics`, the page,
   the manifest, `sw.js`, the icons and SVGs.
4. **Everything the page stores**: every `localStorage` and `sessionStorage` key, what it holds, where it is
   read and written (the outbox queue, the day and month caches, the chosen view, "don't warn" flags, the
   phone's own weather place, caches of weather and sky), Cache Storage use, cookies if any.
5. **The service worker** (`src/phone_sw.js`): what it caches and intercepts, and every place the page
   depends on it.
6. **Page behaviour that depends on the transport**: the token gate, offline banners and the "as of" line,
   the waiting marks on queued edits, how refusals are shown, the long-poll loop, retry timers.
7. **DOM sinks and navigation**: the single `innerHTML` sink and its callers, every place a URL is opened
   or assigned, `target=`, forms, file inputs (the photo picker), geolocation use, clipboard use, `window.open`.
8. **Inline script and style facts needed for a strict CSP**: one inline script or several, inline event
   handlers, inline styles, `eval`-like calls, data: or blob: URLs.
9. **Sizes**: lines and bytes of the page's CSS and JS; the served, gzipped size.

Be complete rather than brief. End each file with a list of what you could not determine.
Return a short text summary of each file.
