// Families: gate, token, address, create, login, append, read (relay.md §21).
import { createHash, createHmac } from 'node:crypto';
import { ALL, CORE, CREATE, DOOR, DUP, F, KEYS, LOCAL, N, NOW, REF, RT, T, addDevice, admin, append, at, err, fn, gateErr, kase, login, range, req, step } from './dsl.mjs';

const TK = '7461736b6465636b2d72656c61792d746573742d6b65792d30303030303030a1';
const big = (author, ctr) => F(author, ctr, { body_len: 262032 }); // a frame of exactly 262,144 bytes
const raw = (method, path, cred, extra = {}) => ({ board: 'main', raw: { method, path }, cred, ...extra });

// ---------- relay-gate.json (§5, §6)
const g = (name, clauses, steps, o) => kase('gate', name, clauses, steps, o);
export const gate = [
  g('a path or method outside the route table', ['RELAY-ENC-001', 'RELAY-GATE-003', 'RELAY-GATE-004', 'RELAY-SHAPE-009', 'RELAY-TEST-002'], [
    ['GET /v1/b/{board}/nope', 'POST /v1/b/{board}/caps', 'GET /v1/b/{board}/caps/', 'GET /v2/b/{board}/caps', 'GET /v1/b/0123/caps',
      'GET /v1/b/0123456789ABCDEF0123456789ABCDEF/caps', 'POST /v1/b/{board}/pair/00112233445566778899aabbccddeeff/a', 'POST /v1/b/{board}/admin/unfence',
      'POST /v1/b/{board}/admin/constructor', 'GET /v1/b/{board}/blobs/00112233445566778899aabbccddeeff/64', 'GET /v1/b/{board}/blobs/00112233445566778899aabbccddeeff/01',
      'GET /v1/b/{board}/pair/00112233445566778899aabbccddeeff/e', 'GET /', 'GET /v1/b/{board}', 'DELETE /v1/b/{board}/log', 'PUT /v1/b/{board}/blobs',
    ].map((s) => step(raw(...s.split(' '), T('alice')), gateErr(404, 'relay/not_found'))),
  ]),
  g('a path that is not written as the route table writes it', ['RELAY-ENC-001', 'RELAY-GATE-004'], [
    ['/v1/b/{board}/x/../caps', '/v1/b/{board}/./caps', '/v1/b/{board}/%2e/caps', '/v1/x/../b/{board}/caps', '/v1\\b\\{board}\\caps', '/v1/b/{board}/caps#frag',
      '/v1/b/{board}/c%61ps', '/v1//b/{board}/caps', '/v1/b/{board}/caps%3Fx=1', '/v1/b/{board}/caps;x', '/V1/b/{board}/caps', '/v1/b/{board}/Caps',
    ].map((path) => step(raw('GET', path, DOOR), gateErr(404, 'relay/not_found'))),
    step(req('caps', DOOR), { status: 200, x_td_relay: 'gate' }),
  ], { targets: LOCAL }),
  // The platform rewrites the request target before the Worker's code runs (relay.md §4.1, §15.2):
  // the Worker matches the path it receives, and checks the credential against that path's board.
  g('what the platform makes of a path before the Worker sees it', ['RELAY-ENC-001', 'RELAY-SHAPE-011'], [
    step(raw('GET', '/v1/b/{board}/x/../caps', DOOR), { status: 200, x_td_relay: 'gate', object_called: false }),
    step(raw('GET', '/v1/b/{board}/./caps', DOOR), { status: 200, x_td_relay: 'gate' }),
    step(raw('GET', '/v1\\b\\{board}\\caps', DOOR), { status: 200, x_td_relay: 'gate' }),
    step(raw('GET', '/v1/b/{board}/hello#frag', T('alice')), { status: 200, x_td_relay: 'object' }),
    step(raw('GET', '/v1/b/{board}/x/../caps', {}), gateErr(401, 'relay/unauthorized')),
    step(raw('GET', '/v1/b/{board}/x/../caps', { door: 'other' }), gateErr(401, 'relay/unauthorized')),
    step(raw('GET', '/v1/b/{board}/x/../hello', DOOR), gateErr(401, 'relay/unauthorized')),
    // What it leaves alone is refused as on the standby.
    ['/v1/b/{board}/c%61ps', '/v1//b/{board}/caps', '/v1/b/{board}/caps%3Fx=1', '/v1/b/{board}/caps;x', '/V1/b/{board}/caps', '/v1/b/{board}/caps/'].map((path) => step(raw('GET', path, DOOR), gateErr(404, 'relay/not_found'))),
  ], { targets: ['worker'], boards: { main: {}, other: { create: false } } }),
  g('a query that is not allowed', ['RELAY-ENC-002', 'RELAY-GATE-003'], [
    step(raw('GET', '/v1/b/{board}/caps?x=1', T('alice')), gateErr(400, 'relay/bad_query')),
    step(raw('GET', '/v1/b/{board}/hello?', T('alice')), gateErr(400, 'relay/bad_query')),
    ['', 'limit=5', 'after=1&after=2', 'after=1&limit=0', 'after=1&limit=257', 'after=01', 'after=', 'after=1&x=2', 'after=1&limit=2&limit=2',
      'after=99999999999999999', 'after=9007199254740992', 'after=-1', 'after=1&', 'after=1;limit=2', 'after=0x10', 'AFTER=1',
    ].map((q) => step(req('read', T('alice'), { query: q }), gateErr(400, 'relay/bad_query'))),
    step(req('read', T('alice'), { query: 'limit=5&after=0' }), { status: 200, x_td_relay: 'object', page: { seqs: [1] } }),
    step(req('read', T('alice'), { query: 'after=9007199254740991' }), { status: 200, page: { seqs: [] } }),
  ]),
  g('a body where none belongs, or no length where one is needed', ['RELAY-GATE-003', 'RELAY-GATE-004'], [
    step(req('hello', T('alice'), { headers: { 'Content-Length': '5' }, body: { $hex: '0102030405' } }), gateErr(400, 'relay/unexpected_body')),
    step(req('hello', T('alice'), { headers: { 'Transfer-Encoding': 'chunked' } }), gateErr(400, 'relay/unexpected_body')),
    step(req('login-nonce', DOOR, { headers: { 'Content-Length': null, 'Transfer-Encoding': 'chunked' }, body: { $dev: 'alice' } }), gateErr(411, 'relay/length_required')),
    step(req('hello', T('alice'), { headers: { 'Content-Length': '0' } }), { status: 200, x_td_relay: 'object' }),
  ], { targets: LOCAL }),
  g('a body over the cap, or not of the fixed length', ['RELAY-GATE-005', 'RELAY-GATE-003', 'RELAY-GEN-003'], [
    step(req('login-nonce', DOOR, { body: { $fill: { len: 17, seed: 'x' } } }), gateErr(413, 'relay/too_large')),
    step(req('login-nonce', DOOR, { body: { $fill: { len: 15, seed: 'x' } } }), gateErr(400, 'relay/bad_body')),
    step(req('login', DOOR, { body: { $fill: { len: 149, seed: 'x' } } }), gateErr(413, 'relay/too_large')),
    step(req('pair-open', T('alice'), { body: { $fill: { len: 99, seed: 'x' } } }), gateErr(400, 'relay/bad_body')),
    step(req('admin', T('alice'), { params: { op: 'revoke' }, body: { $fill: { len: 137, seed: 'x' } } }), gateErr(413, 'relay/too_large')),
    step(req('admin', T('alice'), { params: { op: 'purge' }, body: { $fill: { len: 131, seed: 'x' } } }), gateErr(400, 'relay/bad_body')),
    step(req('admin', T('alice'), { params: { op: 'reinstate' }, body: { $fill: { len: 24719, seed: 'x' } } }), gateErr(413, 'relay/too_large')),
    step(req('slot-post', T('alice'), { params: { tid: '00112233445566778899aabbccddeeff', label: 'd' }, body: { $fill: { len: 16077, seed: 'x' } } }), gateErr(413, 'relay/too_large')),
  ]),
  g('a declared length over the cap is refused before the body is read', ['RELAY-GATE-005', 'RELAY-GEN-004'], [
    step(req('append', T('alice'), { slow: { declare: 4194374, send: 0 } }), { ...gateErr(413, 'relay/too_large'), elapsed_ms: [0, 5000] }),
    step(req('blob-put', T('alice'), { params: { id: '00112233445566778899aabbccddeeff', part: 0 }, slow: { declare: 1048577, send: 0 } }), gateErr(413, 'relay/too_large')),
  ], { targets: LOCAL }),
  g('a Content-Length that is not one decimal number', ['RELAY-GATE-003', 'RELAY-GEN-003'], [
    ...['abc', '-1', '1e3', '0x10', ' 16', '', ['16', '16'], '16, 16'].map((v) => step(req('login-nonce', DOOR, { headers: { 'Content-Length': v }, body: { $dev: 'alice' } }), gateErr(411, 'relay/length_required'))),
    step(req('login-nonce', DOOR, { headers: { 'Content-Length': '016' }, body: { $dev: 'alice' } }), { status: 200, x_td_relay: 'object' }),
    step(req('create', CREATE, { headers: { 'Content-Length': '99999999999999999999' }, body: { $create: { board: 'main' } } }), gateErr(413, 'relay/too_large')),
    step(req('create', CREATE, { headers: { 'Content-Length': '9007199254740993' }, body: { $create: { board: 'main' } } }), gateErr(413, 'relay/too_large')),
    step(req('hello', T('alice'), { headers: { 'Content-Length': ['0', '0'] } }), gateErr(400, 'relay/unexpected_body')),
    step(req('hello', T('alice'), { headers: { 'Content-Length': 'x' } }), gateErr(400, 'relay/unexpected_body')),
  ], { targets: CORE }),
  g('a body whose length is not the declared one', ['RELAY-GATE-007', 'RELAY-GATE-004'], [
    step(req('append', T('alice'), { headers: { 'Content-Length': '700' }, body: { $batch: [F('alice', 1)] } }), gateErr(400, 'relay/bad_body')),
    step(req('append', T('alice'), { headers: { 'Content-Length': '600' }, body: { $batch: [F('alice', 1)] } }), gateErr(400, 'relay/bad_body')),
    step(req('blob-put', T('alice'), { params: { id: '00112233445566778899aabbccddeeff', part: 0 }, headers: { 'Content-Length': '0' }, body: { $hex: '00' } }), gateErr(400, 'relay/bad_body')),
    step(req('append', T('alice'), { body: { $batch: [F('alice', 1)] } }), { status: 200, x_td_relay: 'object', object_called: true }),
  ], { targets: CORE }),
  g('the socket route without an upgrade', ['RELAY-GATE-003'], [
    step(req('socket', T('alice')), gateErr(400, 'relay/bad_upgrade')),
    step(req('socket', T('alice'), { headers: { Upgrade: 'h2c' } }), gateErr(400, 'relay/bad_upgrade')),
    step(req('socket', {}), gateErr(400, 'relay/bad_upgrade')),
  ]),
  g('no credential, a malformed one, or a form the route does not accept', ['RELAY-GATE-002', 'RELAY-GATE-009', 'RELAY-GATE-004', 'RELAY-TEST-002'], [
    [{}, { raw: 'Bearer abc' }, { raw: 'Basic dXNlcjpwYXNz' }, { raw: 'bearer ' + 'A'.repeat(96) }, { raw: 'Bearer  TD1.' + 'A'.repeat(92) }, { raw: 'Bearer TD1.' + 'A'.repeat(91) + '=' },
      { raw: 'Bearer TD2.' + 'A'.repeat(92) }, { raw: 'Door 0011223344' }, { raw: 'Create ' + '00'.repeat(32) }, { raw: 'Door 00112233' }, { raw: 'Door 00112233AA' }, { raw: '' },
    ].map((c) => step(req('hello', c), gateErr(401, 'relay/unauthorized'))),
    step(req('door', T('alice')), gateErr(401, 'relay/unauthorized')),
    step(req('create', DOOR, { body: { $create: { board: 'main' } } }), gateErr(401, 'relay/unauthorized')),
    step(req('append', CREATE, { body: { $batch: [F('alice', 1)] } }), gateErr(401, 'relay/unauthorized')),
    step(req('blob-begin', DOOR, { body: { $fill: { len: 65, seed: 'x' } } }), gateErr(401, 'relay/unauthorized')),
    step(req('admin', DOOR, { params: { op: 'register' }, body: { $fill: { len: 300, seed: 'x' } } }), gateErr(401, 'relay/unauthorized')),
    step(req('admin', T('alice'), { params: { op: 'seed' }, body: { $fill: { len: 300, seed: 'x' } } }), gateErr(401, 'relay/unauthorized')),
    step(req('admin', CREATE, { params: { op: 'revoke' }, body: { $fill: { len: 136, seed: 'x' } } }), gateErr(401, 'relay/unauthorized')),
    step(req('hello', T('alice'), { headers: { Authorization: [`Bearer TD1.${'A'.repeat(92)}`, `Bearer TD1.${'A'.repeat(92)}`] } }), gateErr(401, 'relay/unauthorized')),
  ], { boards: { main: { create: false } } }),
  g('a token with a wrong MAC, for another board, or of a role the route does not accept', ['RELAY-TOKEN-002', 'RELAY-GATE-009', 'RELAY-RTOKEN-003', 'SEAL-GEN-007'], [
    step(req('hello', { raw: 'Bearer TD1.' + 'A'.repeat(92) }), gateErr(401, 'relay/unauthorized')),
    step(req('hello', { token: 'alice', board: 'other' }), gateErr(401, 'relay/unauthorized')),
    ['append', 'login-nonce', 'login', 'socket', 'blob-begin', 'pair-open', 'pair-close', 'admin-nonce', 'seed-tail'].map((r) =>
      step(req(r, RT('alice'), { params: { tid: '00112233445566778899aabbccddeeff' }, ...(r === 'socket' ? { headers: { Upgrade: 'websocket' } } : ['pair-close'].includes(r) ? {} : { body: { $fill: { len: { 'login-nonce': 16, login: 148, 'blob-begin': 65, 'pair-open': 100, 'admin-nonce': 16 }[r] || 700, seed: 'x' } } }) }), gateErr(403, 'relay/forbidden'))),
    step(req('blob-put', RT('alice'), { params: { id: '00112233445566778899aabbccddeeff', part: 0 }, body: { $fill: { len: 70, seed: 'x' } } }), gateErr(403, 'relay/forbidden')),
    step(req('admin', RT('alice'), { params: { op: 'purge' }, body: { $fill: { len: 132, seed: 'x' } } }), gateErr(403, 'relay/forbidden')),
    step(req('slot-post', RT('alice'), { params: { tid: '00112233445566778899aabbccddeeff', label: 'c' }, body: { $fill: { len: 100, seed: 'x' } } }), gateErr(403, 'relay/forbidden')),
    step(req('caps', RT('alice')), { status: 200, x_td_relay: 'gate' }),
    step(req('caps', T('mallory')), { status: 200, x_td_relay: 'gate', object_called: false }),
  ], { boards: { main: {}, other: {} } }),
  g('the door', ['RELAY-DOOR-001', 'RELAY-DOOR-002', 'RELAY-CAPS-001', 'RELAY-CAPS-002', 'RELAY-ENC-004', 'RELAY-ENC-005', 'RELAY-SHAPE-004'], [
    step(req('caps', { raw: 'Door 0000000000' }), gateErr(401, 'relay/unauthorized')),
    step(req('caps', { door: 'other' }), gateErr(401, 'relay/unauthorized')),
    step(req('caps', DOOR), { status: 200, x_td_relay: 'gate', object_called: false, json: { v: 1, push: '$push', max_batch: 16, max_frame: 262144, page_entries: 256, page_bytes: 1048576, part_bytes: 1048576 } }),
    step(req('door', CREATE), { status: 200, x_td_relay: 'gate', object_called: false, json: { door: { $door: true } } }),
    step(req('caps', CREATE), { status: 200, x_td_relay: 'gate' }),
  ], { boards: { main: { create: false }, other: { create: false } } }),
  g('the create secret', ['RELAY-GATE-001'], [
    step(req('door', { create: '00'.repeat(32) }), gateErr(401, 'relay/unauthorized')),
    step(req('create', { create: 'ff'.repeat(32) }, { body: { $create: { board: 'main' } } }), gateErr(401, 'relay/unauthorized')),
    step(req('admin-nonce', { create: '7461736b6465636b2d72656c61792d746573742d6372656174652d3030303033' }, { body: { $dev: 'alice' } }), gateErr(401, 'relay/unauthorized')),
  ], { boards: { main: { create: false } } }),
  g('no create secret configured', ['RELAY-GATE-001', 'RELAY-SHAPE-012'], [
    step(req('door', CREATE), gateErr(403, 'relay/create_disabled')),
    step(req('create', CREATE, { body: { $create: { board: 'main' } } }), gateErr(403, 'relay/create_disabled')),
    step(req('admin-nonce', CREATE, { body: { $dev: 'alice' } }), gateErr(403, 'relay/create_disabled')),
    step(req('caps', CREATE), gateErr(403, 'relay/create_disabled')),
    step(req('admin', CREATE, { params: { op: 'seed' }, body: { $fill: { len: 300, seed: 'x' } } }), gateErr(403, 'relay/create_disabled')),
    step(req('caps', DOOR), { status: 200, x_td_relay: 'gate' }),
  ], { boards: { main: { create: false } }, setup: { create_secret: null } }),
  g('an invalid configuration', ['RELAY-SHAPE-012', 'RELAY-GATE-003'], [
    step(req('caps', DOOR), gateErr(503, 'relay/misconfigured')),
    step(raw('GET', '/nowhere', {}), gateErr(503, 'relay/misconfigured')),
    step(req('hello', {}), gateErr(503, 'relay/misconfigured')),
  ], { boards: { main: { create: false } }, setup: { token_key: TK.slice(2) }, targets: ['core', 'worker'] }),
  g('an origin that is not a relay origin', ['RELAY-SHAPE-012'], [
    step(req('caps', DOOR), gateErr(503, 'relay/misconfigured')),
  ], { boards: { main: { create: false } }, setup: { origin: 'https://relay.test:443' }, targets: ['core', 'worker'] }),
  g('the ticket proof', ['RELAY-GATE-008', 'SEAL-PAIR-015', 'RELAY-GEN-002'], [
    step(req('slot-get', DOOR, { params: { tid: { $tid: '11'.repeat(32) }, label: 'a' } }), gateErr(403, 'relay/bad_proof')),
    step(req('slot-get', DOOR, { params: { tid: { $tid: '11'.repeat(32) }, label: 'a' }, proof: '11'.repeat(31) }), gateErr(403, 'relay/bad_proof')),
    step(req('slot-get', DOOR, { params: { tid: { $tid: '11'.repeat(32) }, label: 'c' }, proof: '12'.repeat(32) }), gateErr(403, 'relay/bad_proof')),
    step(req('slot-get', DOOR, { params: { tid: { $tid: '11'.repeat(32) }, label: 'd' }, proof: 'AB'.repeat(32) }), gateErr(403, 'relay/bad_proof')),
    step(req('slot-post', DOOR, { params: { tid: { $tid: '11'.repeat(32) }, label: 'b' }, proof: '12'.repeat(32), body: { $slot: { label: 'b', body_len: 119 } } }), gateErr(403, 'relay/bad_proof')),
    step(req('slot-get', { raw: 'Door 0000000000' }, { params: { tid: { $tid: '11'.repeat(32) }, label: 'a' }, proof: '11'.repeat(32) }), gateErr(401, 'relay/unauthorized')),
    step(req('slot-get', T('alice'), { params: { tid: { $tid: '11'.repeat(32) }, label: 'a' }, proof: '11'.repeat(32) }), gateErr(401, 'relay/unauthorized')),
    step(req('slot-get', DOOR, { params: { tid: { $tid: '11'.repeat(32) }, label: 'a' }, proof: '11'.repeat(32) }), { ...err(404, 'relay/no_ticket'), object_called: true }),
  ]),
  g('headers named X-TD- from a client give it nothing', ['RELAY-GATE-010'], [
    step(req('append', DOOR, { headers: { 'X-TD-Cred': 'token', 'X-TD-Role': '1', 'X-TD-Dev': { $dev: 'alice' }, 'X-TD-Gen': '1' }, body: { $batch: [F('alice', 1)] } }),
      { status: 403, x_td_relay: 'object', results: [REF(403, 'relay/not_author')] }),
    step(req('hello', T('alice'), { headers: { 'X-TD-Cred': 'create', 'X-TD-Relay': 'gate', 'X-TD-Dev': '00'.repeat(16) } }), { status: 200, x_td_relay: 'object', json: { head: '$any', last_ctr: 0, last_seq: 0, appends_left: 3000, relay_now: '$any', floor: 1, purge_floor: 1, state: 'open', door: { $door: true }, push: '$push' } }),
  ]),
  g('a body that arrives too slowly', ['RELAY-GATE-006', 'RELAY-GATE-004', 'RELAY-TEST-002'], [
    step(req('append', T('alice'), { slow: { declare: 1000, send: 10 } }), { ...gateErr(408, 'relay/slow'), elapsed_ms: [10016, 11016] }),
  ], { targets: ['standby', 'worker'] }),
  fn('the body deadline', ['RELAY-GATE-006'], 'bodyDeadlineMs', { length: 1000 }, { expect: 10016 }),
  fn('the body deadline of a blob part', ['RELAY-GATE-006'], 'bodyDeadlineMs', { length: 1048576 }, { expect: 26000 }),
  fn('the body deadline of a full batch', ['RELAY-GATE-006'], 'bodyDeadlineMs', { length: 4194373 }, { expect: 74002 }),
];

// ---------- relay-token.json (§5.1, §5.2, §8.2)
const dev = (n) => createHash('sha256').update('taskdeck-test-dev').update(pk(n)).digest().subarray(0, 16).toString('hex');
function pk(name) { // the Ed25519 public key of a test seed
  return crypto0.createPublicKey(crypto0.createPrivateKey({ key: Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), Buffer.from(KEYS[name].seed, 'hex')]), format: 'der', type: 'pkcs8' })).export({ format: 'der', type: 'spki' }).subarray(12);
}
import * as crypto0 from 'node:crypto';
function tokenText(key, role, board, d, gen) {
  const head = Buffer.concat([Buffer.from([role]), Buffer.from(board, 'hex'), Buffer.from(d, 'hex'), Buffer.from([gen >>> 24, (gen >>> 16) & 255, (gen >>> 8) & 255, gen & 255])]);
  const mac = createHmac('sha256', Buffer.from(key, 'hex')).update('taskdeck/v1/token\0').update(head).digest();
  return 'TD1.' + Buffer.concat([head, mac]).toString('base64url');
}
const BOARD = '42'.repeat(16);
const t = (name, clauses, steps, o) => kase('token', name, clauses, steps, o);
export const token = [
  fn('token text of a device, generation 1', ['RELAY-TOKEN-001', 'SEAL-CTX-001'], 'makeToken', { token_key: TK, role: 1, board: BOARD, dev: dev('alice'), gen: 1 }, { expect: tokenText(TK, 1, BOARD, dev('alice'), 1) }),
  fn('token text of a backup reader, the largest generation', ['RELAY-TOKEN-001'], 'makeToken', { token_key: TK, role: 2, board: BOARD, dev: dev('bob'), gen: 4294967295 }, { expect: tokenText(TK, 2, BOARD, dev('bob'), 4294967295) }),
  fn('a token is read back', ['RELAY-TOKEN-002'], 'checkToken', { token_key: TK, board: BOARD, text: tokenText(TK, 1, BOARD, dev('alice'), 7) }, { expect: { role: 1, dev: dev('alice'), gen: 7 } }),
  fn('a token under another key', ['RELAY-TOKEN-002'], 'checkToken', { token_key: TK, board: BOARD, text: tokenText('00'.repeat(32), 1, BOARD, dev('alice'), 7) }, { error: 'relay/unauthorized' }),
  fn('a token of another board', ['RELAY-TOKEN-002'], 'checkToken', { token_key: TK, board: BOARD, text: tokenText(TK, 1, '43'.repeat(16), dev('alice'), 7) }, { error: 'relay/unauthorized' }),
  fn('a token of role 3', ['RELAY-TOKEN-002'], 'checkToken', { token_key: TK, board: BOARD, text: tokenText(TK, 3, BOARD, dev('alice'), 7) }, { error: 'relay/unauthorized' }),
  fn('a token one character short', ['RELAY-TOKEN-002'], 'checkToken', { token_key: TK, board: BOARD, text: tokenText(TK, 1, BOARD, dev('alice'), 7).slice(0, -1) }, { error: 'relay/unauthorized' }),
  fn('a token with a changed generation', ['RELAY-TOKEN-002'], 'checkToken', { token_key: TK, board: BOARD, text: (() => { const s = tokenText(TK, 1, BOARD, dev('alice'), 7); return s.slice(0, 52) + (s[52] === 'A' ? 'B' : 'A') + s.slice(53); })() }, { error: 'relay/unauthorized' }),
  fn('the door of a board', ['RELAY-DOOR-001', 'RELAY-DOOR-003', 'SEAL-CTX-001'], 'door', { token_key: TK, board: BOARD }, { expect: createHmac('sha256', Buffer.from(TK, 'hex')).update('taskdeck/v1/door\0').update(Buffer.from(BOARD, 'hex')).digest().subarray(0, 5).toString('hex') }),
  t('a login makes every older token stale, and the login routes still take it', ['RELAY-TOKEN-004', 'RELAY-OBJ-003', 'RELAY-LOGIN-004', 'RELAY-TOKEN-005'], [
    login('alice', 2),
    step(req('hello', T('alice', 1)), err(401, 'relay/token_stale')),
    step(req('read', T('alice', 1), { query: 'after=0' }), err(401, 'relay/token_stale')),
    step(req('append', T('alice', 1), { body: { $batch: [F('alice', 1)] } }), { ...err(401, 'relay/token_stale'), rows_written: 0 }),
    step(req('hello', T('alice', 3)), err(401, 'relay/token_stale')),
    step(req('hello', T('alice', 2)), { status: 200 }),
    login('alice', 3, T('alice', 1)),
    step(req('hello', T('alice', 2)), err(401, 'relay/token_stale')),
    step(req('hello', T('alice', 3)), { status: 200, storage: { devices: { alice: { gen: 3, state: 1 } } } }),
  ]),
  t('a token of a device the board does not know, or of the recovery identity', ['RELAY-OBJ-003', 'RELAY-OBJ-004'], [
    step(req('hello', T('mallory')), { ...err(401, 'relay/unauthorized'), object_called: true, rows_written: 0 }),
    step(req('hello', T('rec')), err(401, 'relay/unauthorized')),
    step(req('login-nonce', T('rec'), { body: { $dev: 'rec' } }), err(401, 'relay/unauthorized')),
    step(req('append', T('rec'), { body: { $batch: [F('rec', 2)] } }), { ...err(401, 'relay/unauthorized'), rows_written: 0 }),
    step(req('hello', RT('alice')), err(401, 'relay/unauthorized')),
  ]),
  t('a revoked device is told so whatever its generation, on every route', ['RELAY-OBJ-010', 'RELAY-OBJ-003', 'RELAY-REVOKE-001', 'RELAY-TOKEN-004'], [
    addDevice('bob', 2),
    admin('revoke', 'alice', T('alice'), { dev: 'bob' }, { status: 200, json: {}, storage: { devices: { bob: { state: 2, gen: 2 } } } }),
    [1, 2, 3].map((gen) => step(req('hello', T('bob', gen)), err(403, 'relay/revoked'))),
    step(req('login-nonce', T('bob', 1), { body: { $dev: 'bob' } }), err(403, 'relay/revoked')),
    step(req('login', T('bob', 2), { body: { $login: { dev: 'bob', gen: 3, nonce: { $hex: '00'.repeat(32) } } } }), err(403, 'relay/revoked')),
    step(req('login-nonce', DOOR, { body: { $dev: 'bob' } }), err(403, 'relay/revoked')),
    step(req('append', T('bob', 2), { body: { $batch: [F('bob', 1)] } }), { ...err(403, 'relay/revoked'), rows_written: 0 }),
    step(req('read', T('bob', 2), { query: 'after=0' }), err(403, 'relay/revoked')),
  ]),
  t('a backup-read token: minted, replaced, revoked', ['RELAY-RTOKEN-001', 'RELAY-TOKEN-004', 'RELAY-RTOKEN-003', 'RELAY-OBJ-003'], [
    admin('mint_read_token', 'alice', T('alice'), { dev: 'alice', epoch: 1 }, { status: 200, json: { token: { $token: { key: 'alice', role: 2, gen: 1 } }, gen: 1 }, rows_written: 2 }),
    step(req('hello', RT('alice', 1)), { status: 200, json: { head: { seq: 1, chain: { $chain: 1 } }, relay_now: { $time: 0 }, floor: 1, purge_floor: 1, state: 'open', door: { $door: true }, push: '$push' }, rows_written: 0 }),
    step(req('read', RT('alice', 1), { query: 'after=0' }), { status: 200, page: { seqs: [1] } }),
    step(req('blob-list', RT('alice', 1)), { status: 200, json: { blobs: [] } }),
    admin('mint_read_token', 'alice', T('alice'), { dev: 'alice', epoch: 1 }, { status: 200, json: { token: { $token: { key: 'alice', role: 2, gen: 2 } }, gen: 2 } }),
    step(req('hello', RT('alice', 1)), err(401, 'relay/token_stale')),
    step(req('hello', RT('alice', 2)), { status: 200 }),
    admin('revoke_read_token', 'alice', T('alice'), { dev: 'alice' }, { status: 200, json: {}, rows_written: 2 }),
    step(req('hello', RT('alice', 2)), err(401, 'relay/token_stale')),
    admin('revoke_read_token', 'alice', T('alice'), { dev: 'bob' }, err(409, 'relay/target')),
  ]),
];

// ---------- relay-address.json (§5.4)
const C32 = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
function b32(h) {
  const bits = [...Buffer.from(h, 'hex')].map((b) => b.toString(2).padStart(8, '0')).join('');
  return (bits + '0'.repeat((5 - (bits.length % 5)) % 5)).match(/.{5}/g).map((x) => C32[parseInt(x, 2)]).join('');
}
const AB = '000102030405060708090a0b0c0d0e0f', AD = 'a1b2c3d4e5', B34 = b32(AB + AD);
const parsed = (host) => ({ expect: { host, origin: 'https://' + host, board: AB, door: AD } });
const bad = { error: 'relay/address' };
const a = (name, text, out) => fn(name, ['RELAY-ADDR-002'], 'parseAddress', { text }, out);
export const address = [
  fn('the canonical text', ['RELAY-ADDR-001'], 'formatAddress', { host: 'relay-x7k2.example.org', board: AB, door: AD }, { expect: 'relay-x7k2.example.org/' + B34 }),
  fn('the display form: groups of four, the last of two', ['RELAY-ADDR-001'], 'displayAddress', { host: 'relay.example.org:8443', board: AB, door: AD }, { expect: 'relay.example.org:8443/' + B34.match(/.{1,4}/g).join('-') }),
  fn('the display form of the largest values', ['RELAY-ADDR-001'], 'displayAddress', { host: 'a.b', board: 'ff'.repeat(16), door: 'ff'.repeat(5) }, { expect: 'a.b/' + 'ZZZZ-'.repeat(8) + 'ZW' }),
  a('canonical text', 'relay.example.org/' + B34, parsed('relay.example.org')),
  a('the display form', 'relay.example.org/' + B34.match(/.{1,4}/g).join('-'), parsed('relay.example.org')),
  a('spaces, tabs and line breaks around it; https:// in any case', ' \t\nHTTPS://relay.example.org/' + B34 + '\r\n ', parsed('relay.example.org')),
  a('capitals in the host, lower case and spaces in the code', 'Relay.Example.ORG/' + B34.toLowerCase().match(/.{1,4}/g).join(' '), parsed('relay.example.org')),
  a('a port', 'relay.example.org:8443/' + B34, parsed('relay.example.org:8443')),
  a('port 443 is never kept', 'relay.example.org:443/' + B34, parsed('relay.example.org')),
  a('look-alikes: O for 0, I and L for 1', 'relay.example.org/' + B34.replace(/0/g, 'O').replace(/1/, 'I').replace(/1/, 'l'), parsed('relay.example.org')),
  a('another scheme', 'http://relay.example.org/' + B34, bad),
  a('no slash', 'relay.example.org' + B34, bad),
  a('two slashes', 'relay.example.org/' + B34 + '/', bad),
  a('a path after the scheme', 'https://relay.example.org/x/' + B34, bad),
  a('one character short', 'relay.example.org/' + B34.slice(1), bad),
  a('one character more', 'relay.example.org/' + B34 + '0', bad),
  a('the letter U', 'relay.example.org/U' + B34.slice(1), bad),
  a('padding bits that are not zero', 'relay.example.org/' + B34.slice(0, 33) + C32[C32.indexOf(B34[33]) | 1], bad),
  a('a host that starts with a hyphen', '-relay.example.org/' + B34, bad),
  a('an empty host', '/' + B34, bad),
  a('a port above 65535', 'relay.example.org:65536/' + B34, bad),
  a('a port with a leading zero', 'relay.example.org:0443/' + B34, bad),
  a('a host of 256 bytes', `${'a'.repeat(63)}.${'b'.repeat(63)}.${'c'.repeat(63)}.${'d'.repeat(64)}`.slice(0, 256) + '/' + B34, bad),
  a('an underscore in the host', 'relay_1.example.org/' + B34, bad),
  a('not text', 7, bad),
];

// ---------- relay-create.json (§10.2)
const c = (name, clauses, steps, o) => kase('create', name, clauses, steps, { boards: { main: { create: false } }, ...o });
const created = { status: 200, x_td_relay: 'object', json: { seq: 1, chain: { $chain: 1 }, token: { $token: { key: 'alice', gen: 1 } }, door: { $door: true } } };
const badGenesis = (body) => step(req('create', CREATE, { body: { $create: { board: 'main', ...body } } }), { ...err(400, 'relay/bad_genesis'), rows_written: 0, storage: { board: null, count: { devices: 0, log: 0 } } });
export const create = [
  c('a board is created', ['RELAY-CREATE-001', 'RELAY-CREATE-002', 'RELAY-STORE-001', 'RELAY-STORE-003', 'SEAL-KEY-005', 'SEAL-KEY-006', 'SEAL-CHAIN-001', 'RELAY-OBJ-006', 'RELAY-OBJ-007'], [
    step(req('hello', DOOR), gateErr(401, 'relay/unauthorized')),
    step(req('login-nonce', DOOR, { body: { $dev: 'alice' } }), { ...err(404, 'relay/no_board'), rows_written: 0 }),
    step(req('create', CREATE, { body: { $create: { board: 'main' } } }), {
      ...created, rows_written: 5, rows_read_max: 3,
      storage: {
        board: { one: 1, schema: 1, board_id: { $board: 'main' }, state: 1, created_at: { $time: 0 }, base_seq: 0, base_chain: { $chain: 0 }, compact_floor: 1, purge_floor: 1, purge_done: 1, compact_snap: null, del_day: 0, del_count: 0, read_epoch: 1, home_gen: 1, fence_host: null, fence_door: null, seed_signer: null, seed_last_seq: null, seed_chain: null },
        devices: { rec: { sign_pk: { $key: 'rec' }, role: 2, state: 1, gen: 0, last_ctr: 1, last_seq: 1, app_day: 0, app_count: 0, added_at: { $time: 0 }, wraps: null }, alice: { sign_pk: { $key: 'alice' }, role: 1, state: 1, gen: 1, last_ctr: 0, last_seq: 0, app_day: 0, app_count: 0, wraps: null } },
        log: [1], count: { devices: 2, nonces: 0, blobs: 0, parts: 0, tickets: 0, read_tokens: 0 },
      },
    }),
    step(req('read', T('alice'), { query: 'after=0' }), { status: 200, page: { head_seq: 1, floor: 1, purge_floor: 1, more: false, truncated: false, fenced: false, seqs: [1], frames: [F('rec', 1, { fill: 'genesis' })] } }),
  ], { targets: ALL }),
  c('a repeated create gives a new token and writes one row', ['RELAY-CREATE-003', 'RELAY-TOKEN-004'], [
    step(req('create', CREATE, { body: { $create: { board: 'main' } } }), created),
    step(req('create', CREATE, { body: { $create: { board: 'main' } } }), { status: 200, json: { seq: 1, chain: { $chain: 1 }, token: { $token: { key: 'alice', gen: 2 } }, door: { $door: true } }, rows_written: 1, rows_read_max: 4 }),
    step(req('hello', T('alice', 1)), err(401, 'relay/token_stale')),
    step(req('hello', T('alice', 2)), { status: 200 }),
  ]),
  c('any other create on a board that exists', ['RELAY-CREATE-003', 'RELAY-OBJ-001'], [
    step(req('create', CREATE, { body: { $create: { board: 'main' } } }), created),
    step(req('create', CREATE, { body: { $create: { board: 'main', frame: F('rec', 1, { fill: 'another genesis' }) } } }), { ...err(409, 'relay/exists'), rows_written: 0 }),
    step(req('create', CREATE, { body: { $create: { board: 'main', dev: 'bob' } } }), { ...err(409, 'relay/exists'), rows_written: 0 }),
    step(req('create', CREATE, { body: { $create: { board: 'main', sign_pk: 'bob' } } }), { ...err(409, 'relay/exists'), rows_written: 0 }),
    step(req('admin-nonce', CREATE, { body: { $dev: 'alice' } }), { ...err(409, 'relay/exists'), rows_written: 0 }),
  ]),
  c('a create request that is inconsistent', ['RELAY-CREATE-001', 'SEAL-KEY-005', 'SEAL-KEY-006', 'SEAL-FRAME-003', 'SEAL-GEN-010'], [
    badGenesis({ salt: '00'.repeat(16) }),
    badGenesis({ rsign: 'rec2' }),
    badGenesis({ frame: F('alice', 1) }),
    badGenesis({ frame: F('rec', 2) }),
    badGenesis({ frame: F('rec', 1, { sig: 'flip' }) }),
    badGenesis({ frame: F('rec', 1, { sig: 'high-s' }) }),
    badGenesis({ frame: F('rec', 1, { signer: 'alice' }) }),
    badGenesis({ frame: F('rec', 1, { body_len: 557 }) }),
    badGenesis({ frame: F('rec', 1, { magic: 'TDF2' }) }),
    badGenesis({ frame: F('rec', 1, { len_delta: 1 }) }),
    badGenesis({ frame: F('rec', 1, { board_id: '00'.repeat(16) }) }),
    badGenesis({ dev: 'rec' }),
    badGenesis({ sign_pk: 'rec' }),
    step(req('create', CREATE, { body: { $create: { board: 'main', flen: 671 } } }), { ...err(400, 'relay/bad_body'), rows_written: 0 }),
    step(req('create', CREATE, { body: { $fill: { len: 99, seed: 'x' } } }), { ...err(400, 'relay/bad_body'), rows_written: 0 }),
    step(req('create', CREATE, { body: { $create: { board: 'main' } } }), created),
  ]),
];

// ---------- relay-login.json (§10.3)
const l = (name, clauses, steps, o) => kase('login', name, clauses, steps, o);
const nonceFor = (key, cred = DOOR, more) => step(req('login-nonce', cred, { body: { $dev: key } }), { status: 200 }, more);
const tryLogin = (body, expect, cred = DOOR) => step(req('login', cred, { body: { $login: body } }), expect);
export const loginF = [
  l('a device logs in with the door', ['RELAY-LOGIN-001', 'RELAY-LOGIN-003', 'RELAY-LOGIN-004', 'RELAY-LOGIN-006', 'SEAL-SIG-001', 'SEAL-ENC-005', 'RELAY-GEN-001'], [
    step(req('login-nonce', DOOR, { body: { $dev: 'alice' } }), { status: 200, x_td_relay: 'object', json: { nonce: '$any', gen: 2, expires: { $time: 120000 } }, rows_written: 1, rows_read_max: 66, storage: { nonces: [{ purpose: 1, dev: { $dev: 'alice' }, at: { $time: 0 } }] } }),
    tryLogin({ dev: 'alice', gen: 2 }, { status: 200, x_td_relay: 'object', json: { token: { $token: { key: 'alice', gen: 2 } }, gen: 2, push: '$push' }, rows_written: 2, storage: { count: { nonces: 0 }, devices: { alice: { gen: 2 } } } }),
    step(req('hello', T('alice', 2)), { status: 200 }),
  ]),
  l('a token names its own device only', ['RELAY-LOGIN-001', 'RELAY-OBJ-004'], [
    addDevice('bob', 2),
    step(req('login-nonce', T('alice'), { body: { $dev: 'bob' } }), { ...err(403, 'relay/forbidden'), rows_written: 0 }),
    nonceFor('bob', DOOR, { save: { nb: 'nonce' } }),
    tryLogin({ dev: 'bob', gen: 2, nonce: '$saved.nb' }, { ...err(403, 'relay/forbidden'), rows_written: 0 }, T('alice')),
    tryLogin({ dev: 'bob', gen: 2, nonce: '$saved.nb' }, { status: 200 }),
  ]),
  l('a device the board does not know, and the recovery identity', ['RELAY-LOGIN-001'], [
    step(req('login-nonce', DOOR, { body: { $dev: 'mallory' } }), { ...err(401, 'relay/unauthorized'), rows_written: 0 }),
    step(req('login-nonce', DOOR, { body: { $dev: 'rec' } }), { ...err(401, 'relay/unauthorized'), rows_written: 0 }),
    tryLogin({ dev: 'mallory', gen: 1, nonce: { $hex: '00'.repeat(32) } }, { ...err(401, 'relay/no_nonce'), rows_written: 0 }),
  ]),
  l('a nonce is tried once', ['RELAY-LOGIN-001', 'RELAY-LOGIN-002', 'RELAY-OBJ-009'], [
    tryLogin({ dev: 'alice', gen: 2, nonce: { $hex: '5a'.repeat(32) } }, { ...err(401, 'relay/no_nonce'), rows_written: 0 }),
    nonceFor('alice', DOOR, { save: { n1: 'nonce' } }),
    tryLogin({ dev: 'alice', gen: 2, nonce: '$saved.n1' }, { status: 200 }),
    tryLogin({ dev: 'alice', gen: 3, nonce: '$saved.n1' }, { ...err(401, 'relay/no_nonce'), rows_written: 0 }),
    nonceFor('alice', DOOR, { save: { n2: 'nonce' } }),
    tryLogin({ dev: 'alice', gen: 2, nonce: '$saved.n2' }, { ...err(409, 'relay/bad_gen'), rows_written: 1, storage: { count: { nonces: 0 } } }),
    tryLogin({ dev: 'alice', gen: 3, nonce: '$saved.n2' }, { ...err(401, 'relay/no_nonce'), rows_written: 0 }),
    nonceFor('alice'),
    tryLogin({ dev: 'alice', gen: 5 }, { ...err(409, 'relay/bad_gen'), rows_written: 1 }),
  ]),
  l('a nonce belongs to one device and one purpose', ['RELAY-LOGIN-001', 'RELAY-ADMIN-001'], [
    addDevice('bob', 2),
    nonceFor('bob', DOOR, { save: { nb: 'nonce' } }),
    tryLogin({ dev: 'alice', gen: 2, nonce: '$saved.nb' }, { ...err(401, 'relay/no_nonce'), rows_written: 0 }),
    step(req('admin-nonce', T('alice'), { body: { $dev: 'alice' } }), { status: 200 }, { save: { na: 'nonce' } }),
    tryLogin({ dev: 'alice', gen: 2, nonce: '$saved.na' }, { ...err(401, 'relay/no_nonce'), rows_written: 0 }),
    tryLogin({ dev: 'bob', gen: 2, nonce: '$saved.nb' }, { status: 200 }),
  ]),
  l('a login signature that does not verify', ['RELAY-LOGIN-003', 'RELAY-LOGIN-002', 'SEAL-SIG-001', 'SEAL-ENC-005', 'SEAL-CTX-001', 'RELAY-GEN-009'], [
    ...[{ origin: 'https://other.test' }, { sig: 'flip' }, { signer: 'rec' }, { context: 'frame' }, { context: 'admin' }, { sign_gen: 3 }, { origin: 'https://relay.test/' }].flatMap((x) => [
      nonceFor('alice'),
      tryLogin({ dev: 'alice', gen: 2, ...x }, { ...err(401, 'relay/bad_login_sig'), rows_written: 1, storage: { count: { nonces: 0 }, devices: { alice: { gen: 1 } } } }),
    ]),
    login('alice', 2),
  ]),
  l('a nonce lives 120,000 ms', ['RELAY-LOGIN-001'], [
    nonceFor('alice', DOOR, { save: { n: 'nonce' } }),
    at(NOW + 120001, tryLogin({ dev: 'alice', gen: 2, nonce: '$saved.n' }, { ...err(401, 'relay/no_nonce'), rows_written: 0 })),
    at(NOW + 120001, nonceFor('alice', DOOR, { save: { m: 'nonce' } })),
    at(NOW + 240001, tryLogin({ dev: 'alice', gen: 2, nonce: '$saved.m' }, { status: 200 })),
  ], { targets: LOCAL }),
  l('at most 64 nonces are kept: expired ones go first, then the oldest', ['RELAY-LOGIN-005', 'RELAY-TOKEN-005'], [
    range(64, (i) => at(NOW + i, nonceFor('alice', DOOR, i === 0 ? { save: { first: 'nonce' } } : i === 1 ? { save: { second: 'nonce' } } : {}))),
    at(NOW + 64, step(req('login-nonce', DOOR, { body: { $dev: 'alice' } }), { status: 200, rows_written: 2, storage: { count: { nonces: 64 } } })),
    at(NOW + 65, tryLogin({ dev: 'alice', gen: 2, nonce: '$saved.first' }, err(401, 'relay/no_nonce'))),
    at(NOW + 66, step(req('admin-nonce', T('alice'), { body: { $dev: 'alice' } }), { status: 200, rows_written: 2, storage: { count: { nonces: 64 } } })),
    at(NOW + 67, tryLogin({ dev: 'alice', gen: 2, nonce: '$saved.second' }, err(401, 'relay/no_nonce'))),
    at(NOW + 120010, step(req('login-nonce', DOOR, { body: { $dev: 'alice' } }), { status: 200, rows_written: 9, storage: { count: { nonces: 57 } } })),
    at(NOW + 240066, step(req('login-nonce', DOOR, { body: { $dev: 'alice' } }), { status: 200, rows_written: 58, storage: { count: { nonces: 1 } } })),
  ], { targets: LOCAL }),
];

// ---------- relay-append.json (§9)
const ap = (name, clauses, steps, o) => kase('append', name, clauses, steps, { state: [addDevice('bob', 2)], ...o });
const one = (cred, frame, code, status) => append(cred, [frame], [REF(status, code)], status, { rows_written: 0 });
export const appendF = [
  ap('new frames are stored at the head, three rows for one frame', ['RELAY-SEQ-001', 'RELAY-SEQ-007', 'RELAY-SEQ-010', 'RELAY-SEQ-011', 'RELAY-STORE-003', 'RELAY-STORE-004', 'RELAY-ROWS-001', 'SEAL-FRAME-001', 'SEAL-FRAME-002', 'SEAL-CHAIN-001'], [
    append(T('alice'), [F('alice', 2)], [N(3)], 200, { rows_written: 3, rows_read_max: 8, storage: { log: [1, 2, 3], devices: { alice: { last_ctr: 2, last_seq: 3, app_count: 2 } } } }),
    append(T('alice'), [F('alice', 3), F('alice', 4), F('alice', 5, { body_len: 3000 })], [N(4), N(5), N(6)], 200, { rows_written: 7, rows_read_max: 12 }),
    append(T('bob'), [F('bob', 1)], [N(7)], 200, { rows_written: 3 }),
    step(req('read', T('bob'), { query: 'after=2' }), { status: 200, page: { head_seq: 7, seqs: [3, 4, 5, 6, 7], frames: [F('alice', 2), F('alice', 3), F('alice', 4), F('alice', 5, { body_len: 3000 }), F('bob', 1)] } }),
  ]),
  ap('a replay is acknowledged again and writes no row', ['RELAY-SEQ-002', 'RELAY-SEQ-005', 'RELAY-SEQ-010', 'RELAY-ROWS-001'], [
    append(T('alice'), [F('alice', 2), F('alice', 3)], [N(3), N(4)]),
    append(T('alice'), [F('alice', 2), F('alice', 3)], [DUP(3), DUP(4)], 200, { rows_written: 0, rows_read_max: 9 }),
    append(T('alice'), [F('alice', 3), F('alice', 4)], [DUP(4), N(5)], 200, { rows_written: 3 }),
    append(T('alice'), [F('alice', 1)], [DUP(2)], 200, { rows_written: 0 }),
    append(T('alice'), [F('alice', 4), F('alice', 4), F('alice', 4)], [DUP(5), DUP(5), DUP(5)], 200, { rows_written: 0 }),
  ]),
  ap('another frame under a stored counter is a conflict that carries the stored frame', ['RELAY-SEQ-002', 'RELAY-SEQ-004', 'RELAY-SEQ-005', 'RELAY-SEQ-011'], [
    append(T('alice'), [F('alice', 2)], [N(3)]),
    append(T('alice'), [F('alice', 2, { fill: 'other' }), F('alice', 3)], [{ status: 'conflict', seq: 3, frame: F('alice', 2) }, 'skipped'], 409, { rows_written: 0 }),
    append(T('alice'), [F('alice', 3), F('alice', 1, { fill: 'other' }), F('alice', 4)], [N(4), { status: 'conflict', seq: 2, frame: F('alice', 1) }, 'skipped'], 409, { rows_written: 3, storage: { log: [1, 2, 3, 4] } }),
  ]),
  ap('a counter above the next one is a gap', ['RELAY-SEQ-003', 'RELAY-SEQ-004', 'RELAY-SEQ-005'], [
    append(T('alice'), [F('alice', 3)], [{ status: 'gap', last_ctr: 1, last_seq: 2 }], 409, { rows_written: 0 }),
    append(T('alice'), [F('alice', 2), F('alice', 4), F('alice', 3)], [N(3), { status: 'gap', last_ctr: 2, last_seq: 3 }, 'skipped'], 409, { rows_written: 3 }),
    append(T('bob'), [F('bob', 2)], [{ status: 'gap', last_ctr: 0, last_seq: 0 }], 409, { rows_written: 0 }),
  ]),
  ap('a frame the relay refuses stops the batch', ['RELAY-SEQ-004', 'RELAY-SEQ-006', 'RELAY-SEQ-008', 'SEAL-FRAME-003', 'SEAL-FRAME-001', 'RELAY-GEN-008', 'RELAY-GEN-004'], [
    one(T('alice'), F('alice', 2, { body_len: 557 }), 'relay/bad_frame', 400),
    one(T('alice'), F('alice', 2, { magic: 'TDF2' }), 'relay/bad_frame', 400),
    one(T('alice'), F('alice', 2, { len_delta: 1 }), 'relay/bad_frame', 400),
    one(T('alice'), F('alice', 2, { len_delta: -1 }), 'relay/bad_frame', 400),
    one(T('alice'), F('alice', 2, { tail: '00' }), 'relay/bad_frame', 400),
    one(T('alice'), F('alice', 0), 'relay/bad_frame', 400),
    one(T('alice'), F('alice', 0, { ctr_hex: '0020000000000000' }), 'relay/bad_frame', 400),
    one(T('alice'), F('alice', 0, { ctr_hex: 'ffffffffffffffff' }), 'relay/bad_frame', 400),
    one(T('alice'), F('alice', 2, { board_id: '00'.repeat(16) }), 'relay/wrong_board', 400),
    one(T('alice'), F('mallory', 1), 'relay/unknown_author', 400),
    one(T('alice'), F('bob', 1), 'relay/not_author', 403),
    one(T('bob'), F('alice', 2), 'relay/not_author', 403),
    append(T('alice'), [F('alice', 2), F('bob', 1), F('alice', 3)], [N(3), REF(403, 'relay/not_author'), 'skipped'], 403, { rows_written: 3 }),
    append(T('alice'), [F('alice', 3), F('alice', 4, { sig: 'flip' }), F('alice', 5)], [N(4), REF(400, 'relay/bad_signature'), 'skipped'], 400, { rows_written: 3 }),
  ]),
  ap('a signature that is not strictly valid', ['RELAY-SEQ-006', 'RELAY-GEN-009', 'RELAY-GEN-010', 'SEAL-GEN-010', 'SEAL-FRAME-002', 'SEAL-CTX-001'], [
    one(T('alice'), F('alice', 2, { sig: 'flip' }), 'relay/bad_signature', 400),
    one(T('alice'), F('alice', 2, { sig: 'zero-s' }), 'relay/bad_signature', 400),
    one(T('alice'), F('alice', 2, { sig: 'high-s' }), 'relay/bad_signature', 400),
    one(T('alice'), F('alice', 2, { signer: 'bob' }), 'relay/bad_signature', 400),
    append(T('alice'), [F('alice', 2)], [N(3)]),
  ]),
  ap('frames of the recovery identity, from a token and from the door', ['RELAY-SEQ-008', 'RELAY-SEQ-009'], [
    append(T('alice'), [F('rec', 2)], [N(3)], 200, { rows_written: 3, storage: { devices: { rec: { last_ctr: 2, last_seq: 3, app_count: 0 }, alice: { last_ctr: 1 } } } }),
    append(DOOR, [F('rec', 3)], [N(4)], 200, { rows_written: 3 }),
    append(DOOR, [F('rec', 4), F('alice', 2), F('rec', 5)], [N(5), REF(403, 'relay/not_author'), 'skipped'], 403, { rows_written: 3 }),
    append(DOOR, [F('alice', 2)], [REF(403, 'relay/not_author')], 403, { rows_written: 0 }),
    append(T('bob'), [F('rec', 5), F('bob', 1), F('rec', 6)], [N(6), N(7), N(8)], 200, { rows_written: 8 }),
    append(DOOR, [F('rec', 6, { sig: 'flip' })], [REF(400, 'relay/bad_signature')], 400, { rows_written: 0 }),
  ]),
  ap('a batch that is not laid out as a batch', ['RELAY-APPEND-001', 'RELAY-ENC-003', 'RELAY-GEN-003', 'SEAL-GEN-008'], [
    ...[{ $cat: [{ $hex: '54444132' }, { $u8: 1 }, { $frames: [F('alice', 2)] }] }, { $cat: [{ $hex: '54444131' }, { $u8: 0 }] }, { $cat: [{ $hex: '54444131' }, { $u8: 17 }, { $frames: range(17, (i) => F('alice', 2 + i)) }] },
      { $cat: [{ $hex: '54444131' }, { $u8: 2 }, { $frames: [F('alice', 2)] }] }, { $cat: [{ $batch: [F('alice', 2)] }, { $hex: '00' }] }, { $cat: [{ $hex: '54444131' }, { $u8: 1 }, { $u32: 4000000000 }, F('alice', 2)] },
      { $hex: '544441' }, { $hex: '' }, { $cat: [{ $hex: '54444131' }, { $u8: 1 }, { $u32: 671 }, F('alice', 2)] },
    ].map((body) => step(req('append', T('alice'), { body }), { ...err(400, 'relay/bad_batch'), rows_written: 0 })),
    append(T('alice'), [F('alice', 2)], [N(3)]),
  ]),
];

// ---------- relay-read.json (§10.4, §10.5)
const r = (name, clauses, steps, o) => kase('read', name, clauses, steps, o);
const page = (query, p, more = {}) => step(req('read', T('alice'), { query }), { status: 200, x_td_relay: 'object', page: p, rows_written: 0, ...more });
export const read = [
  r('hello states the head, the counters and the budget', ['RELAY-HELLO-001', 'RELAY-ENC-004', 'RELAY-DOOR-003', 'RELAY-OBJ-005'], [
    step(req('hello', T('alice')), { status: 200, x_td_relay: 'object', json: { head: { seq: 1, chain: { $chain: 1 } }, last_ctr: 0, last_seq: 0, appends_left: 3000, relay_now: { $time: 0 }, floor: 1, purge_floor: 1, state: 'open', door: { $door: true }, push: '$push' }, rows_written: 0, rows_read_max: 3 }),
    append(T('alice'), [F('alice', 1), F('alice', 2)], [N(2), N(3)]),
    step(req('hello', T('alice')), { status: 200, json: { head: { seq: 3, chain: { $chain: 3 } }, last_ctr: 2, last_seq: 3, appends_left: 2998, relay_now: { $time: 0 }, floor: 1, purge_floor: 1, state: 'open', door: { $door: true }, push: '$push' }, rows_written: 0 }),
  ]),
  r('pages follow the cursor and the limit', ['RELAY-READ-001', 'RELAY-READ-002', 'RELAY-READ-003', 'RELAY-ROWS-001'], [
    append(T('alice'), range(9, (i) => F('alice', 1 + i)), range(9, (i) => N(2 + i))),
    page('after=0', { head_seq: 10, floor: 1, purge_floor: 1, more: false, truncated: false, fenced: false, relay_now: { $time: 0 }, seqs: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] }, { rows_read_max: 14 }),
    page('after=0&limit=3', { head_seq: 10, more: true, seqs: [1, 2, 3] }, { rows_read_max: 7 }),
    page('after=3&limit=3', { head_seq: 10, more: true, seqs: [4, 5, 6] }),
    page('after=7&limit=3', { head_seq: 10, more: false, seqs: [8, 9, 10] }),
    page('after=9&limit=1', { head_seq: 10, more: false, seqs: [10], frames: [F('alice', 9)] }),
    page('after=10', { head_seq: 10, more: false, truncated: false, seqs: [] }, { rows_read_max: 4 }),
    page('after=500', { head_seq: 10, more: false, seqs: [] }),
  ]),
  r('a page holds at most 256 entries', ['RELAY-READ-001'], [
    range(17, (b) => append(T('alice'), range(16, (i) => F('alice', 1 + 16 * b + i)), range(16, (i) => N(2 + 16 * b + i)))),
    page('after=0', { head_seq: 273, more: true, seqs: range(256, (i) => 1 + i) }, { rows_read_max: 260 }),
    page('after=256', { head_seq: 273, more: false, seqs: range(17, (i) => 257 + i) }),
    page('after=0&limit=256', { more: true, seqs: range(256, (i) => 1 + i) }),
  ]),
  r('a page holds at most 1 MiB of frames, and always one entry', ['RELAY-READ-001', 'SEAL-FRAME-003'], [
    append(T('alice'), range(5, (i) => big('alice', 1 + i)), range(5, (i) => N(2 + i)), 200, { rows_written: 11 }),
    append(T('alice'), [F('alice', 6)], [N(7)]),
    page('after=1', { head_seq: 7, more: true, seqs: [2, 3, 4, 5] }, { rows_read_max: 9 }),
    page('after=0', { head_seq: 7, more: true, seqs: [1, 2, 3, 4] }),
    page('after=5', { head_seq: 7, more: false, seqs: [6, 7] }),
    page('after=2&limit=1', { head_seq: 7, more: true, seqs: [3], frames: [big('alice', 2)] }),
    append(T('alice'), [F('alice', 7, { body_len: 262033 })], [REF(400, 'relay/bad_frame')], 400),
  ]),
];
