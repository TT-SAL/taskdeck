# R3: Free hosting, uptime and sync latency

> Working paper from the design run of 2026-11-20 that produced [`SYNC.md`](../../../SYNC.md). It is an agent's report, kept for the detail the design compresses: file and line references, sources, and the reasoning behind each requirement. Where it disagrees with `SYNC.md` or [`SYNC_PLAN.md`](../../../SYNC_PLAN.md), those win. Line numbers refer to `day-planner` at commit `1cfff2f`.

# R3: Free hosting, uptime and sync latency (as of 2026-11-20)

## 1. Summary
- **Primary relay: Cloudflare Workers Free plus one SQLite-backed Durable Object (DO) per board.** It is the only EUR 0 option I found that meets all four needs at once. It is naturally a single sequencer. It can push changes over hibernating WebSockets. It fails closed rather than billing. It is cheap to leave, because a blind relay is a few hundred lines. Normal use stays under about 2% of every free quota (§6).
- **Availability is good, not great.** In June 2025 SQLite-backed DOs had a global incident with errors peaking at 22% for about 2.5 h. In 2026 there were many regional "increased errors" incidents, including at least four touching Western Europe between 15 Oct and 20 Nov 2026. My estimate is **99.7-99.9% relay availability (about 9-26 h/year)**, mostly in partial windows of 15-120 minutes (ASSUMED). With offline-first clients, an outage delays sync; it never makes the app unusable.
- **Do not run a hot second sequencer.** Two relays with automatic failover need an arbiter, or they split-brain. A leaderless design changes core invariants. The simplest robust setup is:
  - one hot relay;
  - a full replica on every device;
  - encrypted off-relay backups;
  - a rehearsed manual "re-home" with an epoch bump.
  A "mailbox" secondary that never sequences is an optional later step.
- **Free limits are hit only by your own bugs or a deliberate attacker**, not by normal use. Someone who learns the hostname but holds no token can burn the Worker's 100k requests/day in under a minute. Sync then pauses until 00:00 UTC, but no data is exposed or lost. Closing that hole fully needs a custom domain (about EUR 10/yr) plus a free WAF rule. That the WAF rule runs before the Worker is ASSUMED.
- **Rejected options:**
  - Deno Deploy: sunset in April 2027.
  - Fly.io: no free tier.
  - Vercel and Netlify: no WebSockets, and going over a limit pauses the account.
  - Oracle and GCP free VMs: operations burden, idle reclamation or a required billing account.

  Supabase Free or a private GitHub repo are fine as cold standby or backup targets.

## 2. Cloudflare Workers + DO (Free) as the blind relay

### 2.1 Limits beyond those in the brief (VERIFIED unless marked)
- **Worker requests:** 100,000/day, reset at 00:00 UTC; above that, **Error 1027** [W-lim]. "Only requests that hit a Worker will count against your limits" [W-price].
- **CPU:** **10 ms per HTTP request on Free** [W-lim]. The DO limits page says "Durable Objects are a special kind of Worker, so Workers limits also apply", but its own table lists a 30 s default [DO-lim]. Assume 10 ms per DO invocation on Free (UNCERTAIN). A relay append plus broadcast needs well under 1 ms; the unknown is Rust/Wasm cold-start CPU.
- **WebSockets:**
  - The upgrade counts as 1 Worker request. "WebSocket messages routed through a Worker do not count as requests" [W-price].
  - At the DO, incoming messages count at 20:1. There is **no charge for outgoing messages or for incoming protocol pings**, and setWebSocketAutoResponse replies are not charged [DO-price].
  - Ping/pong does not interrupt hibernation [DO-ws].
- **Hibernation and billing:**
  - A DO hibernates after **10 s** with no events. A non-hibernatable idle object is evicted after 70-140 s [DO-life].
  - Duration is billed only while the object is running, or idle but non-hibernatable, at **128 MB regardless of actual use** [DO-price].
  - Timers, alarms, in-flight requests and outbound WebSockets all prevent hibernation [DO-ws].
- **Code updates:** connections survive if the object adopts new code after hibernating; a forced update disconnects them [DO-ws].
- **SQL accounting:**
  - Every index-row update counts as an extra row written.
  - **Deletes count as rows written.**
  - rowsRead counts the rows a query scans [DO-sql, DO-price].
  - Maximum row or BLOB size is 8 MB; a received WebSocket message can be up to 32 MiB [DO-lim].
- **Bundle size:** 64 MiB uncompressed on both plans, with no compressed limit [W-lim]. Wasm size is not a blocker.
- **workers.dev:** Browser Integrity Check always runs there, and some default library User-Agents get **Error 1010** before the Worker runs [W-dev]. Native clients must send a descriptive User-Agent.
- **Idle WebSockets:** secondary sources say Cloudflare closes WebSockets idle for about 100 s (UNCERTAIN) [ws-idle]. Send protocol pings every 30-45 s; they are free.

### 2.2 Rust (workers-rs) vs TypeScript
**What workers-rs offers.** Version 0.9.0 exposes the hibernation API and SqlStorage [rs]:
- `State::accept_web_socket`, `accept_websocket_with_tags`, `get_websockets`, `set_websocket_auto_response`;
- `DurableObject::websocket_message`, `websocket_close`, `websocket_error`.

**Its gaps.**
- The README still warns of "rough edges, some unimplemented APIs".
- `State` has no PITR bookmark methods.
- No cold-start figures are published, which matters under a 10 ms CPU cap.

**Recommendation: write the relay in TypeScript**, about 300-500 lines, on the platform's first-class path. A blind relay never decrypts or applies commands, so it shares no code with the Rust core. Deploying workers-rs needs wrangler (npm) anyway, so TypeScript adds no new toolchain.

### 2.3 Placement and durability
- **Placement:** a DO is created near the first `get()`, and location hints are best-effort [DO-loc].
  - **Objects never move**, so create the DO from home.
  - An `eu` jurisdiction exists.
- **Durability:** each SQLite commit is forwarded to 5 followers in nearby data centres and acknowledged once 3 confirm [SRS-blog].
  - Output gates hold responses until those writes are confirmed, so a relay "ack" means the write is on 3 of 5 machines.
  - **Point-in-time recovery (PITR) covers 30 days**, via `getBookmarkForTime` and `onNextSessionRestoreBookmark` [DO-sql]. Whether it is available on Free is not stated.
- **SLA:** none on Free (ASSUMED).

### 2.4 Outage record and availability estimate
| Date (UTC) | Incident | Impact |
|---|---|---|
| 2025-02-06 | R2 gateway disabled by a mistaken phishing takedown | 59 min; DOs +0.09% errors in North America [cf-feb25] |
| 2025-06-12 | Third-party storage behind KV failed | ~2.5 h; **SQLite DOs peaked at 22% errors** [cf-jun25] |
| 2025-11-18 | Bot-management file crashed the core proxy | 11:20-14:30 core impact, all clear 17:06; KV 5xx; DO impact not stated in the post [cf-nov25] |
| 2025-12-05 | Proxy change | ~25 min [trackers] |
| 2026-07-16, 2026-08-14 | DO (+D1/Workflows) outage; DO availability drop | hours [trackers] |
| 2026-10-15, 10-21, 10-22, 11-05 | DO errors in Western Europe or Lisbon | ~1 h; ~46 min; fixed after 28 min (incident stayed open 6 h 18 m); impact window unclear [trackers] |
| 2026-10-28, 11-04 | DO errors in Eastern North America | ~1.5 h; minutes |
| 2026-11-20 | "R2 and Durable Objects availability issues" | 02:52-04:35 [cf-status] |

Third-party trackers overstate durations because they include monitoring time.

A DO lives in one data centre, so only incidents there, in its region, or global ones affect it. That suggests about 10-20 incidents a year of 15-120 min, mostly partial (retries succeed), plus 1-2 global ones. **Estimate: 99.7-99.9%** (ASSUMED). The incident rate in late 2026 is higher than in 2025.

### 2.5 Terms, suspension, export
**Terms.** The Developer Platform terms (updated 2026-09-28) [cf-terms]:
- forbid malware, phishing, volumetric attacks and deceptive names;
- say Cloudflare "may temporarily limit your storage and/or the number of requests";
- say nothing against a personal encrypted sync relay.

**Suspension.** Cloudflare "may, but is not obligated to, limit or suspend" access over suspected violations. After termination, content is reachable for up to 30 days, with no obligation to keep it. Takedowns do happen; one caused the February 2025 R2 outage.
- A JSON-only relay with no HTML is an unlikely target (ASSUMED).
- The design must still survive losing the account, which it does because every device holds the whole board.

**Export.** There is no bulk export. Data Studio (beta, October 2025) can view and edit DO SQLite in the dashboard [cf-ds]. Add an authenticated endpoint that streams the ciphertext. The real export is any device's replica.

### 2.6 R2
- **Free tier:** 10 GB-month of storage, 1M Class A and 10M Class B operations per month, free egress [r2].
- **Beyond that it bills, with no cap stated:** $4.50 per million Class A, $0.36 per million Class B, $0.015 per GB-month.
- **Payment method:** reportedly required (UNCERTAIN, third-party source).

DO storage fails closed and 5 GB is plenty, so **keep snapshots and images in the DO, in chunks of 1 MB or less, and skip R2** at EUR 0.

## 3. Alternatives
| Option (free tier) | EUR 0? | Can be the single sequencer? | Push? | Durability, limits, failure mode | Verdict |
|---|---|---|---|---|---|
| Deno Deploy + KV | - | KV atomics | yes | "Deno Deploy will be sunset in April 2027" [deno] | Reject |
| Supabase | yes | yes (Postgres transactions) | Realtime | one region; pauses after 7 days of low activity, restorable for 1 year [supa]; low lock-in | Cold standby or mailbox |
| Firebase Spark | yes | yes (Firestore transactions) | SDK listeners | 50k reads, 20k writes, 20k deletes per day; 1 GiB; resets around midnight Pacific; no Functions on Spark; FCM free [fb]; no Rust SDK; high lock-in | Use FCM only |
| Vercel Hobby + Upstash | yes | Redis INCR/XADD | no (poll) | Upstash: 500k commands/month, 256 MB [upstash]. Hobby: over a limit, the account may be paused for 30 days; non-commercial use only [vercel] | Reject |
| Netlify | 300 credits/month, hard cap [netlify] | weak | no | sites stop at the cap | Reject |
| Fly.io | no: new customers get no free allowance [fly] | - | - | - | Reject |
| Oracle Always Free VM | card required | yes | yes | A1 is now 2 OCPU/12 GB. Counted as idle when, over 7 days, p95 CPU, network (and memory, A1 only) are all below 20%; a relay qualifies and can be reclaimed [oci]. Operations burden | Not primary |
| GCP e2-micro | billing account; IPv4 charge UNCERTAIN | yes | yes | US regions only; 30 GB disk; 1 GB/month egress [gcp]; operations burden | Reject |
| GitHub private repo | yes | slow compare-and-swap (ref fast-forward) | no (poll) | very durable and versioned; 5,000 req/h; 80 content-creating req/min [gh] | Encrypted backup target |

## 4. Uptime strategies

**(a) One relay plus offline-first clients.**
- **Local availability is about 100%.** Each device views and edits its own replica, queues commands, and fetches subscriptions and weather itself.
- **"Down" means cross-device propagation pauses.**
  - The outbox flushes in order on recovery.
  - Sequencer order settles conflicts.
  - Refused commands are reported, as today.
- **Sync availability: 99.7-99.9%.** This is a large gain over today, where the phone needs a reachable server behind Tailscale.

**(b) Two relays from different providers.**
- **Hot standby with automatic failover: reject.**
  - With two nodes and no arbiter, a partition gives two sequencers (one device cannot reach relay A while another can).
  - Leases would need the standby to arbitrate and the primary to renew every ~30 s. That defeats hibernation and adds the riskiest code in the system.
  - About 3-5 weeks of work.
- **Manual failover with an epoch: keep it, as the disaster-recovery and exit procedure.**
  - The user promotes a cold standby, seeded from one device's confirmed snapshot at epoch N+1.
  - Devices refuse epoch N afterwards, and pending outboxes replay.
  - It takes hours and causes no split brain.
- **Mailbox secondary (optional, later): the cheapest real "second server".**
  - Relay B stores each device's encrypted pending commands but never orders them.
  - Devices show other devices' pending commands as tentative. Each device then needs its own disjoint temporary-id range.
  - Relay A stays the only sequencer, and each device submits only its own outbox to it.
  - If the two providers are independent at 99.7%, both are down about 5 min/year, so tentative propagation keeps working through A's outages.
  - About 1 week of work.
- **Leaderless (hybrid logical clocks plus deterministic replay): not now.** It needs:
  - globally unique ids, replacing the CLIENT_ID_FLOOR remapping (sync.rs:74, main.rs:165/182, board.rs:58);
  - clock stamps applied at the originating device;
  - rewinding and replaying when a late command arrives;
  - compaction only below a stability point, which stalls whenever a device is lost;
  - giving up synchronous refusals.

  Sync then works whenever any relay is up, but the design drops the one-writer invariant (DOCUMENTATION.md §4.1). Estimated cost: an extra 3-6 weeks.

**(c) LAN or device-to-device: reject.** This means the desktop as an mDNS-discovered peer, for example with the `mdns-sd` crate plus a Noise handshake from `snow`.
- It only helps when the phone and desktop share a LAN while the internet or relay is down.
- It opens a listening port on the main machine, and on the laptop when it is on public Wi-Fi.

**(d) Backups.**
- **Already covered:** replicas on 3-4 devices, 3-of-5 relay replication and 30-day PITR together cover hardware loss.
- **Remaining risks:** logical corruption and losing the account.
- **Nightly snapshots:** each desktop writes an encrypted snapshot into a folder that OneDrive (Windows) or iCloud Drive (macOS) already syncs, so no new service is needed. Keep 30 daily and 12 monthly copies; a private GitHub repo is an optional extra.
- **Restore:** import a snapshot on any device, then seed a relay at epoch N+1. Rehearse it quarterly.

**Recommended: (a) + (d) + the manual epoch procedure.**

## 5. Sync latency
| Device state | Mechanism | Expected | Target |
|---|---|---|---|
| Desktop or laptop, awake | WebSocket push through the DO | ~0.1-0.3 s within a region, including wake from hibernation (ASSUMED; no published figures) | p95 < 1 s |
| Laptop after sleep | reconnect, then catch up | 1-5 s; Wi-Fi rejoin dominates | < 5 s once the network is back |
| Phone app in foreground | WebSocket (OkHttp, 30 s ping) | as for desktops | p95 < 1 s |
| Phone app opened | show the local replica, then connect and read new entries | 0.3-1 s typical; 2-5 s on poor mobile data | instant view; fresh in < 2 s (p90) |
| Phone in background, screen on | optional content-free FCM wake-up, then expedited work | seconds | best effort < 1 min |
| Phone in background, no FCM | WorkManager periodic job, minimum 15 min [wm] | 15-60 min | covered by sync on open |
| Phone in Doze | FCM messages that show no notification are deprioritized to normal priority and wait for Doze to end [fcm] | minutes to hours | covered by sync on open |
| Samsung "sleeping" app | apps unused for 3 days are put to sleep and background work stops [dkma] | until opened | add the app to "Never sleeping apps" |
| Two weeks offline | ~4,200 commands (~1.6 MB), or a snapshot plus the tail | 2-10 s on 4G | < 10 s |

**Notes:**
- A persistent background socket is not viable: Android 15 caps dataSync foreground services at 6 h per 24 h [fgs].
- **Do not port today's /api/wait long-poll (40 s, sync.rs:85) to a DO.** A pending request keeps the object running, which costs 83% of the duration quota per object.
- TaskDeck has no reminders today (grep over src/), so sync-on-open carries freshness and FCM can wait.

## 6. Quota math
Assumptions: 4 devices; 300 edits/day; phone opened 50 times plus 48 background syncs per day; desktops connected all day.

| Free limit | Typical day | Stress day | Bug or attack | At the limit |
|---|---|---|---|---|
| Worker requests (100k) | ~250 (≈120 WebSocket connects + phone background + snapshots/blobs + retries): 0.25% | ~2,500 | 43,200 per device per day if a connect-then-drop loop resets backoff to 2 s (today's listener resets after any successful wait, sync.rs:786/791); an attacker has no bound | Error 1027 until 00:00 UTC |
| DO requests (100k) | ~300 (connects + 600 WebSocket messages ÷ 20) | ~3,000 | same as above | DO calls fail until 00:00 UTC |
| DO duration (13,000 GB-s) | ~6 (1,000 events × 50 ms × 0.125 GB); ~18 with FCM | ~60 | broken hibernation: 10,800 per object (83%); two such objects exhaust it around 14:30 UTC | DO calls fail |
| Rows written (100k) | ~1,700 (≈3 per append, cursors, compaction deletes) | ~15k (a 5,000-command import) | - | appends refused; outboxes keep the edits |
| Rows read (5M) | ~10k | ~100k | an unindexed scan of a 20k-row log on every append ≈ 6M | reads fail |
| Storage (5 GB; 1 GB per object per the FAQ) | 50-150 MB after a year (log 53 MB/yr uncompacted, 3 snapshots, ~10 images) | - | - | writes fail |

**Headroom is at least 97% on every limit**; only bugs or an attack reach them. Guards:
- backoff with full jitter from 1 s to 60 s, reset only after at least 60 s of healthy connection;
- a per-device request budget (e.g. 3,000/day), then fall back to 5-minute polling;
- proper indexes, plus tests that assert rowsRead;
- no timers in the DO.

## 7. Abuse resistance on the Free plan
**Can someone without a token lock you out?** **Yes, on workers.dev.**
- Rejected requests (401) still count against the Worker quota, and 100k requests take under a minute at moderate concurrency.
- Sync pauses until 00:00 UTC, and the attack can be repeated every day.
- Confidentiality and integrity are unaffected, because the data is end-to-end encrypted, and the outboxes keep every edit.

**Keeping the hostname secret is weak protection.**
- CT logs reportedly show only the account wildcard (a single anecdote).
- Passive-DNS sites list many workers.dev names, and the hostname is visible in TLS SNI to anyone on the network path [ws-ct].

**Free mitigations:**
1. Authenticate in the Worker before calling the DO, with a constant-time HMAC check of a per-device token. Without a valid token, nobody can touch the DO's quotas or duration.
2. Use a random Worker name, serve no HTML and send no CORS headers.
3. The Free rate-limiting rule is weak: 1 rule, IP-only, 10 s period, and only the path field is available [rl].
4. Gate the Worker behind the WAF:
   - put it on a custom domain on Cloudflare;
   - set `workers_dev = false` and turn preview URLs off;
   - add a WAF custom rule that blocks any request without a secret path prefix (Free: 5 rules, no regex [waf]).

   Blocked requests never "hit a Worker", so they do not count (pricing VERIFIED; that the WAF runs before the Worker is ASSUMED). This needs a domain, about EUR 10/yr (ASSUMED).
5. Workers Paid (USD 5/month) removes the daily cap but bills overage instead.

**Recommendation:**
- Start with mitigations 1 and 2.
- Have mitigation 4 ready as a 30-minute switch, or adopt it from day one if EUR 10/yr is acceptable.
- Clients should show "relay quota exhausted until 00:00 UTC" and stop retrying.

## 8. Recommendation

**Setup**
- **Account and plan:** one Cloudflare account, secured with a passkey and a dedicated email, on Workers Free.
- **Relay code:** one TypeScript Worker plus one SQLite-backed DO class, with one object per board, named by a random 128-bit id.
  - Use the hibernation API only, protocol pings, and no timers; compact on append.
- **Blind API:**
  - `hello(device, last seq)`;
  - `append(ciphertext, idempotency key)`, which returns the sequence number;
  - `read-since`, paged;
  - snapshot put/get and blob put/get, both chunked;
  - push over WebSocket.
- **Storage rules:** a unique index on the idempotency key; keep the latest snapshot plus a 60-day tail.
- **Desktops:** a blocking WebSocket thread, for example tungstenite with rustls, matching the no-async precedent in MOBILE.md §3.4.
- **Android:** a native app with a local replica, a WebSocket while in the foreground, sync on open, and a 30-minute WorkManager job. FCM wake-ups only if you want them later.
- **Backups:** a nightly encrypted snapshot into the OneDrive or iCloud folder.

**Expected availability and latency**
- Local use: about 100%.
- Cross-device sync: 99.7-99.9%, about 9-26 h/year, mostly in 15-120-minute windows.
- Latency: under 1 s between connected devices; the phone is fresh within 2 s of opening.

**Worst cases**
- **Cloudflare incident (regional or global):** sync is delayed for as long as it lasts.
- **Quota exhausted by a bug or an attacker:** sync is delayed until 00:00 UTC.
- **Account suspended or a platform change:** the relay is gone but devices keep everything; re-home within hours.
- **Compromised Cloudflare account:** it cannot read or forge data, but it can withhold or roll back. Clients should detect this with a hash chain (covered by the crypto design).
- **Lost phone:** only its unsent outbox is lost.

**Exit plan**
1. Specify and version the relay protocol, with at most 8 operations.
2. Keep a second implementation that passes the same tests: either a Rust standalone relay on a EUR 1-2/month VPS behind a body-buffering proxy (because of the tiny_http stall finding), or Supabase.
3. To re-home: deploy the standby, seed it from a device snapshot at epoch N+1, and re-pair devices by QR code. Rehearse once a year; it takes about 1-2 hours.

## Sources
[W-lim] https://developers.cloudflare.com/workers/platform/limits/ · [W-price] https://developers.cloudflare.com/workers/platform/pricing/ · [W-dev] https://developers.cloudflare.com/workers/configuration/routing/workers-dev/ · [DO-lim] https://developers.cloudflare.com/durable-objects/platform/limits/ · [DO-price] https://developers.cloudflare.com/durable-objects/platform/pricing/ · [DO-ws] https://developers.cloudflare.com/durable-objects/best-practices/websockets/ · [DO-life] https://developers.cloudflare.com/durable-objects/concepts/durable-object-lifecycle/ · [DO-sql] https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/ · [DO-loc] https://developers.cloudflare.com/durable-objects/reference/data-location/ · [SRS-blog] https://blog.cloudflare.com/sqlite-in-durable-objects · [rs] https://docs.rs/worker/0.9.0/worker/durable/struct.State.html , https://github.com/cloudflare/workers-rs · [cf-feb25] https://blog.cloudflare.com/cloudflare-incident-on-february-6-2025/ · [cf-jun25] https://blog.cloudflare.com/cloudflare-service-outage-june-12-2025/ · [cf-nov25] https://blog.cloudflare.com/18-november-2025-outage/ · [cf-status] https://www.cloudflarestatus.com/history · [trackers] https://pulsetic.com/status/cloudflare/incidents/152441/ , https://pulsetic.com/status/cloudflare/incidents/157723/ , https://shattered.io/cloudflare-outage-august-2026/ , https://www.pulsapi.com/blog/cloudflare-outage-history · [cf-terms] https://www.cloudflare.com/service-specific-terms-developer-platform/ · [cf-ds] https://developers.cloudflare.com/changelog/2025-10-16-durable-objects-data-studio · [r2] https://developers.cloudflare.com/r2/pricing/ · [rl] https://developers.cloudflare.com/waf/rate-limiting-rules/ · [waf] https://developers.cloudflare.com/waf/custom-rules/ · [deno] https://deno.com/deploy · [supa] https://supabase.com/docs/guides/platform/free-project-pausing · [fb] https://firebase.google.com/pricing · [upstash] https://upstash.com/pricing/redis · [vercel] https://vercel.com/docs/plans/hobby · [netlify] https://docs.netlify.com/manage/accounts-and-billing/billing/billing-for-credit-based-plans/credit-based-pricing-plans/ · [fly] https://fly.io/docs/about/discontinued-plans/ · [oci] https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm · [gcp] https://docs.cloud.google.com/free/docs/free-cloud-features · [gh] https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api · [fcm] https://firebase.google.com/docs/cloud-messaging/android-message-priority · [fgs] https://developer.android.com/develop/background-work/services/fgs/timeout · [wm] https://developer.android.com/reference/androidx/work/PeriodicWorkRequest · [dkma] https://dontkillmyapp.com/samsung · [ws-idle] https://developers.cloudflare.com/network/websockets/ · [ws-ct] https://dns.ninja/en/dns-lookup/dev/workers

---

## Key facts

- **[VERIFIED]** Workers Free: 100,000 requests/day, resetting at midnight UTC; when exceeded Cloudflare returns Error 1027. CPU 10 ms per HTTP request on Free (5 min on Paid). Worker size 64 MiB uncompressed on both plans with no compressed-size limit. _(evidence: https://developers.cloudflare.com/workers/platform/limits/ (dateModified 2026-11-19))_
- **[VERIFIED]** Only requests that hit a Worker count against limits and the bill; a WebSocket upgrade is one request and 'WebSocket messages routed through a Worker do not count as requests'. _(evidence: https://developers.cloudflare.com/workers/platform/pricing/ (dateModified 2026-11-18))_
- **[VERIFIED]** DO pricing: incoming WebSocket messages 20:1; no charge for outgoing WebSocket messages or incoming protocol pings; setWebSocketAutoResponse replies are not charged; duration billed for 128 MB regardless of actual usage; deletes count as rows written; idle hibernation-eligible objects are not billed for duration. _(evidence: https://developers.cloudflare.com/durable-objects/platform/pricing/ (dateModified 2026-11-13))_
- **[UNCERTAIN]** 'Durable Objects are a special kind of Worker, so Workers limits also apply' while the DO table lists a 30 s CPU default per invocation; whether the 10 ms Free CPU cap applies to DO invocations is not explicit. _(evidence: https://developers.cloudflare.com/durable-objects/platform/limits/ (dateModified 2026-11-13))_
- **[VERIFIED]** A hibernatable DO hibernates after 10 s with no request/event; idle non-hibernatable objects are evicted after 70-140 s; duration is billed only when running or idle-non-hibernatable. _(evidence: https://developers.cloudflare.com/durable-objects/concepts/durable-object-lifecycle/ (dateModified 2026-10-30))_
- **[VERIFIED]** Ping/pong does not interrupt hibernation; alarms, incoming requests, setTimeout/setInterval and outbound WebSockets prevent it; hibernated connections survive new code, a forced update disconnects them. _(evidence: https://developers.cloudflare.com/durable-objects/best-practices/websockets/ (dateModified 2026-11-18))_
- **[VERIFIED]** Free DO SQL limits: max string/BLOB/row 8 MB; received WebSocket message max 32 MiB; every index row update counts as an additional row written; rowsRead (rows scanned) is billed. _(evidence: https://developers.cloudflare.com/durable-objects/platform/limits/ ; https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/)_
- **[UNCERTAIN]** Free per-object DO storage: the FAQ says 1 GB on Free, the table says 10 GB with no plan split; account total 5 GB. _(evidence: https://developers.cloudflare.com/durable-objects/platform/limits/)_
- **[VERIFIED]** workers-rs 0.9.0 supports the hibernation API (State::accept_web_socket, accept_websocket_with_tags, get_websockets, set_websocket_auto_response; DurableObject::websocket_message/close/error) and SqlStorage; README warns of 'rough edges, some unimplemented APIs'; no PITR bookmark methods on State. _(evidence: https://docs.rs/worker/0.9.0/worker/durable/struct.State.html ; https://docs.rs/worker/0.9.0/worker/durable/trait.DurableObject.html ; https://github.com/cloudflare/workers-rs)_
- **[VERIFIED]** A DO is instantiated near the first get(); location hints are best-effort; DOs do not change location after creation; jurisdictions eu/us/fedramp exist. _(evidence: https://developers.cloudflare.com/durable-objects/reference/data-location/ (dateModified 2026-06-26))_
- **[VERIFIED]** SQLite DO commits are forwarded to five followers in nearby data centres and acknowledged after three confirm; WAL batches (up to 10 s or 16 MB) go to object storage; PITR to any point in the last 30 days. _(evidence: https://blog.cloudflare.com/sqlite-in-durable-objects ; https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/)_
- **[VERIFIED]** June 12, 2025 outage (third-party storage behind KV): about 2.5 h; SQLite-backed DO error rate peaked at 22%; Workers scripts about 2%. _(evidence: https://blog.cloudflare.com/cloudflare-service-outage-june-12-2025/)_
- **[VERIFIED]** Nov 18, 2025 outage: failures from 11:20 UTC, core traffic largely normal by 14:30, all systems normal at 17:06; Workers KV elevated 5xx; the post does not mention DOs. _(evidence: https://blog.cloudflare.com/18-november-2025-outage/)_
- **[VERIFIED]** Feb 6, 2025: R2 gateway disabled by human error during a phishing takedown, 08:14-09:13 UTC; DOs +0.09% errors in North America until 09:36. _(evidence: https://blog.cloudflare.com/cloudflare-incident-on-february-6-2025/)_
- **[VERIFIED]** Cloudflare status history lists 'R2 and Durable Objects availability issues' on 2026-11-20, 02:52:58-04:35:18 UTC. _(evidence: https://www.cloudflarestatus.com/history (fetched 2026-11-20))_
- **[VERIFIED]** Several 2026 DO incidents in Western Europe: Oct 22 (investigating 17:51, fix implemented 18:19, resolved 00:09) and Nov 5 (resolved/monitoring at 13:23, closed Nov 6 09:05); trackers' durations include monitoring time. _(evidence: https://pulsetic.com/status/cloudflare/incidents/152441/ ; https://pulsetic.com/status/cloudflare/incidents/157723/ (third-party mirrors of cloudflarestatus))_
- **[ASSUMED]** Relay availability estimate for one DO: 99.7-99.9% (about 9-26 h/year), mostly partial 15-120 min windows. _(evidence: Extrapolated from the 2025-2026 incident record above; no SLA on Free)_
- **[VERIFIED]** Cloudflare Developer Platform terms: Cloudflare 'may temporarily limit your storage and/or the number of requests'; may limit or suspend for suspected violations; content accessible up to 30 days after termination with no retention obligation; prohibits malware, phishing, volumetric attacks, deceptive names. _(evidence: https://www.cloudflare.com/service-specific-terms-developer-platform/ (last updated 2026-09-28))_
- **[VERIFIED]** R2 free tier: 10 GB-month, 1M Class A, 10M Class B per month, free egress; overage billed ($4.50/M A, $0.36/M B, $0.015/GB-month) with no hard cap stated. _(evidence: https://developers.cloudflare.com/r2/pricing/ (dateModified 2026-10-30))_
- **[UNCERTAIN]** Enabling R2 requires adding a payment method. _(evidence: Third-party only: https://tech-insider.org/how-to-set-up-cloudflare-r2-2026/)_
- **[VERIFIED]** Free plan rate limiting: 1 rule, IP-only counting, 10 s period and 10 s mitigation timeout, fields limited to path and verified bot. Free WAF custom rules: 5 rules, no regex. _(evidence: https://developers.cloudflare.com/waf/rate-limiting-rules/ (2026-08-25) ; https://developers.cloudflare.com/waf/custom-rules/ (2026-10-16))_
- **[ASSUMED]** WAF custom rules on a zone are evaluated before the Worker runs, so blocked requests do not count toward the Workers daily quota. _(evidence: Workers pricing says only requests that hit a Worker count; ordering only from a community thread https://community.cloudflare.com/t/workers-behavior-with-firewall-rules/109182)_
- **[VERIFIED]** workers.dev always applies Browser Integrity Check; clients with certain default library User-Agents get Error 1010 before the Worker runs; workers_dev = false disables the route but not Preview URLs. _(evidence: https://developers.cloudflare.com/workers/configuration/routing/workers-dev/ (dateModified 2026-11-06))_
- **[VERIFIED]** Deno Deploy will be sunset in April 2027 (Deno team joined Cloudflare, announced Oct 9, 2026). _(evidence: https://deno.com/deploy banner; https://alternativeto.net/news/2026/10/deno-team-joins-cloudflare-as-runtime-development-winds-down/)_
- **[VERIFIED]** Supabase Free projects with low activity over 7 days are paused (a few requests per day prevent it); paused projects can be restored for up to 1 year. _(evidence: https://supabase.com/docs/guides/platform/free-project-pausing)_
- **[VERIFIED]** Firebase Spark: Firestore 1 GiB, 50K reads/day, 20K writes/day, 20K deletes/day, 10 GiB/month egress; Cloud Functions not applicable on Spark; FCM no-cost; quotas reset around midnight Pacific. _(evidence: https://firebase.google.com/pricing ; https://firebase.google.com/docs/firestore/quotas)_
- **[VERIFIED]** Vercel Hobby is non-commercial and an account over limits may be paused (wait up to 30 days); Upstash free is 500K commands/month and 256 MB; Netlify Free is 300 credits/month with a hard limit. _(evidence: https://vercel.com/docs/plans/hobby ; https://upstash.com/pricing/redis ; https://docs.netlify.com/manage/accounts-and-billing/billing/billing-for-credit-based-plans/credit-based-pricing-plans/ (via search snippets of official pages))_
- **[VERIFIED]** Fly.io offers no free allowance to new customers (pay-as-you-go with a short trial). _(evidence: https://fly.io/docs/about/discontinued-plans/ ; https://community.fly.io/t/free-tier-is-dead/20651 (search summary))_
- **[VERIFIED]** Oracle reclaims Always Free instances that are idle over 7 days: p95 CPU < 20%, network < 20%, memory < 20% (A1 only); A1 Always Free is 1,500 OCPU-h and 9,000 GB-h per month (2 OCPU/12 GB). _(evidence: https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm)_
- **[UNCERTAIN]** GCP free e2-micro: us-west1/us-central1/us-east1 only, 30 GB-months standard disk, 1 GB/month egress from North America, billing account required; whether the external IPv4 charge is covered is unclear. _(evidence: https://docs.cloud.google.com/free/docs/free-cloud-features (IPv4 coverage not stated); https://cloud.google.com/vpc/pricing-announce-external-ips)_
- **[VERIFIED]** GitHub REST: 5,000 requests/hour per user token; secondary limits of 80 content-creating requests/minute and 500/hour, 900 points/minute. _(evidence: https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)_
- **[VERIFIED]** FCM may deprioritize high-priority messages that do not result in user-facing notifications; normal-priority delivery may be delayed in Doze until the device exits it; expedited jobs get a brief exemption after a high-priority message. _(evidence: https://firebase.google.com/docs/cloud-messaging/android-message-priority)_
- **[VERIFIED]** Android 15 limits dataSync foreground services to 6 hours per 24 hours; WorkManager periodic work has a 15-minute minimum interval. _(evidence: https://developer.android.com/develop/background-work/services/fgs/timeout ; https://developer.android.com/reference/androidx/work/PeriodicWorkRequest)_
- **[VERIFIED]** Samsung One UI puts apps not started for 3 days to sleep by default, stopping background tasks; 'Never sleeping apps' is the exemption. _(evidence: https://dontkillmyapp.com/samsung)_
- **[UNCERTAIN]** Cloudflare closes WebSockets with no data in either direction after about 100 s. _(evidence: https://developers.cloudflare.com/network/websockets/ (official page states the behaviour; 100 s figure from secondary sources))_
- **[UNCERTAIN]** workers.dev hostnames leak via passive DNS datasets; CT logs reportedly show only the account-level wildcard. _(evidence: https://dns.ninja/en/dns-lookup/dev/workers ; single anecdote at https://scrapbox.io/Tsut-ps/)_
- **[VERIFIED]** The desktop listener long-polls /api/wait with a 40 s timeout and resets backoff to 2 s after successful waits/fetches (max 30 s); a naive port to a DO would keep it non-hibernating. _(evidence: src/sync.rs:85 (WAIT_TIMEOUT 40 s), src/sync.rs:89 (LISTEN_BACKOFF_MAX 30 s), src/sync.rs:762-791)_
- **[VERIFIED]** Temporary-id remapping is spread over board.rs, sync.rs and main.rs (TEMPORARY_ID_FLOOR = 1<<62), which a leaderless design would replace with globally unique ids. _(evidence: src/board.rs:58, src/sync.rs:74, src/sync.rs:403-434, src/main.rs:165,182)_
- **[VERIFIED]** TaskDeck currently has no reminders/notifications (only comments mention 'alarm'), so background push is needed only for freshness, not alerts. _(evidence: grep -i 'notification|reminder|alarm' over src/: only src/phone.html:152,1933 comments)_
- **[VERIFIED]** Durable Objects Data Studio (beta, Oct 2025) lets SQLite-backed DO storage be viewed and edited in the dashboard (queries billed); no bulk export was found. _(evidence: https://developers.cloudflare.com/changelog/2025-10-16-durable-objects-data-studio (export absence ASSUMED))_

## Recommendations

- Use Cloudflare Workers Free + one SQLite-backed Durable Object per board as the single sequencer and blind relay; write the relay in TypeScript (about 300-500 lines). workers-rs works but is rougher and its Wasm cold-start CPU against the Free 10 ms cap is unmeasured.
- Use only the WebSocket Hibernation API (ctx.acceptWebSocket), client-side protocol pings every 30-45 s, and no timers or alarms in the DO. Do not port /api/wait long-polling to the DO: a non-hibernating object uses 83% of the daily duration quota.
- Authenticate in the Worker (constant-time HMAC of a per-device token) before calling the DO. Serve no HTML and send no CORS headers, use a random Worker name, and make clients send a descriptive User-Agent (Browser Integrity Check gives Error 1010 to some default ones).
- Client reconnects: exponential backoff with full jitter from 1 s to 60 s, reset only after at least 60 s of healthy connection; a per-device daily request budget (e.g. 3,000), then 5-minute polling; recognise quota errors (1027) and show 'sync paused until 00:00 UTC' instead of retrying.
- Relay schema: INTEGER PRIMARY KEY seq, a unique index on the idempotency key, and compaction to the latest snapshot plus a 60-day tail. Store snapshots and images in DO SQLite in chunks of at most 1 MB. Add tests that assert rowsRead per operation. Skip R2 on a EUR 0 setup because it bills overage.
- Android: a native app with a local replica; a WebSocket while in the foreground; sync on open (show the local view instantly); a 30-minute WorkManager periodic job; content-free FCM wake-ups only later and optionally; ask the user to put the app in Samsung's 'Never sleeping apps'.
- Backups: each desktop writes a nightly encrypted snapshot into a folder already synced by OneDrive (Windows) or iCloud Drive (macOS), keeping 30 daily and 12 monthly copies. Keep DO PITR (30 days) as a second line and rehearse a restore quarterly.
- Do not build a hot second sequencer, automatic failover, or a leaderless design now. Document and rehearse a manual re-home (deploy standby, seed from a device snapshot at epoch N+1, re-pair by QR). Consider a 'mailbox' secondary that never sequences (Supabase Free) only if outages are felt.
- Keep the relay protocol specified and versioned, with a second implementation (Rust standalone relay behind a body-buffering reverse proxy on a EUR 1-2/month VPS, or Supabase) that passes the same tests, as the exit plan.
- Optional hardening, recommended if about EUR 10/yr is acceptable: a custom domain on Cloudflare, workers_dev = false, preview URLs disabled, and a WAF custom rule blocking requests without a secret path prefix, so token-less attackers cannot burn the Worker quota.
- Protect the Cloudflare account with a passkey or hardware key and a dedicated email; the relay cannot read data but whoever controls it can withhold or roll back, so clients need fork detection (hash chain) from the crypto design.

## Open questions

- Is the Free per-object DO storage limit 1 GB (FAQ) or 10 GB (table)? It only matters if blobs grow large.
- Does the Free 10 ms CPU limit apply to every DO invocation, and how much CPU does a Rust/Wasm DO cold start use? Measure before choosing workers-rs.
- Do WAF custom rules on a Worker custom domain run before the Worker, so blocked requests do not count toward the 100k/day quota? Are header fields usable in Free custom rules?
- Is DO point-in-time recovery (getBookmarkForTime/onNextSessionRestoreBookmark) available on the Free plan, and from workers-rs?
- Does enabling R2 require a payment method, and is there any way to cap R2 spend? This only matters if R2 is used.
- What is the real end-to-end push latency and wake-from-hibernation time from the user's region? No published figures exist, so measure with a prototype.
- Will the user pay about EUR 10/year for a domain to close the quota-exhaustion attack, and accept Google FCM (content-free wake-ups) for background freshness on Android?
- Is the high DO incident rate seen from August to November 2026 a trend? Re-check cloudflarestatus history before committing.
- Which provider should the cold standby use (EUR 1-2 VPS, Supabase Free, or none), and how often should the re-home procedure be rehearsed?
- Is a mailbox secondary (tentative cross-device propagation during primary outages) worth about 1 week of work to this user, given an expected 9-26 h/year of delayed sync?
