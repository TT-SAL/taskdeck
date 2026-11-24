// What is checked by reading the relay's files rather than by calling it
// (relay.md §3.2, §3.3, §7.1, §18 item 7), and a few properties of the core
// that no request shows. The same on every target.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import { DatabaseSync } from 'node:sqlite';
import test from 'node:test';
import * as core from '../relay-core.js';
import { countingStorage } from './lib/harness.mjs';

const read = (p) => fs.readFileSync(new URL(`../${p}`, import.meta.url), 'utf8');
const SOURCES = { 'relay-core.js': read('relay-core.js'), 'worker.js': read('worker.js'), 'standby.js': read('standby.js') };
const imports = (text) => [...text.matchAll(/^\s*import\s[^'"]*['"]([^'"]+)['"]/gm)].map((m) => m[1]).sort();
// The code without its comments, so that a word in a comment is not a finding.
const code = (text) => text.replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:'"`\\])\/\/.*$/gm, '$1');

// spec: RELAY-SHAPE-002
test('relay/package.json: no runtime dependency, wrangler pinned as the only devDependency', () => {
  const pkg = JSON.parse(read('package.json'));
  for (const k of ['dependencies', 'optionalDependencies', 'peerDependencies', 'bundledDependencies', 'bundleDependencies']) assert.equal(pkg[k], undefined, k);
  assert.deepEqual(Object.keys(pkg.devDependencies), ['wrangler']);
  assert.match(pkg.devDependencies.wrangler, /^[0-9]+\.[0-9]+\.[0-9]+$/, 'an exact version');
  const pinned = JSON.parse(fs.readFileSync(new URL('../../scripts/wrangler/package.json', import.meta.url), 'utf8')).dependencies.wrangler;
  assert.equal(pkg.devDependencies.wrangler, pinned, 'the version scripts/dev-setup.sh installs');
  assert.equal(pkg.type, 'module');
});

// spec: RELAY-SHAPE-003
// spec: RELAY-GEN-001
test('the relay code imports only what §3.2 allows, and computes with the platform\'s primitives', () => {
  assert.deepEqual(imports(SOURCES['relay-core.js']), ['node:crypto']);
  assert.match(SOURCES['relay-core.js'], /import \{ createHash, createHmac, timingSafeEqual \} from 'node:crypto';/);
  assert.deepEqual(imports(SOURCES['worker.js']), ['./relay-core.js', 'cloudflare:workers']);
  assert.deepEqual(imports(SOURCES['standby.js']), ['./relay-core.js', 'node:fs', 'node:http', 'node:https', 'node:path', 'node:sqlite']);
  const c = code(SOURCES['relay-core.js']);
  assert.match(c, /createHash\('sha256'\)/);
  assert.match(c, /createHmac\('sha256', key\)/);
  assert.match(c, /crypto\.subtle\.verify\(\{ name: 'Ed25519' \}/);
  assert.match(c, /crypto\.getRandomValues\(/);
  for (const [name, text] of Object.entries(SOURCES)) {
    assert.doesNotMatch(code(text), /Math\.random|createSign|createVerify|generateKey|require\(/, name);
  }
});

// spec: RELAY-SHAPE-005
// spec: RELAY-SHAPE-006
// spec: RELAY-SHAPE-007
// spec: RELAY-SHAPE-008
// spec: RELAY-GEN-007
test('no log, no timer, no alarm, no eval, no outbound fetch, hibernating sockets only', () => {
  for (const [name, text] of Object.entries(SOURCES)) {
    const c = code(text);
    for (const word of ['console.', 'eval(', 'new Function', 'setTimeout', 'setInterval', 'setAlarm', 'WebAssembly', 'import(']) assert.ok(!c.includes(word), `${name} contains ${word}`);
    assert.doesNotMatch(c, /\balarm\s*\(/, `${name} defines an alarm handler`);
  }
  const w = code(SOURCES['worker.js']);
  // Every `fetch(` in worker.js is a handler's definition or the call to the board's stub.
  const fetches = [...w.matchAll(/(\S*)fetch\(/g)].map((m) => m[1]);
  assert.deepEqual(fetches.sort(), ['', '', 'stub.'].sort());
  assert.match(w, /ctx\.acceptWebSocket\(/);
  assert.doesNotMatch(w, /\.accept\(\)/);
  assert.match(w, /ctx\.blockConcurrencyWhile\(async \(\) => this\.core\.migrate\(\)\)/);
  assert.match(w, /setWebSocketAutoResponse\(new WebSocketRequestResponsePair\('ping', 'pong'\)\)/);
  const toml = read('wrangler.toml');
  assert.match(toml, /\[observability\]\s*\nenabled = false/);
  assert.match(toml, /compatibility_flags = \["nodejs_compat"\]/);
  assert.match(toml, /new_sqlite_classes = \["Board"\]/);
  assert.match(toml, /preview_urls = false/);
  assert.doesNotMatch(toml, /^\s*(TOKEN_KEY|CREATE_SECRET|RELAY_ORIGIN)\s*=/m, 'no secret and no relay address in the repository');
});

test('nothing that wrangler writes while it runs is tracked, and the suite does not let it ask Cloudflare about this machine', () => {
  const root = new URL('../../', import.meta.url);
  const ls = spawnSync('git', ['ls-files', '--', 'relay'], { cwd: root, encoding: 'utf8' });
  assert.equal(ls.status, 0, 'the suite runs in a checkout');
  const tracked = ls.stdout.split('\n').filter(Boolean);
  assert.ok(tracked.includes('relay/relay-core.js'));
  assert.deepEqual(tracked.filter((f) => /(^|\/)\.wrangler(\/|$)|node_modules|\.sqlite|\.log$/.test(f)), []);
  assert.match(read('.gitignore'), /^\.wrangler\/$/m);
  const run = read('test/run.sh');
  assert.match(run, /CLOUDFLARE_CF_FETCH_ENABLED=false/);
  assert.match(run, /WRANGLER_SEND_METRICS=false/);
  assert.match(run, /cd "\$work\/relay"/, 'wrangler runs on a copy outside the repository');
});

// spec: RELAY-SHAPE-001
test('worker.js and standby.js decide nothing: no error code but the object call\'s, no SQL, no status of their own', () => {
  for (const name of ['worker.js', 'standby.js']) {
    const c = code(SOURCES[name]);
    assert.deepEqual([...new Set([...c.matchAll(/'relay\/[a-z_]+'/g)].map((m) => m[0]))], ["'relay/object_unavailable'"], name);
    assert.doesNotMatch(c, /\b(SELECT|INSERT|UPDATE|DELETE)\b/, name);
  }
  assert.doesNotMatch(code(SOURCES['worker.js']), /status: (?!101\b|reply\.status)[0-9a-z.]+/);
  assert.doesNotMatch(code(SOURCES['standby.js']), /writeHead\((?!reply\.status)/);
});

// spec: RELAY-GEN-002
// spec: RELAY-GEN-006
// spec: SEAL-GEN-007
test('secrets are compared in constant time, decoded once, and their copies wiped', () => {
  const c = code(SOURCES['relay-core.js']);
  assert.match(c, /export function ctEqual\(a, b\) \{\s*return a\.length === b\.length && timingSafeEqual\(a, b\);\s*\}/);
  assert.doesNotMatch(c, /\.equals\(|Buffer\./);
  assert.equal((c.match(/\.fill\(0\)/g) || []).length, 2, 'the create secret of a request and the ticket key are wiped');
  assert.equal(core.ctEqual(Uint8Array.of(1, 2), Uint8Array.of(1, 2)), true);
  assert.equal(core.ctEqual(Uint8Array.of(1, 2), Uint8Array.of(1, 3)), false);
  assert.equal(core.ctEqual(Uint8Array.of(1, 2), Uint8Array.of(1, 2, 3)), false);
  const key = '11'.repeat(32);
  const config = core.makeConfig({ tokenKey: key, createSecret: key, origin: 'https://relay.test', push: true });
  assert.ok(Object.isFrozen(config) && config.tokenKey instanceof Uint8Array && config.createSecret instanceof Uint8Array);
  assert.match(code(SOURCES['worker.js']), /const configs = new WeakMap\(\)/);
  // The same key gives the same door for the life of a deployment; another key, another door (RELAY-DOOR-004).
  const board = new Uint8Array(16).fill(7);
  assert.deepEqual(core.door(config.tokenKey, board), core.door(config.tokenKey, board));
  assert.notDeepEqual(core.door(config.tokenKey, board), core.door(new Uint8Array(32).fill(2), board));
});

// spec: RELAY-SHAPE-009
// spec: RELAY-SHAPE-010
test('the route table is the one of §10.1: a deploy may add to it, never change it', () => {
  const table = core.ROUTES.map((r) => `${r.name} ${r.method} ${r.pattern} ${r.creds.join(',')} ${r.cap} ${r.fixed} ${r.object}`);
  assert.deepEqual(table, [
    'caps GET /v1/b/{board}/caps T1,T2,D,C 0 0 false',
    'door GET /v1/b/{board}/door C 0 0 false',
    'create POST /v1/b/{board}/create C 262244 null true',
    'login-nonce POST /v1/b/{board}/login/nonce T1,D 16 16 true',
    'login POST /v1/b/{board}/login T1,D 148 148 true',
    'hello GET /v1/b/{board}/hello T1,T2 0 0 true',
    'socket GET /v1/b/{board}/socket T1 0 0 true',
    'read GET /v1/b/{board}/log T1,T2 0 0 true',
    'append POST /v1/b/{board}/append T1,D 4194373 null true',
    'blob-list GET /v1/b/{board}/blobs T1,T2 0 0 true',
    'blob-begin POST /v1/b/{board}/blobs T1 65 65 true',
    'blob-meta GET /v1/b/{board}/blobs/{id} T1,T2 0 0 true',
    'blob-put PUT /v1/b/{board}/blobs/{id}/{part} T1 1048576 null true',
    'blob-get GET /v1/b/{board}/blobs/{id}/{part} T1,T2 0 0 true',
    'pair-open POST /v1/b/{board}/pair T1 100 100 true',
    'slot-post POST /v1/b/{board}/pair/{tid}/{label} T1,D 16076 null true',
    'slot-get GET /v1/b/{board}/pair/{tid}/{label} T1,D 0 0 true',
    'pair-close DELETE /v1/b/{board}/pair/{tid} T1 0 0 true',
    'admin-nonce POST /v1/b/{board}/admin/nonce T1,D,C 16 16 true',
    'admin POST /v1/b/{board}/admin/{op} T1,D,C 1051044 null true',
    'seed-tail POST /v1/b/{board}/seed/tail T1 1049610 null true',
  ]);
  for (const r of core.ROUTES) assert.ok(r.pattern.startsWith('/v1/'), r.name);
  // The admin operations: credentials, cap and fixed length of §12.1, as the gate applies them.
  const board = 'ab'.repeat(16);
  const ops = { register: [262344, null, 'T1'], revoke: [136, 136, 'T1,D'], reinstate: [24718, null, 'T1,D'], fence: [262529, null, 'T1,D'], seed: [18782, null, 'C'], compact: [1048732, null, 'T1'],
    restore_tail: [1051044, null, 'T1,D'], purge: [132, 132, 'T1'], mint_read_token: [140, 140, 'T1,D'], revoke_read_token: [136, 136, 'T1,D'], recovery_register: [208, 208, 'T1,D'] };
  for (const [op, [cap, fixed, creds]] of Object.entries(ops)) {
    const { route } = core.matchRoute('POST', `/v1/b/${board}/admin/${op}`, null);
    assert.deepEqual([route.cap, route.fixed, route.creds.join(',')], [cap, fixed, creds], op);
  }
  assert.equal(core.PROTOCOL, 1);
  assert.ok(Object.isFrozen(core.LIMITS) && Object.isFrozen(core.ROUTES) && Object.isFrozen(core.SQL));
  assert.deepEqual([core.LIMITS.maxBatch, core.LIMITS.maxFrame, core.LIMITS.appendBudget, core.LIMITS.deleteBudget, core.LIMITS.compactWindow], [16, 262144, 3000, 5000, 16384]);
});

// spec: RELAY-ENC-006
test('every code of the error catalogue has the status §19 gives it', () => {
  // The catalogue is read from the spec itself: `relay/<code>` and the status in its second column.
  const spec = fs.readFileSync(new URL('../../docs/sync/spec/relay.md', import.meta.url), 'utf8');
  const table = spec.slice(spec.indexOf('## 19. Error catalogue'), spec.indexOf('## 20. Limits'));
  const want = {};
  for (const m of table.matchAll(/^\| `(relay\/[a-z_]+)` \| [^|]*?\((\d{3})[;),]/gm)) want[m[1]] = Number(m[2]);
  assert.equal(Object.keys(want).length, 65, 'the codes of §19 that carry a status');
  const got = { ...core.ERRORS };
  assert.equal(got['relay/address'], 400); // client only: §19 gives it no status
  delete got['relay/address'];
  assert.deepEqual(got, want);
  for (const [code_, status] of Object.entries(core.ERRORS)) {
    const reply = core.refusal(code_);
    assert.equal(reply.status, status);
    assert.equal(Buffer.from(reply.body).toString(), JSON.stringify({ error: code_ }));
    assert.deepEqual(reply.headers, { 'Cache-Control': 'no-store', 'X-TD-Relay': 'gate', 'Content-Type': 'application/json' });
  }
  const e = new core.RelayError(409, 'relay/not_next', { head: { seq: 1, chain: 'aa' } });
  assert.equal(e.message, 'relay/not_next'); // nothing but the code (RELAY-GEN-005)
  assert.equal(Buffer.from(core.errorReply(e, 'object').body).toString(), '{"error":"relay/not_next","head":{"seq":1,"chain":"aa"}}');
});

// spec: RELAY-STORE-001
// spec: RELAY-STORE-002
test('the schema is exactly the tables, columns, keys and index of §7.1, and running it again writes nothing', () => {
  const db = new DatabaseSync(':memory:'), stats = { rowsRead: 0, rowsWritten: 0 };
  const c = new core.BoardCore({ storage: countingStorage(core.sqliteStorage(db), stats), config: null, board: null });
  c.migrate();
  const schemaVersion = db.prepare('PRAGMA schema_version').get().schema_version;
  c.migrate();
  assert.equal(db.prepare('PRAGMA schema_version').get().schema_version, schemaVersion, 'the second run changed nothing');
  assert.equal(stats.rowsWritten, 0);
  const objects = db.prepare("SELECT type, name, tbl_name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name").all().map((r) => `${r.type} ${r.name}`);
  assert.deepEqual(objects, ['index log_dev_ctr', 'table blobs', 'table board', 'table devices', 'table log', 'table nonces', 'table parts', 'table read_tokens', 'table tickets']);
  const cols = (t) => db.prepare(`PRAGMA table_info(${t})`).all().map((r) => `${r.name}:${r.type}${r.notnull ? '!' : ''}${r.pk ? '*' : ''}`).join(' ');
  assert.equal(cols('board'), 'one:INTEGER* schema:INTEGER! board_id:BLOB! state:INTEGER! created_at:INTEGER! base_seq:INTEGER! base_chain:BLOB! compact_floor:INTEGER! purge_floor:INTEGER! purge_done:INTEGER! compact_snap:BLOB del_day:INTEGER! del_count:INTEGER! read_epoch:INTEGER! home_gen:INTEGER! fence_host:TEXT fence_door:BLOB seed_signer:BLOB seed_last_seq:INTEGER seed_chain:BLOB');
  assert.equal(cols('devices'), 'dev:BLOB!* sign_pk:BLOB! role:INTEGER! state:INTEGER! gen:INTEGER! last_ctr:INTEGER! last_seq:INTEGER! app_day:INTEGER! app_count:INTEGER! added_at:INTEGER! wraps:BLOB');
  assert.equal(cols('log'), 'seq:INTEGER!* dev:BLOB! ctr:INTEGER! hash:BLOB! chain:BLOB! at:INTEGER! frame:BLOB!');
  assert.equal(cols('blobs'), 'id:BLOB!* slot:INTEGER! purpose:INTEGER! seq:INTEGER! author:BLOB! size:INTEGER! parts:INTEGER! sha256:BLOB! received:INTEGER! complete:INTEGER! at:INTEGER!');
  assert.equal(cols('parts'), 'pk:INTEGER!* data:BLOB!');
  assert.equal(cols('tickets'), 'tid:BLOB!* creator:BLOB! expires:INTEGER! tries:INTEGER! a:BLOB! b:BLOB c:BLOB d:BLOB');
  assert.equal(cols('nonces'), 'nonce:BLOB!* purpose:INTEGER! dev:BLOB! at:INTEGER!');
  assert.equal(cols('read_tokens'), 'dev:BLOB!* gen:INTEGER! epoch:INTEGER! state:INTEGER!');
  const idx = db.prepare('PRAGMA index_list(log)').all();
  assert.deepEqual(idx.map((i) => [i.name, i.unique]), [['log_dev_ctr', 1]]);
  assert.deepEqual(db.prepare('PRAGMA index_info(log_dev_ctr)').all().map((i) => i.name), ['dev', 'ctr']);
  for (const t of ['devices', 'blobs', 'tickets', 'nonces', 'read_tokens']) assert.match(db.prepare('SELECT sql FROM sqlite_master WHERE name = ?').get(t).sql, /WITHOUT ROWID/, t);
  assert.throws(() => db.exec("INSERT INTO board (one, schema, board_id, state, created_at, base_seq, base_chain, compact_floor, purge_floor, purge_done, del_day, del_count, read_epoch, home_gen) VALUES (2, 1, x'00', 1, 0, 0, x'00', 1, 1, 1, 0, 0, 1, 1)"), /CHECK/);
  db.close();
});

// spec: RELAY-OBJ-005
test('the relay clock is read once for each request and each socket message', async () => {
  const db = new DatabaseSync(':memory:');
  let reads = 0;
  const config = core.makeConfig({ tokenKey: '11'.repeat(32), createSecret: '22'.repeat(32), origin: 'https://relay.test', push: true });
  const sent = [];
  const sockets = { list: () => [1], attachment: () => ({ dev: '00'.repeat(16), gen: 1, ready: true, session: 'aa' }), setAttachment() {}, send: (s, b) => sent.push(b), close() {}, accept() {} };
  const board = new Uint8Array(16).fill(9);
  const c = new core.BoardCore({ storage: core.sqliteStorage(db), sockets, now: () => { reads++; return 1795000000000; }, config, board });
  c.migrate();
  const params = { board: core.hex(board) };
  for (const [route, cred, body] of [['hello', { cred: 'token', role: 1, dev: new Uint8Array(16), gen: 1 }, new Uint8Array(0)], ['login-nonce', { cred: 'door' }, new Uint8Array(16)], ['create', { cred: 'create' }, new Uint8Array(800)], ['append', { cred: 'door' }, new Uint8Array(9)]]) {
    reads = 0;
    const reply = await c.handle({ route, params, query: {}, cred, body });
    assert.ok(reply.status >= 400, route);
    assert.equal(reads, 1, route);
  }
  reads = 0;
  await c.onMessage(1, Uint8Array.of(3, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5));
  assert.equal(reads, 1, 'a socket message');
  assert.equal(sent.length, 1);
  db.close();
});

// spec: RELAY-GATE-006
// spec: RELAY-GATE-007
test('the body check of the fronts: complete, short, or late', () => {
  assert.equal(core.checkBody(3, Uint8Array.of(1, 2, 3)), null);
  assert.equal(core.checkBody(0, new Uint8Array(0)), null);
  for (const [length, got] of [[3, Uint8Array.of(1, 2)], [3, Uint8Array.of(1, 2, 3, 4)], [1, new Uint8Array(0)]]) {
    const reply = core.checkBody(length, got);
    assert.equal(reply.status, 400);
    assert.equal(Buffer.from(reply.body).toString(), '{"error":"relay/bad_body"}');
    assert.equal(reply.headers['X-TD-Relay'], 'gate');
  }
  const late = core.checkBody(1000, null);
  assert.equal(late.status, 408);
  assert.equal(Buffer.from(late.body).toString(), '{"error":"relay/slow"}');
  for (const name of ['worker.js', 'standby.js']) assert.match(code(SOURCES[name]), /checkBody\(g\.contentLength, body\)/, `${name} checks the body it read`);
});

// spec: RELAY-GEN-003
// spec: RELAY-ENC-003
test('no length a request states is believed before the bytes are there', async () => {
  const config = core.makeConfig({ tokenKey: '11'.repeat(32), createSecret: '22'.repeat(32), origin: 'https://relay.test', push: true });
  const db = new DatabaseSync(':memory:'), board = new Uint8Array(16).fill(9);
  const c = new core.BoardCore({ storage: core.sqliteStorage(db), now: () => 1, config, board });
  c.migrate();
  const params = { board: core.hex(board), op: 'compact' };
  const call = async (route, cred, body) => JSON.parse(Buffer.from((await c.handle({ route, params, query: {}, cred, body })).body).toString()).error;
  // A batch that announces 16 frames of 4 GiB in nine bytes, and an envelope whose lengths run past its end.
  assert.equal(await call('create', { cred: 'create' }, Uint8Array.from([...new Uint8Array(96), 0xff, 0xff, 0xff, 0xff])), 'relay/bad_body');
  const env = new Uint8Array(120);
  env.set([0xff, 0xff, 0xff, 0xff], 112);
  assert.equal(await call('admin', { cred: 'door' }, env), 'relay/bad_body');
  // A compact request that announces 2^32 - 1 keep seqs in an empty list: refused by its count, nothing allocated.
  const args = new Uint8Array(36);
  args.set([0xff, 0xff, 0xff, 0xff], 32);
  const body = new Uint8Array(120 + 36);
  body.set([0, 0, 0, 36], 112);
  body.set(args, 116);
  assert.equal(await call('admin', { cred: 'door' }, body), 'relay/bad_args');
  db.close();
});
