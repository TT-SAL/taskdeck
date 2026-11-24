// The typical day and the import of relay.md §16.2, replayed against the target
// and measured: Worker requests, object requests, rows written and rows read
// must each stay under the family's share of the free plan's daily limits.
// spec: RELAY-ROWS-002
// spec: RELAY-ROWS-003
import assert from 'node:assert/strict';
import test from 'node:test';
import { TARGET, World, decodeMessage, decodeResults, makeTarget } from './lib/harness.mjs';
import { families } from './vectors/build.mjs';
import { DAY, DOOR, F, KEYS, NOW, T, addDevice, admin, fillLog } from './vectors/dsl.mjs';
import { READS } from './vectors/reads.mjs';

const cases = families['relay-day.json'].cases;
const keys = { rec: KEYS.rec, alice: KEYS.alice, bob: KEYS.bob, carol: KEYS.carol };
const board = (name) => ({ keys, boards: { main: { salt: Buffer.from(name.padEnd(16, '.')).toString('hex').slice(0, 32), recovery: 'rec', first: 'alice' } } });
const run = async (world, steps) => { for (const s of steps.flat(Infinity)) await world.step(s, 'day'); };
const small = (cred, id, purpose, seq) => [
  { request: { route: 'blob-begin', board: 'main', cred, body: { $blob_begin: { id, purpose, seq, size: 67 } } }, expect: { status: 200 } },
  { request: { route: 'blob-put', board: 'main', cred, params: { id, part: 0 }, body: { $part: { size: 67, index: 0 } } }, expect: { status: 200 } },
];
const ID = (n) => n.toString(16).padStart(2, '0').repeat(16);

// What one compaction of a day's entries costs, counted on the core in process
// (it needs entries a year old, which only an injected clock gives).
async function compactionOnTheCore(deletes) {
  const world = new World({ ...board('compaction'), clock_ms: NOW }, makeTarget('core'));
  try {
    await world.init();
    const filled = 16384 + deletes + 64, head = 1 + filled; // the Genesis frame and the filled entries
    await run(world, [fillLog('rec', DOOR, 2, filled), small(T('alice'), ID(1), 1, Math.ceil((deletes + 1) / 64) * 64),
      { request: { route: 'append', board: 'main', cred: T('alice'), body: { $batch: [F('alice', 1)] } }, expect: { status: 200 } }, addDevice('bob', head + 2, 'alice', 2)]);
    const b = world.board('main'), before = await world.target.stats(b.hex);
    const steps = admin('compact', 'bob', T('bob'), { below_seq: deletes + 1, snap_entry_seq: head + 1, blob_id: ID(1), keep: [] }, { status: 200, json: { deleted: deletes, floor: deletes + 1, more: false } });
    steps[0] = { clock_ms: NOW + 366 * DAY, ...steps[0] };
    await run(world, steps);
    const after = await world.target.stats(b.hex);
    // The core's counting storage sees only the rows its queries return; the bound of §16.1 stands in for the platform's count.
    return { worker_requests: 2, object_requests: after.calls - before.calls, rows_written: after.rowsWritten - before.rowsWritten, rows_read: READS.nonce(0) + READS.compact(1, deletes, 0) };
  } finally {
    await world.stop();
  }
}

function check(t, c, total) {
  for (const [k, limit] of Object.entries(c.limits)) {
    t.diagnostic(`${k}: ${Math.round(total[k] * 10) / 10} of ${limit} (${(100 * total[k] / limit).toFixed(2)}%)`);
    assert.ok(total[k] < c.share * limit, `${k}: ${total[k]} is not under ${c.share * 100}% of ${limit}`);
  }
}

for (const c of cases) {
  if (!c.targets.includes(TARGET)) continue;
  test(`relay-day: ${c.name}`, async (t) => {
    const world = new World({ ...board(c.script), clock_ms: NOW }, makeTarget());
    try {
      await world.init();
      const b = world.board('main'), n = c.counts;
      let requests = 0;
      const http = async (rq, status = 200) => { requests++; const r = await world.http({ board: 'main', ...rq }); assert.equal(r.status, status, `${rq.route}: ${Buffer.from(r.body).toString().slice(0, 200)}`); return r; };
      const measure = async (fn) => {
        const before = await world.target.stats(b.hex);
        requests = 0;
        await fn();
        const after = await world.target.stats(b.hex);
        return { worker_requests: requests, object_requests: after.calls - before.calls, rows_written: after.rowsWritten - before.rowsWritten, rows_read: after.rowsRead - before.rowsRead };
      };

      if (c.script === 'import') {
        // One device, 16 frames to an append; the append budget splits them over two UTC days.
        let ctr = 1;
        const loginAndAppend = async (gen, frames) => {
          const { nonce } = JSON.parse(Buffer.from((await http({ route: 'login-nonce', cred: DOOR, body: { $dev: 'alice' } })).body).toString());
          const j = JSON.parse(Buffer.from((await http({ route: 'login', cred: DOOR, body: { $login: { dev: 'alice', gen, nonce: { $hex: nonce } } } })).body).toString());
          assert.equal(j.gen, gen);
          for (let done = 0; done < frames; done += n.per_append) {
            const batch = [];
            for (let i = 0; i < Math.min(n.per_append, frames - done); i++) batch.push(F('alice', ctr++));
            const r = await http({ route: 'append', cred: T('alice', gen), body: { $batch: batch } });
            assert.ok(decodeResults(r.body).every((x) => x.status === 'new'));
          }
        };
        const total = await measure(async () => {
          await loginAndAppend(2, n.first_day);
          world.clock = NOW + DAY;
          await world.target.setClock(world, world.clock);
          await loginAndAppend(3, n.frames - n.first_day);
        });
        for (const [k, v] of Object.entries(c.expect)) assert.equal(total[k], v, k);
        return check(t, c, total);
      }

      // The day. Before it: the two other devices, a log long enough for a snapshot, three older snapshots.
      await run(world, [addDevice('bob', 2, 'alice', 1), addDevice('carol', 3, 'alice', 2), fillLog('rec', DOOR, 2, 64), [1, 2, 3].map((i) => small(T('alice'), ID(i), 1, 64))]);
      const next = { alice: 3, bob: 1, carol: 1 };
      let head = 67;
      // One session per device, as one process has: a new connection replaces the last one silently.
      const session = (key) => ({ $hex: (key.length * 17).toString(16).padStart(32, '0') });
      // One connection: upgrade, hello, a catch-up read of the last entries, some appends, close.
      const connect = async (key, appends) => {
        requests++;
        const r = await world.target.openSocket(world, b.hex, `/v1/b/${b.hex}/socket`, await world.credHeader(T(key), 'main'));
        assert.equal(r.status, 101);
        const s = r.socket;
        const next1 = async (type) => { const m = await s.next(5000); assert.ok(m && m.data, `expected ${type}`); const d = decodeMessage(m.data); assert.equal(d.type, type); return d; };
        await s.send(await world.bytes({ $hello: { session: session(key) } }, 'main'));
        await next1('welcome');
        await s.send(await world.bytes({ $sock_read: { req: 1, after: head - n.catch_up_entries, limit: 256 } }, 'main'));
        assert.equal((await next1('page')).page.entries.length, n.catch_up_entries);
        for (let k = 0; k < appends; k++) {
          await s.send(await world.bytes({ $sock_append: { req: 2 + k, batch: [F(key, next[key]++)] } }, 'main'));
          assert.equal((await next1('result')).results[0].status, 'new');
          await next1('push');
          head++;
        }
        s.close();
      };
      const perDesktop = Math.floor((n.socket_appends - 3) / n.desktop_connections);
      let left = n.socket_appends;
      const day = await measure(async () => {
        for (let i = 0; i < n.desktop_connections; i++) { const k = i === 0 || i === 1 ? perDesktop + 1 : perDesktop; await connect(i % 2 ? 'bob' : 'alice', k); left -= k; }
        for (let i = 0; i < n.phone_opens; i++) { const k = left > 0 ? 1 : 0; await connect('carol', k); left -= k; }
        assert.equal(left, 0, 'every socket append of the day was sent');
        for (let i = 0; i < n.background_syncs; i++) {
          await http({ route: 'hello', cred: T('carol') });
          await http({ route: 'read', cred: T('carol'), query: `after=${head}` });
        }
        for (let i = 0; i < n.background_appends; i++) { await http({ route: 'append', cred: T('carol'), body: { $batch: [F('carol', next.carol++)] } }); head++; }
        // The snapshot of the day: begin, its parts, and an older one deleted on completion.
        const size = n.snapshot_parts * 1048576;
        await http({ route: 'blob-begin', cred: T('alice'), body: { $blob_begin: { id: ID(9), purpose: 1, seq: 64, size } } });
        for (let i = 0; i < n.snapshot_parts; i++) await http({ route: 'blob-put', cred: T('alice'), params: { id: ID(9), part: i }, body: { $part: { size, index: i } } });
        for (let i = 0; i < n.caps_and_retries; i++) await http(i % 2 ? { route: 'caps', cred: T('carol') } : { route: 'hello', cred: T('carol') });
      });
      const compaction = await compactionOnTheCore(n.compaction_deletes);
      t.diagnostic(`without the compaction: ${JSON.stringify(day)}; the compaction: ${JSON.stringify(compaction)}`);
      const total = Object.fromEntries(Object.keys(day).map((k) => [k, day[k] + compaction[k]]));
      return check(t, c, total);
    } finally {
      await world.stop();
    }
  });
}
