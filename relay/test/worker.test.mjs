// The shipped worker.js, loaded in this process with a stand-in for the platform's
// `cloudflare:workers` module: what the Worker hands to the object (relay.md §6.4)
// and how often the secrets are decoded (§3.3). The Worker target runs the same file
// under `wrangler dev`; these cases need to see inside it. The same on every target.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { register } from 'node:module';
import test from 'node:test';
import * as core from '../relay-core.js';
import { STD, hex, unhex } from './lib/harness.mjs';

const data = (text) => `data:text/javascript,${encodeURIComponent(text)}`;
const platform = data('export class DurableObject { constructor(ctx, env) { this.ctx = ctx; this.env = env; } }');
register(data(`export async function resolve(specifier, context, next) {
  if (specifier === 'cloudflare:workers') return { url: ${JSON.stringify(platform)}, shortCircuit: true };
  return next(specifier, context);
}`));
globalThis.WebSocketRequestResponsePair = class { constructor(request, response) { this.request = request; this.response = response; } };
const { default: worker, Board } = await import('../worker.js');

const board = new Uint8Array(16).fill(0x42), dev = new Uint8Array(16).fill(0x80);
const ctx = () => ({
  storage: { sql: { exec: () => ({ toArray: () => [], rowsWritten: 0 }) }, transactionSync: (fn) => fn() },
  blockConcurrencyWhile(fn) { this.blocked = fn(); },
  setWebSocketAutoResponse(pair) { this.auto = pair; },
  acceptWebSocket() {},
  getWebSockets: () => [],
});
// An env whose BOARD binding records the one call the Worker may make.
function envOf(more = {}) {
  const calls = [];
  const BOARD = { idFromName: (name) => ({ name }), get: (id) => ({ fetch: async (url, init) => { calls.push({ id, url, init }); return new Response('{"ok":true}', { status: 200, headers: { 'X-TD-Relay': 'object' } }); } }) };
  return { env: { TOKEN_KEY: STD.token_key, CREATE_SECRET: STD.create_secret, RELAY_ORIGIN: STD.origin, BOARD, ...more }, calls };
}
const url = (path) => `https://relay.test/v1/b/${hex(board)}/${path}`;

// spec: RELAY-GEN-006
test('the Worker and every board object of an isolate share one decoded configuration', async () => {
  const { env } = envOf();
  const a = new Board(ctx(), env), b = new Board(ctx(), env);
  assert.equal(a.core.config, b.core.config, 'two objects of one isolate hold the same Config');
  assert.equal(a.core.config.tokenKey, b.core.config.tokenKey, 'and so the same decoded token key');
  assert.ok(Object.isFrozen(a.core.config) && a.core.config.push === true);
  // The Worker's own gate reads the same one: makeConfig is called in one place of worker.js.
  const source = fs.readFileSync(new URL('../worker.js', import.meta.url), 'utf8');
  assert.equal((source.match(/makeConfig\(/g) || []).length, 1);
  assert.match(source, /config: configOf\(env\)/);
  // Another configuration (another isolate, in production) has its own.
  const other = new Board(ctx(), envOf({ TOKEN_KEY: '11'.repeat(32) }).env);
  assert.notEqual(other.core.config, a.core.config);
  assert.notDeepEqual(other.core.config.tokenKey, a.core.config.tokenKey);
});

// spec: RELAY-GATE-010
// spec: RELAY-GATE-004
// spec: RELAY-SHAPE-006
test('the Worker passes to the object only what §6.4 lists, and nothing a client sent in X-TD- headers', async () => {
  const { env, calls } = envOf();
  const token = core.makeToken(unhex(STD.token_key), 1, board, dev, 3);
  const spoof = { 'X-TD-Cred': 'create', 'X-TD-Dev': '00'.repeat(16), 'X-TD-Role': '9', 'X-TD-Gen': '99', 'X-TD-Relay': 'gate', Cookie: 'a=b', 'X-Other': 'x' };
  // The internal request: its route, its parameters (in any order), its method, headers and body.
  const sent = (i) => { const u = new URL(calls[i].url); return { id: calls[i].id.name, url: `${u.origin}${u.pathname}?${[...u.searchParams].sort().map((p) => p.join('=')).join('&')}`, method: calls[i].init.method, headers: [...calls[i].init.headers].sort(), body: calls[i].init.body }; };

  let res = await worker.fetch(new Request(url('log?limit=7&after=5'), { headers: { Authorization: `Bearer ${token}`, ...spoof } }), env);
  assert.equal(res.status, 200);
  assert.deepEqual(sent(0), { id: hex(board), url: `https://object/read?after=5&board=${hex(board)}&limit=7`, method: 'GET',
    headers: [['x-td-cred', 'token'], ['x-td-dev', hex(dev)], ['x-td-gen', '3'], ['x-td-role', '1']], body: undefined });

  const body = new Uint8Array(16).fill(7);
  const door = hex(core.door(unhex(STD.token_key), board));
  res = await worker.fetch(new Request(url('login/nonce'), { method: 'POST', body, headers: { Authorization: `Door ${door}`, 'Content-Length': '16', ...spoof } }), env);
  assert.equal(res.status, 200);
  assert.deepEqual({ ...sent(1), body: hex(sent(1).body) }, { id: hex(board), url: `https://object/login-nonce?board=${hex(board)}`, method: 'POST', headers: [['x-td-cred', 'door']], body: hex(body) });

  res = await worker.fetch(new Request(url('socket'), { headers: { Authorization: `Bearer ${token}`, Upgrade: 'websocket', ...spoof } }), env);
  assert.deepEqual(sent(2).headers, [['upgrade', 'websocket'], ['x-td-cred', 'token'], ['x-td-dev', hex(dev)], ['x-td-gen', '3'], ['x-td-role', '1']]);

  // Refused by the gate: no call at all, and the gate's own headers on the answer.
  const before = calls.length;
  for (const [request, status, code] of [
    [new Request(url('hello'), { headers: spoof }), 401, 'relay/unauthorized'],
    [new Request(url('hello'), { headers: { Authorization: `Bearer ${token.slice(0, -1)}A` } }), 401, 'relay/unauthorized'],
    [new Request(url('nope'), { headers: { Authorization: `Bearer ${token}` } }), 404, 'relay/not_found'],
    [new Request(url('login/nonce'), { method: 'POST', body, headers: { Authorization: `Door ${door}`, 'Content-Length': '17' } }), 413, 'relay/too_large'],
    [new Request(url('login'), { method: 'POST', body: new Uint8Array(100), headers: { Authorization: `Door ${door}`, 'Content-Length': '148' } }), 400, 'relay/bad_body'],
  ]) {
    res = await worker.fetch(request, env);
    assert.equal(res.status, status);
    assert.deepEqual(await res.json(), { error: code });
    assert.equal(res.headers.get('X-TD-Relay'), 'gate');
    assert.equal(res.headers.get('Cache-Control'), 'no-store');
  }
  assert.equal(calls.length, before, 'the object was not called for a request the gate refused');

  // An object call that throws is the front's 503; an invalid configuration answers before anything else.
  const broken = envOf();
  broken.env.BOARD.get = () => ({ fetch: async () => { throw new Error('the object is over its quota'); } });
  res = await worker.fetch(new Request(url('hello'), { headers: { Authorization: `Bearer ${token}` } }), broken.env);
  assert.deepEqual([res.status, await res.json()], [503, { error: 'relay/object_unavailable' }]);
  const bad = envOf({ RELAY_ORIGIN: 'http://relay.test' });
  res = await worker.fetch(new Request(url('caps'), { headers: { Authorization: `Door ${door}` } }), bad.env);
  assert.deepEqual([res.status, await res.json(), bad.calls.length], [503, { error: 'relay/misconfigured' }, 0]);
});

// spec: RELAY-STORE-002
// spec: RELAY-SOCK-010
test('a new board object runs the schema while it blocks other events, and registers the ping answer', async () => {
  const c = ctx(), statements = [];
  c.storage.sql.exec = (q) => { statements.push(q); return { toArray: () => [], rowsWritten: 0 }; };
  const b = new Board(c, envOf().env);
  await c.blocked;
  assert.deepEqual(statements, [...core.SQL.schema]);
  assert.deepEqual([c.auto.request, c.auto.response], ['ping', 'pong']);
  assert.equal(typeof b.webSocketMessage, 'function');
  assert.equal(b.alarm, undefined);
});
