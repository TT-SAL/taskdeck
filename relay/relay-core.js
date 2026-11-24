// TaskDeck relay core: every rule of docs/sync/spec/relay.md that decides a
// status, a response body, a socket message or a storage effect. worker.js and
// standby.js call it and decide nothing themselves (RELAY-SHAPE-001).
import { createHash, createHmac, timingSafeEqual } from 'node:crypto';

export const PROTOCOL = 1;

// ---------- limits (§20), error catalogue (§19)

export const LIMITS = Object.freeze({
  maxBatch: 16, minFrame: 670, maxFrame: 262144, maxInt: Number.MAX_SAFE_INTEGER,
  maxAppendBody: 4194373, maxSocketMessage: 4194378,
  deadlineBaseMs: 10000, deadlineBytesPerSecond: 65536,
  pageEntries: 256, pageBytes: 1048576, maxLimit: 256,
  nonceLifetimeMs: 120000, maxNonces: 64,
  ticketLifetimeMs: 600000, maxTickets: 4, maxTries: 5,
  slotA: 84, slotBMin: 155, slotBMax: 278, slotC: 100, slotDMin: 76, slotDMax: 16076,
  minBlob: 67, maxBlob: 67108864, partBytes: 1048576, maxParts: 64,
  maxIncompleteBlobs: 3, maxBlobBytes: 805306368, blobLifetimeMs: 3600000,
  snapshotsKept: 3, otherBlobsKept: 2,
  appendBudget: 3000, deleteBudget: 5000,
  compactAgeMs: 31536000000, maxKeep: 131072, compactWindow: 16384,
  restoreFrames: 64, restoreBytes: 1048576, seedTailFrames: 256, seedTailBytes: 1048576,
  seedEntries: 320, maxCurrentDevices: 16, maxWraps: 256, maxDevices: 256, maxRecovery: 64,
  tokenChars: 96, doorBytes: 5, addressChars: 34, maxHost: 255, maxGen: 4294967295,
  maxAttachment: 2048, dayShare: 0.02, importShare: 0.2,
});

export const ERRORS = Object.freeze({
  'relay/not_found': 404, 'relay/bad_query': 400, 'relay/unexpected_body': 400,
  'relay/length_required': 411, 'relay/too_large': 413, 'relay/bad_body': 400,
  'relay/bad_upgrade': 400, 'relay/unauthorized': 401, 'relay/forbidden': 403,
  'relay/create_disabled': 403, 'relay/bad_proof': 403, 'relay/slow': 408,
  'relay/object_unavailable': 503, 'relay/misconfigured': 503, 'relay/no_push': 501,
  'relay/no_board': 404, 'relay/seeding': 409, 'relay/fenced': 403, 'relay/exists': 409,
  'relay/revoked': 403, 'relay/token_stale': 401, 'relay/gen_exhausted': 409,
  'relay/bad_genesis': 400, 'relay/no_nonce': 401, 'relay/bad_gen': 409,
  'relay/bad_login_sig': 401, 'relay/bad_admin_sig': 401, 'relay/not_signer': 403,
  'relay/bad_args': 400, 'relay/bad_batch': 400, 'relay/bad_frame': 400,
  'relay/wrong_board': 400, 'relay/unknown_author': 400, 'relay/not_author': 403,
  'relay/bad_signature': 400, 'relay/budget': 429, 'relay/conflict': 409, 'relay/gap': 409,
  'relay/duplicate_key': 409, 'relay/device_cap': 409, 'relay/frame_mismatch': 400,
  'relay/target': 409, 'relay/last_device': 409, 'relay/home_gen': 409,
  'relay/not_next': 409, 'relay/chain_mismatch': 409, 'relay/no_snapshot': 409,
  'relay/too_recent': 409, 'relay/delete_budget': 429, 'relay/stale_epoch': 409,
  'relay/bad_blob': 400, 'relay/blob_exists': 409, 'relay/blob_quota': 409,
  'relay/no_blob': 404, 'relay/blob_incomplete': 409, 'relay/part_conflict': 409,
  'relay/bad_slot': 400, 'relay/ticket_exists': 409, 'relay/ticket_cap': 409,
  'relay/no_ticket': 404, 'relay/tries': 409, 'relay/slot_order': 409,
  'relay/slot_empty': 404, 'relay/bad_message': 400, 'relay/storage_refused': 507,
  'relay/address': 400,
});

export class RelayError extends Error {
  constructor(status, code, extra) {
    super(code); // the message is the code alone (RELAY-GEN-005)
    this.status = status;
    this.code = code;
    this.extra = extra || {};
  }
}

function fail(code, extra) {
  throw new RelayError(ERRORS[code], code, extra);
}
const E = (code, extra) => new RelayError(ERRORS[code], code, extra);

// ---------- bytes

const MAX = Number.MAX_SAFE_INTEGER;
const MAXGEN = 4294967295;
const DAY = 86400000;
const MIB = 1048576;
const te = new TextEncoder();
const EMPTY = new Uint8Array(0);

function cat(...parts) {
  let n = 0;
  for (const p of parts) n += p.length;
  const out = new Uint8Array(n);
  let o = 0;
  for (const p of parts) { out.set(p, o); o += p.length; }
  return out;
}
const u8 = (v) => Uint8Array.of(v);
const u16 = (v) => Uint8Array.of((v >>> 8) & 255, v & 255);
const u32 = (v) => Uint8Array.of((v >>> 24) & 255, (v >>> 16) & 255, (v >>> 8) & 255, v & 255);
const u64 = (v) => cat(u32(Math.floor(v / 4294967296)), u32(v >>> 0));
const vb8 = (b) => cat(u8(b.length), b);

function readU32(b, o) {
  return b[o] * 16777216 + ((b[o + 1] << 16) | (b[o + 2] << 8) | b[o + 3]);
}
// A u64 within 0 to 2^53 - 1, or null (RELAY-GEN-008).
function readU64(b, o) {
  const hi = readU32(b, o);
  return hi > 0x1fffff ? null : hi * 4294967296 + readU32(b, o + 4);
}

// Reads a body by its layout. Every length is checked against the bytes that
// remain before anything is sliced (RELAY-GEN-003); `code` is the refusal.
class Reader {
  constructor(bytes, code) { this.b = bytes; this.o = 0; this.code = code; }
  need(n) { if (n > this.b.length - this.o) fail(this.code); }
  u8() { this.need(1); return this.b[this.o++]; }
  u16() { this.need(2); this.o += 2; return (this.b[this.o - 2] << 8) | this.b[this.o - 1]; }
  u32() { this.need(4); this.o += 4; return readU32(this.b, this.o - 4); }
  u64() { this.need(8); this.o += 8; const v = readU64(this.b, this.o - 8); if (v === null) fail(this.code); return v; }
  u64OrNull() { this.need(8); this.o += 8; return readU64(this.b, this.o - 8); }
  bytes(n) { this.need(n); this.o += n; return this.b.subarray(this.o - n, this.o); }
  left() { return this.b.length - this.o; }
  end() { if (this.o !== this.b.length) fail(this.code); }
}

const HEX = '0123456789abcdef';
export function hex(bytes) {
  let s = '';
  for (let i = 0; i < bytes.length; i++) s += HEX[bytes[i] >> 4] + HEX[bytes[i] & 15];
  return s;
}
function fromHex(text) {
  const out = new Uint8Array(text.length >> 1);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(text.substr(i * 2, 2), 16);
  return out;
}
export function unhex(text, width) {
  if (typeof text !== 'string' || text.length !== width * 2 || !/^[0-9a-f]*$/.test(text)) fail('relay/bad_body');
  return fromHex(text);
}

// Lengths first, then timingSafeEqual (RELAY-GEN-002, SEAL-GEN-007).
export function ctEqual(a, b) {
  return a.length === b.length && timingSafeEqual(a, b);
}
function cmp(a, b) {
  for (let i = 0; i < a.length && i < b.length; i++) if (a[i] !== b[i]) return a[i] - b[i];
  return a.length - b.length;
}

const CTX = new Map();
// The registered context string of seal.md §4.1: "taskdeck/v1/<use>" and one zero byte.
function ctx(use) {
  let c = CTX.get(use);
  if (!c) { c = te.encode('taskdeck/v1/' + use + '\0'); CTX.set(use, c); }
  return c;
}
function H(...parts) {
  const h = createHash('sha256');
  for (const p of parts) h.update(p);
  return new Uint8Array(h.digest());
}
function hmac(key, ...parts) {
  const h = createHmac('sha256', key);
  for (const p of parts) h.update(p);
  return new Uint8Array(h.digest());
}

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
function b64url(bytes) {
  let s = '';
  for (let i = 0; i < bytes.length; i += 3) {
    const n = (bytes[i] << 16) | ((bytes[i + 1] || 0) << 8) | (bytes[i + 2] || 0);
    s += B64[n >> 18] + B64[(n >> 12) & 63];
    if (i + 1 < bytes.length) s += B64[(n >> 6) & 63];
    if (i + 2 < bytes.length) s += B64[n & 63];
  }
  return s;
}
// 92 characters of the alphabet give exactly 69 bytes with no bit left over.
function unb64url92(text) {
  const out = new Uint8Array(69);
  for (let i = 0, o = 0; i < 92; i += 4, o += 3) {
    const n = (B64.indexOf(text[i]) << 18) | (B64.indexOf(text[i + 1]) << 12) | (B64.indexOf(text[i + 2]) << 6) | B64.indexOf(text[i + 3]);
    out[o] = n >> 16; out[o + 1] = (n >> 8) & 255; out[o + 2] = n & 255;
  }
  return out;
}

// ---------- tokens, the door, the address (§5)

export function makeToken(tokenKey, role, board, dev, gen) {
  const head = cat(u8(role), board, dev, u32(gen));
  return 'TD1.' + b64url(cat(head, hmac(tokenKey, ctx('token'), head)));
}

export function checkToken(tokenKey, text, board) {
  if (typeof text !== 'string' || !/^TD1\.[A-Za-z0-9_-]{92}$/.test(text)) fail('relay/unauthorized');
  const raw = unb64url92(text.slice(4));
  const head = raw.subarray(0, 37);
  const macOk = ctEqual(raw.subarray(37), hmac(tokenKey, ctx('token'), head));
  const role = raw[0];
  if (!macOk || (role !== 1 && role !== 2) || !ctEqual(raw.subarray(1, 17), board)) fail('relay/unauthorized');
  return { role, dev: raw.slice(17, 33), gen: readU32(raw, 33) };
}

export function door(tokenKey, board) {
  return hmac(tokenKey, ctx('door'), board).subarray(0, 5);
}

const HOST_RE = /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)*(:[1-9][0-9]{0,4})?$/;
// The host rule of seal.md §5.4.
function validHost(h) {
  if (typeof h !== 'string' || h.length < 1 || h.length > 255 || !HOST_RE.test(h)) return false;
  const colon = h.indexOf(':');
  if ((colon < 0 ? h.length : colon) > 253) return false;
  if (colon >= 0) {
    const port = Number(h.slice(colon + 1));
    if (port > 65535 || port === 443) return false;
  }
  return true;
}
function validOrigin(o) {
  return typeof o === 'string' && o.length <= 263 && o.startsWith('https://') && validHost(o.slice(8));
}

const C32 = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
function base32(bytes) {
  let out = '', acc = 0, bits = 0;
  for (const b of bytes) {
    acc = (acc << 8) | b; bits += 8;
    while (bits >= 5) { bits -= 5; out += C32[(acc >>> bits) & 31]; }
    acc &= (1 << bits) - 1;
  }
  if (bits) out += C32[(acc << (5 - bits)) & 31];
  return out;
}

export function formatAddress(host, board, doorBytes) {
  return host + '/' + base32(cat(board, doorBytes));
}
export function displayAddress(host, board, doorBytes) {
  return host + '/' + base32(cat(board, doorBytes)).match(/.{1,4}/g).join('-');
}
// The six steps of §5.4, in order.
export function parseAddress(text) {
  const bad = () => fail('relay/address');
  if (typeof text !== 'string' || text.length > 1024) bad();
  let t = text.replace(/^[ \t\r\n]+/, '').replace(/[ \t\r\n]+$/, '');
  if (/^https:\/\//i.test(t)) t = t.slice(8);
  if (t.includes('://')) bad();
  const parts = t.split('/');
  if (parts.length !== 2) bad();
  let host = parts[0].replace(/[A-Z]/g, (c) => c.toLowerCase());
  if (host.endsWith(':443')) host = host.slice(0, -4);
  if (!validHost(host)) bad();
  const b = parts[1].replace(/[ \t-]/g, '').toUpperCase().replace(/O/g, '0').replace(/[IL]/g, '1');
  if (b.length !== 34) bad();
  const raw = new Uint8Array(21);
  let acc = 0, bits = 0, o = 0;
  for (const ch of b) {
    const v = C32.indexOf(ch);
    if (v < 0) bad();
    acc = (acc << 5) | v; bits += 5;
    if (bits >= 8) { bits -= 8; raw[o++] = (acc >>> bits) & 255; acc &= (1 << bits) - 1; }
  }
  if (acc !== 0) bad(); // the two padding bits
  return { host, origin: 'https://' + host, board: raw.slice(0, 16), door: raw.slice(16, 21) };
}

// ---------- crypto (§3.4, seal.md §8, §15, §16)

const ED_L = fromHex('edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010');
const ED_P = fromHex('edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f');
// The y of every point of order 1, 2, 4 or 8: 0, 1, p - 1, Y1, Y2.
const ED_SMALL = [
  '0000000000000000000000000000000000000000000000000000000000000000',
  '0100000000000000000000000000000000000000000000000000000000000000',
  'ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f',
  '26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05',
  'c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a',
].map(fromHex);
function ltLE(a, b) { // a < b, both 32 bytes little-endian
  for (let i = 31; i >= 0; i--) if (a[i] !== b[i]) return a[i] < b[i];
  return false;
}
function strictPoint(enc) {
  const y = new Uint8Array(enc);
  y[31] &= 0x7f;
  if (!ltLE(y, ED_P)) return false;
  for (const s of ED_SMALL) if (cmp(y, s) === 0) return false;
  return true;
}

// Never accepts what ed25519-dalek verify_strict rejects (RELAY-GEN-009, §3.4).
export async function verifyStrict(pk, msg, sig) {
  if (!pk || !sig || pk.length !== 32 || sig.length !== 64) return false; // step 1
  if (!ltLE(sig.subarray(32, 64), ED_L)) return false; // step 2
  if (!strictPoint(pk) || !strictPoint(sig.subarray(0, 32))) return false; // step 3
  try { // step 4
    const key = await crypto.subtle.importKey('raw', pk, { name: 'Ed25519' }, false, ['verify']);
    return (await crypto.subtle.verify({ name: 'Ed25519' }, key, sig, msg)) === true;
  } catch {
    return false;
  }
}

export function frameHash(frame) {
  return H(frame);
}
export function chainZero(board) {
  return H(ctx('chain'), board);
}
export function chainStep(prev, seq, fh) {
  return H(ctx('chain'), prev, u64(seq), fh);
}
export function loginMessage(origin, board, dev, gen, relayNonce, clientNonce) {
  const o = te.encode(origin);
  return cat(ctx('login'), u16(o.length), o, board, dev, u32(gen), relayNonce, clientNonce);
}
export function adminMessage(op, board, signer, args, relayNonce) {
  return cat(ctx('admin'), vb8(te.encode(op)), board, signer, H(args), relayNonce);
}
function recoveryDev(board, signPk) {
  return H(ctx('recovery-dev'), board, signPk).subarray(0, 16);
}

// ---------- replies (§4.2)

function reply(status, type, body, by) {
  return { status, headers: { 'Cache-Control': 'no-store', 'X-TD-Relay': by, 'Content-Type': type }, body };
}
const json = (status, obj, by = 'object') => reply(status, 'application/json', te.encode(JSON.stringify(obj)), by);
const bin = (status, bytes, by = 'object') => reply(status, 'application/octet-stream', bytes, by);

export function errorReply(e, by) {
  const x = e.extra || {};
  const body = { error: e.code };
  for (const k of ['until', 'index', 'head', 'read_epoch', 'last_ctr', 'last_seq']) if (x[k] !== undefined) body[k] = x[k];
  return json(e.status, body, by);
}
// The reply for one code of the catalogue; the fronts use it for the refusals of §6.2 steps 9 and 10.
export function refusal(code, by = 'gate') {
  return errorReply(E(code), by);
}

// ---------- the gate (§6)

const T1 = 'T1', T2 = 'T2', DOOR = 'D', CREATE = 'C';
const route = (name, method, pattern, creds, cap, fixed, object) =>
  Object.freeze({ name, method, pattern, creds: Object.freeze(creds), cap, fixed, object });

export const ROUTES = Object.freeze([
  route('caps', 'GET', '/v1/b/{board}/caps', [T1, T2, DOOR, CREATE], 0, 0, false),
  route('door', 'GET', '/v1/b/{board}/door', [CREATE], 0, 0, false),
  route('create', 'POST', '/v1/b/{board}/create', [CREATE], 262244, null, true),
  route('login-nonce', 'POST', '/v1/b/{board}/login/nonce', [T1, DOOR], 16, 16, true),
  route('login', 'POST', '/v1/b/{board}/login', [T1, DOOR], 148, 148, true),
  route('hello', 'GET', '/v1/b/{board}/hello', [T1, T2], 0, 0, true),
  route('socket', 'GET', '/v1/b/{board}/socket', [T1], 0, 0, true),
  route('read', 'GET', '/v1/b/{board}/log', [T1, T2], 0, 0, true),
  route('append', 'POST', '/v1/b/{board}/append', [T1, DOOR], 4194373, null, true),
  route('blob-list', 'GET', '/v1/b/{board}/blobs', [T1, T2], 0, 0, true),
  route('blob-begin', 'POST', '/v1/b/{board}/blobs', [T1], 65, 65, true),
  route('blob-meta', 'GET', '/v1/b/{board}/blobs/{id}', [T1, T2], 0, 0, true),
  route('blob-put', 'PUT', '/v1/b/{board}/blobs/{id}/{part}', [T1], 1048576, null, true),
  route('blob-get', 'GET', '/v1/b/{board}/blobs/{id}/{part}', [T1, T2], 0, 0, true),
  route('pair-open', 'POST', '/v1/b/{board}/pair', [T1], 100, 100, true),
  route('slot-post', 'POST', '/v1/b/{board}/pair/{tid}/{label}', [T1, DOOR], 16076, null, true),
  route('slot-get', 'GET', '/v1/b/{board}/pair/{tid}/{label}', [T1, DOOR], 0, 0, true),
  route('pair-close', 'DELETE', '/v1/b/{board}/pair/{tid}', [T1], 0, 0, true),
  route('admin-nonce', 'POST', '/v1/b/{board}/admin/nonce', [T1, DOOR, CREATE], 16, 16, true),
  route('admin', 'POST', '/v1/b/{board}/admin/{op}', [T1, DOOR, CREATE], 1051044, null, true),
  route('seed-tail', 'POST', '/v1/b/{board}/seed/tail', [T1], 1049610, null, true),
]);

// The admin operations of §12.1: credentials, body cap, fixed length, who may sign.
const OPS = new Map([
  ['register', { creds: [T1], cap: 262344, fixed: null, signer: 1 }],
  ['revoke', { creds: [T1, DOOR], cap: 136, fixed: 136, signer: 0 }],
  ['reinstate', { creds: [T1, DOOR], cap: 24718, fixed: null, signer: 2 }],
  ['fence', { creds: [T1, DOOR], cap: 262529, fixed: null, signer: 0 }],
  ['seed', { creds: [CREATE], cap: 18782, fixed: null, signer: 1 }],
  ['compact', { creds: [T1], cap: 1048732, fixed: null, signer: 1 }],
  ['restore_tail', { creds: [T1, DOOR], cap: 1051044, fixed: null, signer: 0 }],
  ['purge', { creds: [T1], cap: 132, fixed: 132, signer: 1 }],
  ['mint_read_token', { creds: [T1, DOOR], cap: 140, fixed: 140, signer: 0 }],
  ['revoke_read_token', { creds: [T1, DOOR], cap: 136, fixed: 136, signer: 0 }],
  ['recovery_register', { creds: [T1, DOOR], cap: 208, fixed: 208, signer: 0 }],
]);

const PARAM = {
  board: /^[0-9a-f]{32}$/, tid: /^[0-9a-f]{32}$/, id: /^[0-9a-f]{32}$/,
  part: /^(0|[1-9][0-9]?)$/, label: /^[abcd]$/, op: /^[a-z_]{1,32}$/,
};
const SEGS = new Map(ROUTES.map((r) => [r, r.pattern.split('/')]));

function parseQuery(name, query) {
  if (query === null || query === undefined) {
    if (name === 'read') fail('relay/bad_query'); // `after` is required
    return {};
  }
  if (name !== 'read' || typeof query !== 'string' || query.length > 64) fail('relay/bad_query');
  const out = {};
  for (const pair of query.split('&')) {
    const m = /^(after|limit)=(0|[1-9][0-9]{0,15})$/.exec(pair);
    if (!m || m[1] in out) fail('relay/bad_query');
    out[m[1]] = Number(m[2]);
  }
  if (!('after' in out) || out.after > MAX) fail('relay/bad_query');
  if (!('limit' in out)) out.limit = 256;
  if (out.limit < 1 || out.limit > 256) fail('relay/bad_query');
  return out;
}

// Steps 1 and 2 of §6.2. `query` is the text after "?", or null when the URL has none.
export function matchRoute(method, path, query) {
  const seg = typeof path === 'string' && path.length <= 256 ? path.split('/') : [];
  for (const r of ROUTES) {
    const pat = SEGS.get(r);
    if (r.method !== method || pat.length !== seg.length) continue;
    const params = {};
    let ok = true;
    for (let i = 0; i < pat.length && ok; i++) {
      if (pat[i][0] === '{') {
        const name = pat[i].slice(1, -1);
        if (PARAM[name].test(seg[i])) params[name] = seg[i]; else ok = false;
      } else if (pat[i] !== seg[i]) ok = false;
    }
    if (!ok) continue;
    let { creds, cap, fixed } = r;
    let proof = false;
    if (r.name === 'blob-put' || r.name === 'blob-get') {
      if (Number(params.part) > 63) continue;
    } else if (r.name === 'slot-post') {
      if (params.label === 'a') continue;
      if (params.label === 'b') { creds = [DOOR]; proof = true; } else creds = [T1];
    } else if (r.name === 'slot-get') {
      if (params.label === 'b') creds = [T1]; else { creds = [DOOR]; proof = true; }
    } else if (r.name === 'admin') {
      const op = OPS.get(params.op);
      if (!op) continue;
      ({ creds, cap, fixed } = op);
    }
    const found = { name: r.name, method: r.method, creds, cap, fixed, object: r.object, proof };
    return { route: found, params, query: parseQuery(r.name, query) };
  }
  return fail('relay/not_found');
}

const hv = (headers, name) => (headers && headers[name]) || [];

// Steps 3 to 5 of §6.2. `headers` maps lowercase names to the list of values received.
export function checkHeaders(r, headers) {
  const te_ = hv(headers, 'transfer-encoding'), cl = hv(headers, 'content-length');
  let contentLength = 0;
  if (r.cap === 0) {
    if (te_.length || cl.length > 1 || (cl.length === 1 && !/^0+$/.test(cl[0]))) fail('relay/unexpected_body');
  } else {
    if (te_.length || cl.length !== 1 || !/^[0-9]+$/.test(cl[0])) fail('relay/length_required');
    const digits = cl[0].replace(/^0+(?=[0-9])/, '');
    contentLength = digits.length > 15 ? Infinity : Number(digits);
    if (contentLength > r.cap) fail('relay/too_large');
    if (r.fixed !== null && contentLength !== r.fixed) fail('relay/bad_body');
  }
  if (r.name === 'socket') {
    const up = hv(headers, 'upgrade');
    if (up.length !== 1 || up[0].toLowerCase() !== 'websocket') fail('relay/bad_upgrade');
  }
  return { contentLength };
}

const AUTH_TOKEN = /^Bearer (TD1\.[A-Za-z0-9_-]{92})$/;
const AUTH_DOOR = /^Door ([0-9a-f]{10})$/;
const AUTH_CREATE = /^Create ([0-9a-f]{64})$/;

// Steps 6 and 7 of §6.2.
export async function checkCredential(r, params, headers, config) {
  const auth = hv(headers, 'authorization');
  if (auth.length !== 1) fail('relay/unauthorized');
  const board = fromHex(params.board);
  let cred, m;
  if ((m = AUTH_TOKEN.exec(auth[0]))) {
    if (!r.creds.includes(T1) && !r.creds.includes(T2)) fail('relay/unauthorized');
    const t = checkToken(config.tokenKey, m[1], board);
    if (!r.creds.includes(t.role === 1 ? T1 : T2)) fail('relay/forbidden');
    cred = { cred: 'token', role: t.role, dev: t.dev, gen: t.gen };
  } else if ((m = AUTH_DOOR.exec(auth[0]))) {
    if (!r.creds.includes(DOOR)) fail('relay/unauthorized');
    if (!ctEqual(fromHex(m[1]), door(config.tokenKey, board))) fail('relay/unauthorized');
    cred = { cred: 'door' };
  } else if ((m = AUTH_CREATE.exec(auth[0]))) {
    if (!r.creds.includes(CREATE)) fail('relay/unauthorized');
    if (!config.createSecret) fail('relay/create_disabled');
    const got = fromHex(m[1]);
    const ok = ctEqual(got, config.createSecret);
    got.fill(0); // RELAY-GEN-006
    if (!ok) fail('relay/unauthorized');
    cred = { cred: 'create' };
  } else fail('relay/unauthorized');
  if (r.proof) {
    const p = hv(headers, 'x-td-proof');
    if (p.length !== 1 || !/^[0-9a-f]{64}$/.test(p[0])) fail('relay/bad_proof');
    const key = fromHex(p[0]);
    const tid = H(ctx('pair-tid'), key).subarray(0, 16);
    key.fill(0); // RELAY-GEN-006
    if (!ctEqual(tid, fromHex(params.tid))) fail('relay/bad_proof');
  }
  return cred;
}

// Step 9 of §6.2, once the front has the body or has given up on it: null when the
// whole body arrived, else the refusal. `received` null means the deadline passed.
export function checkBody(contentLength, received) {
  if (received === null) return refusal('relay/slow');
  return received.length === contentLength ? null : refusal('relay/bad_body');
}

export function bodyDeadlineMs(contentLength) {
  return 10000 + Math.ceil((contentLength * 1000) / 65536);
}

// Step 8 of §6.2: caps and door, and the socket route on a relay without push (§15.2).
export function gateAnswer(r, params, cred, config) {
  if (r.name === 'caps') {
    return json(200, { v: PROTOCOL, push: config.push, max_batch: 16, max_frame: 262144, page_entries: 256, page_bytes: MIB, part_bytes: MIB }, 'gate');
  }
  if (r.name === 'door') return json(200, { door: hex(door(config.tokenKey, fromHex(params.board))) }, 'gate');
  if (r.name === 'socket' && !config.push) return refusal('relay/no_push');
  return null;
}

// The first invalid value of a configuration (§17), by its field name, or null.
export function configError({ tokenKey, createSecret, origin }) {
  if (typeof tokenKey !== 'string' || !/^[0-9a-f]{64}$/.test(tokenKey)) return 'tokenKey';
  const hasCreate = createSecret !== undefined && createSecret !== null;
  if (hasCreate && (typeof createSecret !== 'string' || !/^[0-9a-f]{64}$/.test(createSecret))) return 'createSecret';
  return validOrigin(origin) ? null : 'origin';
}

// The configuration of §17 from its text values, decoded once (RELAY-GEN-006):
// a Config, or null when a value is invalid (step 0 of §6.2).
export function makeConfig({ tokenKey, createSecret, origin, push }) {
  if (configError({ tokenKey, createSecret, origin })) return null;
  const hasCreate = createSecret !== undefined && createSecret !== null;
  return Object.freeze({ tokenKey: fromHex(tokenKey), createSecret: hasCreate ? fromHex(createSecret) : null, origin, push: push === true });
}

// Steps 0 to 8 of §6.2 for one request: { reply } when the gate answers, else what
// the front needs to read the body (step 9) and call the object (step 10).
export async function gate(req, config) {
  try {
    if (!config) fail('relay/misconfigured');
    const { route: r, params, query } = matchRoute(req.method, req.path, req.query);
    const { contentLength } = checkHeaders(r, req.headers);
    const cred = await checkCredential(r, params, req.headers, config);
    const answer = gateAnswer(r, params, cred, config);
    if (answer) return { reply: answer };
    return { route: r, params, query, cred, contentLength, deadlineMs: bodyDeadlineMs(contentLength) };
  } catch (e) {
    if (e instanceof RelayError) return { reply: errorReply(e, 'gate') };
    throw e;
  }
}

// ---------- storage (§7)

export const SQL = Object.freeze({
  schema: Object.freeze([
    `CREATE TABLE IF NOT EXISTS board (
  one           INTEGER PRIMARY KEY CHECK (one = 1),
  schema        INTEGER NOT NULL,
  board_id      BLOB    NOT NULL,
  state         INTEGER NOT NULL,
  created_at    INTEGER NOT NULL,
  base_seq      INTEGER NOT NULL,
  base_chain    BLOB    NOT NULL,
  compact_floor INTEGER NOT NULL,
  purge_floor   INTEGER NOT NULL,
  purge_done    INTEGER NOT NULL,
  compact_snap  BLOB,
  del_day       INTEGER NOT NULL,
  del_count     INTEGER NOT NULL,
  read_epoch    INTEGER NOT NULL,
  home_gen      INTEGER NOT NULL,
  fence_host    TEXT,
  fence_door    BLOB,
  seed_signer   BLOB,
  seed_last_seq INTEGER,
  seed_chain    BLOB
)`,
    `CREATE TABLE IF NOT EXISTS devices (
  dev      BLOB    PRIMARY KEY NOT NULL,
  sign_pk  BLOB    NOT NULL,
  role     INTEGER NOT NULL,
  state    INTEGER NOT NULL,
  gen      INTEGER NOT NULL,
  last_ctr INTEGER NOT NULL,
  last_seq INTEGER NOT NULL,
  app_day  INTEGER NOT NULL,
  app_count INTEGER NOT NULL,
  added_at INTEGER NOT NULL,
  wraps    BLOB
) WITHOUT ROWID`,
    `CREATE TABLE IF NOT EXISTS log (
  seq   INTEGER PRIMARY KEY NOT NULL,
  dev   BLOB    NOT NULL,
  ctr   INTEGER NOT NULL,
  hash  BLOB    NOT NULL,
  chain BLOB    NOT NULL,
  at    INTEGER NOT NULL,
  frame BLOB    NOT NULL
)`,
    'CREATE UNIQUE INDEX IF NOT EXISTS log_dev_ctr ON log (dev, ctr)',
    `CREATE TABLE IF NOT EXISTS blobs (
  id       BLOB    PRIMARY KEY NOT NULL,
  slot     INTEGER NOT NULL,
  purpose  INTEGER NOT NULL,
  seq      INTEGER NOT NULL,
  author   BLOB    NOT NULL,
  size     INTEGER NOT NULL,
  parts    INTEGER NOT NULL,
  sha256   BLOB    NOT NULL,
  received INTEGER NOT NULL,
  complete INTEGER NOT NULL,
  at       INTEGER NOT NULL
) WITHOUT ROWID`,
    `CREATE TABLE IF NOT EXISTS parts (
  pk   INTEGER PRIMARY KEY NOT NULL,
  data BLOB    NOT NULL
)`,
    `CREATE TABLE IF NOT EXISTS tickets (
  tid     BLOB    PRIMARY KEY NOT NULL,
  creator BLOB    NOT NULL,
  expires INTEGER NOT NULL,
  tries   INTEGER NOT NULL,
  a BLOB NOT NULL, b BLOB, c BLOB, d BLOB
) WITHOUT ROWID`,
    `CREATE TABLE IF NOT EXISTS nonces (
  nonce   BLOB    PRIMARY KEY NOT NULL,
  purpose INTEGER NOT NULL,
  dev     BLOB    NOT NULL,
  at      INTEGER NOT NULL
) WITHOUT ROWID`,
    `CREATE TABLE IF NOT EXISTS read_tokens (
  dev   BLOB    PRIMARY KEY NOT NULL,
  gen   INTEGER NOT NULL,
  epoch INTEGER NOT NULL,
  state INTEGER NOT NULL
) WITHOUT ROWID`,
  ]),
  boardGet: 'SELECT * FROM board WHERE one = 1',
  boardInsert: 'INSERT INTO board (one, schema, board_id, state, created_at, base_seq, base_chain, compact_floor, purge_floor, purge_done, compact_snap, del_day, del_count, read_epoch, home_gen, fence_host, fence_door, seed_signer, seed_last_seq, seed_chain) VALUES (1, 1, ?, ?, ?, ?, ?, ?, ?, ?, NULL, 0, 0, 1, ?, NULL, NULL, ?, ?, ?)',
  boardFence: 'UPDATE board SET state = 2, home_gen = ?, fence_host = ?, fence_door = ? WHERE one = 1',
  boardOpen: 'UPDATE board SET state = 1, seed_signer = NULL, seed_last_seq = NULL, seed_chain = NULL WHERE one = 1',
  boardCompact: 'UPDATE board SET compact_floor = ?, compact_snap = ?, del_day = ?, del_count = ? WHERE one = 1',
  boardPurge: 'UPDATE board SET purge_floor = ?, purge_done = ?, compact_snap = ?, del_day = ?, del_count = ?, read_epoch = ? WHERE one = 1',
  boardReadEpoch: 'UPDATE board SET read_epoch = ? WHERE one = 1',
  devGet: 'SELECT * FROM devices WHERE dev = ?',
  devAll: 'SELECT dev, sign_pk, role, state, gen FROM devices',
  devInsert: 'INSERT INTO devices (dev, sign_pk, role, state, gen, last_ctr, last_seq, app_day, app_count, added_at, wraps) VALUES (?, ?, ?, ?, ?, ?, ?, 0, 0, ?, NULL)',
  devGen: 'UPDATE devices SET gen = ? WHERE dev = ?',
  devCounters: 'UPDATE devices SET last_ctr = ?, last_seq = ?, app_day = ?, app_count = ? WHERE dev = ?',
  devRevoke: 'UPDATE devices SET state = 2, gen = ?, wraps = NULL WHERE dev = ?',
  devReinstate: 'UPDATE devices SET state = 1, wraps = ? WHERE dev = ?',
  devRetire: 'UPDATE devices SET state = 2 WHERE role = 2 AND state = 1',
  logHead: 'SELECT seq, chain, at FROM log ORDER BY seq DESC LIMIT 1',
  logInsert: 'INSERT INTO log (seq, dev, ctr, hash, chain, at, frame) VALUES (?, ?, ?, ?, ?, ?, ?)',
  logByCtr: 'SELECT seq, hash, chain FROM log WHERE dev = ? AND ctr = ?',
  logFrame: 'SELECT frame FROM log WHERE seq = ?',
  logAt: 'SELECT seq, dev, hash, chain, at FROM log WHERE seq = ?',
  logPage: 'SELECT seq, at, frame FROM log WHERE seq > ? AND seq >= ? ORDER BY seq LIMIT ?',
  logDeleteRange: 'DELETE FROM log WHERE seq >= ? AND seq < ?',
  logDeleteCounted: 'DELETE FROM log WHERE seq >= ? AND seq < ? RETURNING seq',
  nonceFor: 'SELECT nonce, at FROM nonces WHERE purpose = ? AND dev = ?',
  nonceExpire: 'DELETE FROM nonces WHERE at < ?',
  nonceCount: 'SELECT COUNT(*) AS n FROM nonces',
  nonceTrim: 'DELETE FROM nonces WHERE nonce IN (SELECT nonce FROM nonces ORDER BY at, nonce LIMIT ?)',
  nonceInsert: 'INSERT INTO nonces (nonce, purpose, dev, at) VALUES (?, ?, ?, ?)',
  nonceDelete: 'DELETE FROM nonces WHERE nonce = ?',
  ticketAll: 'SELECT tid, creator, expires, a FROM tickets',
  ticketGet: 'SELECT * FROM tickets WHERE tid = ?',
  ticketInsert: 'INSERT INTO tickets (tid, creator, expires, tries, a, b, c, d) VALUES (?, ?, ?, 0, ?, NULL, NULL, NULL)',
  ticketDelete: 'DELETE FROM tickets WHERE tid = ?',
  ticketTries: 'UPDATE tickets SET tries = ? WHERE tid = ?',
  ticketSetB: 'UPDATE tickets SET tries = ?, b = ? WHERE tid = ?',
  ticketSetC: 'UPDATE tickets SET c = ? WHERE tid = ?',
  ticketSetD: 'UPDATE tickets SET d = ? WHERE tid = ?',
  blobAll: 'SELECT * FROM blobs',
  blobGet: 'SELECT * FROM blobs WHERE id = ?',
  blobInsert: 'INSERT INTO blobs (id, slot, purpose, seq, author, size, parts, sha256, received, complete, at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?)',
  blobProgress: 'UPDATE blobs SET received = ?, complete = ? WHERE id = ?',
  blobDelete: 'DELETE FROM blobs WHERE id = ?',
  partGet: 'SELECT data FROM parts WHERE pk = ?',
  partInsert: 'INSERT INTO parts (pk, data) VALUES (?, ?)',
  partsDelete: 'DELETE FROM parts WHERE pk >= ? AND pk <= ?',
  rtGet: 'SELECT * FROM read_tokens WHERE dev = ?',
  rtInsert: 'INSERT INTO read_tokens (dev, gen, epoch, state) VALUES (?, ?, ?, 1)',
  rtUpdate: 'UPDATE read_tokens SET gen = ?, epoch = ?, state = 1 WHERE dev = ?',
  rtRevoke: 'UPDATE read_tokens SET state = 2 WHERE dev = ?',
  rtRevokeAll: 'UPDATE read_tokens SET state = 2 WHERE state = 1',
  rtRevokeBelow: 'UPDATE read_tokens SET state = 2 WHERE state = 1 AND epoch < ? AND dev <> ?',
  wipe: Object.freeze(['DELETE FROM board', 'DELETE FROM devices', 'DELETE FROM log', 'DELETE FROM blobs', 'DELETE FROM parts', 'DELETE FROM tickets', 'DELETE FROM read_tokens']),
});

// The Storage interface over node:sqlite's DatabaseSync (standby, tests).
export function sqliteStorage(db) {
  const cache = new Map();
  const stmt = (sql) => {
    let s = cache.get(sql);
    if (!s) { s = db.prepare(sql); cache.set(sql, s); }
    return s;
  };
  return {
    all: (sql, ...params) => stmt(sql).all(...params),
    run: (sql, ...params) => ({ changes: Number(stmt(sql).run(...params).changes) }),
    transaction(fn) {
      db.exec('BEGIN IMMEDIATE');
      let out;
      try { out = fn(); } catch (e) { db.exec('ROLLBACK'); throw e; }
      db.exec('COMMIT');
      return out;
    },
  };
}

// A relay without sockets (the standby, §15.2).
export const noSockets = Object.freeze({
  accept() { throw new Error('no sockets'); },
  list() { return []; },
  attachment() { return null; },
  setAttachment() {},
  send() {},
  close() {},
});

// ---------- frames, batches, results, pages (§9, §10.5)

// P1 to P3 of §9.3: { code } or the header fields and the signed bytes.
function frameHead(frame, board) {
  const n = frame.length;
  if (n < 670 || n > 262144 || frame[0] !== 0x54 || frame[1] !== 0x44 || frame[2] !== 0x46 || frame[3] !== 0x31) return { code: 'relay/bad_frame' };
  const len = readU32(frame, 44);
  if (len !== n - 112) return { code: 'relay/bad_frame' };
  if (!ctEqual(frame.subarray(4, 20), board)) return { code: 'relay/wrong_board' };
  const ctr = readU64(frame, 36);
  if (ctr === null || ctr < 1) return { code: 'relay/bad_frame' };
  return { dev: frame.subarray(20, 36), ctr, signed: cat(ctx('frame'), frame.subarray(0, 48 + len)), sig: frame.subarray(48 + len) };
}

function parseBatch(body) {
  const r = new Reader(body, 'relay/bad_batch');
  const m = r.bytes(4);
  if (m[0] !== 0x54 || m[1] !== 0x44 || m[2] !== 0x41 || m[3] !== 0x31) fail('relay/bad_batch');
  const n = r.u8();
  if (n < 1 || n > 16) fail('relay/bad_batch');
  const frames = [];
  for (let i = 0; i < n; i++) frames.push(r.bytes(r.u32()));
  r.end();
  return frames;
}

// Results carry t = the status byte of §9.6.
const refused = (code) => ({ t: 4, status: ERRORS[code], code });

function encodeResults(results) {
  const parts = [te.encode('TDR1'), u8(results.length)];
  for (const r of results) {
    parts.push(u8(r.t));
    if (r.t === 0 || r.t === 1) parts.push(u64(r.seq), r.chain);
    else if (r.t === 2) parts.push(u64(r.seq), u32(r.frame.length), r.frame);
    else if (r.t === 3) parts.push(u64(r.last_ctr), u64(r.last_seq));
    else if (r.t === 4) parts.push(u16(r.status), vb8(te.encode(r.code)));
  }
  return cat(...parts);
}
function resultsStatus(results) {
  for (const r of results) {
    if (r.t === 2 || r.t === 3) return 409;
    if (r.t === 4) return r.status;
  }
  return 200;
}

function encodePage(board, head, now, rows, more, truncated) {
  const flags = (more ? 1 : 0) | (truncated ? 2 : 0) | (board.state === 2 ? 4 : 0);
  const parts = [te.encode('TDP1'), u64(head.seq), head.chain, u64(now), u64(Math.max(board.compact_floor, board.purge_floor)), u64(board.purge_floor), u8(flags), u16(rows.length)];
  for (const r of rows) parts.push(u64(r.seq), u64(r.at), u32(r.frame.length), r.frame);
  return cat(...parts);
}

const SLOTS = { a: [0x41, 0x45, 84, 84], b: [0x42, 0x4e, 155, 278], c: [0x43, 0x45, 100, 100], d: [0x44, 0x45, 76, 16076] };
// The label byte, role byte and length of a slot; never its MAC (RELAY-PAIR-004).
function slotOk(label, s) {
  const [first, role, min, max] = SLOTS[label];
  return s.length >= min && s.length <= max && s[0] === first && s[1] === role && ((s[2] << 8) | s[3]) === s.length - 36;
}

const blobJson = (b) => ({
  id: hex(b.id), purpose: b.purpose, seq: b.seq, author: hex(b.author), size: b.size,
  parts: b.parts, sha256: hex(b.sha256), received: b.received, complete: b.complete === 1,
});

// ---------- admin envelope and arguments (§12.1, seal.md §16.2)

const A = (b) => new Reader(b, 'relay/bad_args');
const ARGS = {
  register(b) { const r = A(b); const o = { dev: r.bytes(16), pk: r.bytes(32), hash: r.bytes(32) }; r.end(); return o; },
  revoke(b) { const r = A(b); const o = { dev: r.bytes(16) }; r.end(); return o; },
  reinstate(b) {
    const r = A(b);
    const o = { dev: r.bytes(16), first: r.u32(), n: r.u16(), stored: b.subarray(16) };
    if (o.n < 1 || o.n > 256 || r.left() !== 96 * o.n) fail('relay/bad_args');
    return o;
  },
  fence(b) {
    const r = A(b);
    const host = r.bytes(r.u8());
    const o = { host: String.fromCharCode(...host), door: r.bytes(5), homeGen: r.u32() };
    r.end();
    return o;
  },
  seed(b) {
    const r = A(b);
    // A u64 above 2^53 - 1 is read as null and refused by the check that names the field (RELAY-GEN-008).
    const o = { homeGen: r.u32(), first: r.u64OrNull(), chainBefore: r.bytes(32), last: r.u64OrNull(), chainLast: r.bytes(32), blob: r.bytes(16), entries: [] };
    const n = r.u16();
    if (n < 1 || n > 320 || r.left() !== 58 * n) fail('relay/bad_args');
    for (let i = 0; i < n; i++) o.entries.push({ dev: r.bytes(16), pk: r.bytes(32), role: r.u8(), status: r.u8(), last_ctr: r.u64OrNull() });
    return o;
  },
  compact(b) {
    const r = A(b);
    const o = { below: r.u64OrNull(), snapSeq: r.u64OrNull(), blob: r.bytes(16), keep: [] };
    const n = r.u32();
    if (n > 131072 || r.left() !== 8 * n) fail('relay/bad_args');
    for (let i = 0; i < n; i++) o.keep.push(r.u64OrNull());
    return o;
  },
  restore_tail(b) {
    const r = A(b);
    const o = { first: r.u64OrNull(), hashes: [] };
    const n = r.u32();
    if (n < 1 || n > 64 || r.left() !== 32 * n) fail('relay/bad_args');
    for (let i = 0; i < n; i++) o.hashes.push(r.bytes(32));
    return o;
  },
  purge(b) { const r = A(b); const o = { before: r.u64OrNull(), epoch: r.u32() }; r.end(); return o; },
  mint_read_token(b) { const r = A(b); const o = { dev: r.bytes(16), epoch: r.u32() }; r.end(); return o; },
  revoke_read_token(b) { const r = A(b); const o = { dev: r.bytes(16) }; r.end(); return o; },
  recovery_register(b) { const r = A(b); const o = { dev: r.bytes(16), pk: r.bytes(32), hash: r.bytes(32) }; r.end(); return o; },
};

const isFrameSize = (n) => n >= 670 && n <= 262144;
function extraOf(op, x, a) {
  const bad = () => fail('relay/bad_body');
  if (op === 'register') {
    if (x.length === 8) return { seq: readU64(x, 0) };
    if (!isFrameSize(x.length)) bad();
    return { frame: x };
  }
  if (op === 'fence') {
    if (x.length && !isFrameSize(x.length)) bad();
    return x.length ? { frame: x } : {};
  }
  if (op === 'recovery_register') {
    if (x.length !== 8) bad();
    return { seq: readU64(x, 0) };
  }
  if (op === 'restore_tail') {
    const r = new Reader(x, 'relay/bad_body');
    const o = { chainBefore: r.bytes(32), frames: [] };
    let total = 0;
    for (let i = 0; i < a.hashes.length; i++) {
      const f = r.bytes(r.u32());
      total += f.length;
      if (total > MIB) bad();
      o.frames.push(f);
    }
    r.end();
    return o;
  }
  if (x.length) bad();
  return {};
}

function parseAdmin(op, body) {
  const r = new Reader(body, 'relay/bad_body');
  const p = { signer: r.bytes(16), nonce: r.bytes(32), sig: r.bytes(64) };
  p.args = r.bytes(r.u32());
  const extra = r.bytes(r.u32());
  r.end();
  p.a = ARGS[op](p.args);
  p.x = extraOf(op, extra, p.a);
  return p;
}

// ---------- the board (§7 to §16)

export class BoardCore {
  constructor({ storage, sockets, now, random, config, board }) {
    this.s = storage;
    this.sockets = sockets || noSockets;
    this.clock = now || Date.now;
    this.random = random || ((n) => crypto.getRandomValues(new Uint8Array(n)));
    this.config = config;
    this.board = board || null;
  }

  migrate() {
    for (const q of SQL.schema) this.s.run(q);
  }

  // One request of §10.1 that passed the gate: a Reply, or { socket: true, reply } for an accepted upgrade.
  async handle(req) {
    const now = this.clock(); // once per request (RELAY-OBJ-005)
    const board = fromHex(req.params.board);
    if (!this.board) this.board = board;
    else if (!ctEqual(this.board, board)) throw new Error('request for another board');
    try {
      const h = HANDLERS[req.route];
      if (!h) fail('relay/not_found');
      return await h(this, req, now);
    } catch (e) {
      if (e instanceof RelayError) return errorReply(e, 'object');
      throw e;
    }
  }

  // ----- storage calls: a throw is a refused write (RELAY-BUDGET-003)

  all(q, ...p) {
    try { return this.s.all(q, ...p); } catch { return fail('relay/storage_refused'); }
  }
  run(q, ...p) {
    try { return this.s.run(q, ...p); } catch { return fail('relay/storage_refused'); }
  }
  one(q, ...p) {
    return this.all(q, ...p)[0] || null;
  }
  // The one transaction of a request (§8.4). Inside it, a thrown RelayError rolls
  // everything back; a returned one commits the writes made so far (the nonce
  // deletion, a counted try, lazy cleanups) and is then the refusal.
  tx(fn) {
    let bug = null, out;
    try {
      out = this.s.transaction(() => {
        try { return fn(); } catch (e) { if (!(e instanceof RelayError)) bug = e; throw e; }
      });
    } catch (e) {
      if (e instanceof RelayError) throw e;
      if (bug) throw bug;
      return fail('relay/storage_refused');
    }
    if (out instanceof RelayError) throw out;
    return out;
  }

  // ----- rows every route needs

  boardRow() {
    return this.one(SQL.boardGet);
  }
  head(board) {
    const r = this.one(SQL.logHead);
    return r ? { seq: r.seq, chain: r.chain, at: r.at } : { seq: board.base_seq, chain: board.base_chain, at: board.created_at };
  }
  // §8.2: the token's row, its conditions in the order the table lists them.
  credRow(cred, anyGen) {
    if (cred.cred !== 'token') return null;
    if (cred.role === 1) {
      const d = this.one(SQL.devGet, cred.dev);
      if (!d || d.role !== 1) fail('relay/unauthorized');
      if (d.state !== 1) fail('relay/revoked');
      if (!anyGen && d.gen !== cred.gen) fail('relay/token_stale');
      return d;
    }
    const t = this.one(SQL.rtGet, cred.dev);
    if (!t) fail('relay/unauthorized');
    if (t.state !== 1 || t.gen !== cred.gen) fail('relay/token_stale');
    return t;
  }
  // §8.1: the board state must be one of `states` (0 empty, 1 open, 2 fenced, 3 seeding).
  need(board, cred, ...states) {
    const st = board ? board.state : 0;
    if (states.includes(st)) return;
    if (st === 0) fail('relay/no_board');
    if (st === 3) fail('relay/seeding');
    if (cred.cred === 'create') fail('relay/exists');
    if (st === 2) fail('relay/fenced');
    fail('relay/target');
  }
  doorHex() {
    return hex(door(this.config.tokenKey, this.board));
  }
  token(role, dev, gen) {
    return makeToken(this.config.tokenKey, role, this.board, dev, gen);
  }

  // ----- nonces (§10.3, §12.1)

  issueNonce(purpose, dev, now) {
    this.run(SQL.nonceExpire, now - 120000);
    const n = this.one(SQL.nonceCount).n;
    if (n >= 64) this.run(SQL.nonceTrim, n - 63);
    const nonce = this.random(32);
    this.run(SQL.nonceInsert, nonce, purpose, dev, now);
    return nonce;
  }
  // The unexpired nonce row of this purpose and device, compared in constant time.
  findNonce(nonce, purpose, dev, now) {
    let hit = null;
    for (const n of this.all(SQL.nonceFor, purpose, dev)) if (ctEqual(n.nonce, nonce)) hit = n;
    if (!hit || hit.at < now - 120000) fail('relay/no_nonce');
    return hit;
  }

  // ----- sequencing (§9)

  // P1 to P7 of §9.3 for each frame until one fails. `cred` null is the tail
  // form of §12.7 and §12.9: any registered author, of any role and state.
  async precheck(frames, cred, own) {
    const ok = [], rows = new Map();
    if (own) rows.set(hex(own.dev), own);
    for (let i = 0; i < frames.length; i++) {
      const frame = frames[i];
      const f = frameHead(frame, this.board);
      let code = f.code, a = null;
      if (!code) {
        const key = hex(f.dev);
        a = rows.get(key);
        if (a === undefined) { a = this.one(SQL.devGet, f.dev); rows.set(key, a); }
        if (!a) code = 'relay/unknown_author';
        else if (cred && a.role !== 2 && !(cred.cred === 'token' && ctEqual(f.dev, cred.dev))) code = 'relay/not_author';
        else if (cred && a.state !== 1) code = 'relay/revoked';
        else if (!(await verifyStrict(a.sign_pk, f.signed, f.sig))) code = 'relay/bad_signature';
      }
      if (code) return { ok, stop: { index: i, code } };
      ok.push({ frame, dev: f.dev, key: hex(f.dev), ctr: f.ctr, pk: a.sign_pk, hash: frameHash(frame) });
    }
    return { ok, stop: null };
  }
  // The author row of a frame inside the transaction, with its running counters.
  author(authors, f) {
    let a = authors.get(f.key);
    if (a === undefined) {
      const row = this.one(SQL.devGet, f.dev);
      a = row ? { row, last_ctr: row.last_ctr, last_seq: row.last_seq, app_day: row.app_day, app_count: row.app_count, dirty: false } : null;
      authors.set(f.key, a);
    }
    return a;
  }
  // T1 to T4 of §9.4 for one frame, writing nothing: t = 0 means "store it".
  decide(board, f, a, day) {
    if (!a || a.row.state !== 1 || !ctEqual(a.row.sign_pk, f.pk)) return refused('relay/revoked');
    if (f.ctr <= a.last_ctr) {
      const row = this.one(SQL.logByCtr, f.dev, f.ctr);
      if (!row || row.seq < board.purge_floor) return { t: 2, seq: 0, frame: EMPTY };
      if (ctEqual(row.hash, f.hash)) return { t: 1, seq: row.seq, chain: row.chain };
      return { t: 2, seq: row.seq, frame: this.one(SQL.logFrame, row.seq).frame };
    }
    if (f.ctr > a.last_ctr + 1) return { t: 3, last_ctr: a.last_ctr, last_seq: a.last_seq };
    if (a.row.role === 1 && (a.app_day === day ? a.app_count : 0) >= 3000) return refused('relay/budget');
    return { t: 0 };
  }
  // T5 of §9.4: the log row at head + 1; the author row is written by flush().
  store(f, a, head, now, day, budget) {
    const seq = head.seq + 1, at = Math.max(now, head.at), chain = chainStep(head.chain, seq, f.hash);
    this.run(SQL.logInsert, seq, f.dev, f.ctr, f.hash, chain, at, f.frame);
    a.last_ctr = f.ctr; a.last_seq = seq; a.dirty = true;
    if (budget && a.row.role === 1) { a.app_count = (a.app_day === day ? a.app_count : 0) + 1; a.app_day = day; }
    return { seq, chain, at, frame: f.frame };
  }
  flush(authors) {
    for (const a of authors.values()) if (a && a.dirty) this.run(SQL.devCounters, a.last_ctr, a.last_seq, a.app_day, a.app_count, a.row.dev);
  }
  // §9.4 for a batch whose frames passed §9.3 up to pre.stop.
  sequence(board, pre, total, now, own) {
    const results = [], stored = [], authors = new Map(), day = Math.floor(now / DAY);
    if (own) authors.set(hex(own.dev), { row: own, last_ctr: own.last_ctr, last_seq: own.last_seq, app_day: own.app_day, app_count: own.app_count, dirty: false });
    let head = this.head(board), stopped = false;
    for (const f of pre.ok) {
      const a = this.author(authors, f);
      const r = this.decide(board, f, a, day);
      if (r.t === 0) {
        const row = this.store(f, a, head, now, day, true);
        head = row;
        stored.push(row);
        results.push({ t: 0, seq: row.seq, chain: row.chain });
      } else {
        results.push(r);
        if (r.t !== 1) { stopped = true; break; }
      }
    }
    if (!stopped && pre.stop) results.push(refused(pre.stop.code));
    while (results.length < total) results.push({ t: 5 });
    this.flush(authors);
    return { results, stored };
  }
  // `own` is the token's device row the caller has just read (none for the door).
  async appendBatch(cred, frames, now, own) {
    const pre = await this.precheck(frames, cred, own);
    const out = this.tx(() => {
      const row = this.credRow(cred, false);
      const board = this.boardRow();
      this.need(board, cred, 1);
      return this.sequence(board, pre, frames.length, now, row);
    });
    out.status = resultsStatus(out.results);
    return out;
  }
  // The frame a register or fence request carries (§12.3, §12.6): stored when its
  // outcome is new; a dup is returned as it is; anything else refuses the request.
  carried(board, pre, now) {
    if (pre.stop) fail(pre.stop.code);
    const f = pre.ok[0], authors = new Map(), day = Math.floor(now / DAY);
    const a = this.author(authors, f);
    const r = this.decide(board, f, a, day);
    if (r.t === 1) return { dup: true, seq: r.seq, chain: r.chain, a };
    if (r.t === 2 || r.t === 3) fail(r.t === 2 ? 'relay/conflict' : 'relay/gap', { last_ctr: a.last_ctr, last_seq: a.last_seq });
    if (r.t === 4) fail(r.code, r.code === 'relay/budget' ? { until: (day + 1) * DAY } : undefined);
    return { dup: false, f, a, authors, day };
  }
  // The tails of §12.7 and §12.9: every frame has a registered author, a strict
  // signature and its author's next counter, or nothing is written.
  tail(pre, total, now, head, before, atEnd) {
    const authors = new Map(), plan = [];
    let cur = head;
    for (let i = 0; i < total; i++) {
      if (before) before(i);
      if (pre.stop && pre.stop.index === i) fail(pre.stop.code, { index: i });
      const f = pre.ok[i];
      const a = this.author(authors, f);
      if (!a) fail('relay/unknown_author', { index: i });
      if (!ctEqual(a.row.sign_pk, f.pk)) fail('relay/bad_signature', { index: i });
      if (f.ctr !== a.last_ctr + 1) fail(f.ctr > a.last_ctr + 1 ? 'relay/gap' : 'relay/conflict', { index: i });
      const seq = cur.seq + 1;
      cur = { seq, chain: chainStep(cur.chain, seq, f.hash), at: Math.max(now, cur.at), f };
      a.last_ctr = f.ctr; a.last_seq = seq; a.dirty = true;
      plan.push(cur);
    }
    if (atEnd) atEnd(cur);
    for (const s of plan) this.run(SQL.logInsert, s.seq, s.f.dev, s.f.ctr, s.f.hash, s.chain, s.at, s.f.frame);
    this.flush(authors);
    return { head: cur, stored: plan.map((s) => ({ seq: s.seq, at: s.at, frame: s.f.frame })) };
  }

  // ----- pages (§10.5)

  readPage(board, after, limit, now) {
    const head = this.head(board), rows = [];
    let bytes = 0, cursor = after;
    while (rows.length < limit) {
      // As many rows as surely fit, and one more that may not: at most one row is read in vain.
      const want = Math.min(limit - rows.length, Math.floor((MIB - bytes) / 262144) + 1);
      const got = this.all(SQL.logPage, cursor, board.purge_floor, want);
      let stop = got.length < want;
      for (const r of got) {
        if (rows.length && bytes + r.frame.length > MIB) { stop = true; break; }
        rows.push(r); bytes += r.frame.length; cursor = r.seq;
      }
      if (stop) break;
    }
    const more = rows.length > 0 && rows[rows.length - 1].seq < head.seq;
    return encodePage(board, head, now, rows, more, after + 1 < Math.max(board.compact_floor, board.purge_floor));
  }

  // ----- blobs (§10.7, §13)

  deleteBlob(b) {
    this.run(SQL.partsDelete, b.slot * 64, b.slot * 64 + 63);
    this.run(SQL.blobDelete, b.id);
  }
  // §13.1, in the transaction of the blob-put that completed `done`.
  retain(board, done) {
    if (done.purpose !== 1 && done.purpose !== 4) return;
    const all = this.all(SQL.blobAll).filter((b) => b.complete === 1 && b.purpose === done.purpose);
    if (done.purpose === 1) {
      all.sort((x, y) => y.seq - x.seq || y.at - x.at || cmp(x.id, y.id));
      const authors = new Set();
      all.forEach((b, i) => {
        const a = hex(b.author);
        const keep = i < 3 || !authors.has(a) || (board.compact_snap && ctEqual(board.compact_snap, b.id));
        authors.add(a);
        if (!keep) this.deleteBlob(b);
      });
    } else {
      const mine = all.filter((b) => ctEqual(b.author, done.author));
      mine.sort((x, y) => y.at - x.at || cmp(x.id, y.id));
      for (const b of mine.slice(2)) this.deleteBlob(b);
    }
  }

  // ----- tickets (§10.8)

  // The unexpired ticket, or null: an expired one is deleted here (RELAY-PAIR-001).
  ticket(tid, now) {
    const t = this.one(SQL.ticketGet, tid);
    if (t && t.expires <= now) { this.run(SQL.ticketDelete, tid); return null; }
    return t;
  }

  // ----- sockets (§11)

  send(socket, bytes) {
    try { this.sockets.send(socket, bytes); } catch { /* the client catches up by reading (§11.4) */ }
  }
  close(socket, code, reason) {
    try { this.sockets.close(socket, code, reason); } catch { /* already closed */ }
  }
  closeSockets(dev, when, code) {
    for (const s of this.sockets.list(dev)) {
      const a = this.sockets.attachment(s);
      if (a && when(a)) this.close(s, code, 'credential');
    }
  }
  ready(dev) {
    return this.sockets.list(dev).filter((s) => { const a = this.sockets.attachment(s); return a && a.ready === true; });
  }
  // After a transaction stored new entries: exactly those, in seq order, to every ready socket (§11.4).
  push(stored, now) {
    if (!stored.length) return;
    const socks = this.ready();
    if (!socks.length) return;
    const board = this.boardRow(), head = this.head(board), msgs = [];
    for (let i = 0; i < stored.length;) {
      let j = i, bytes = 0;
      while (j < stored.length && j - i < 256 && (j === i || bytes + stored[j].frame.length <= MIB)) bytes += stored[j++].frame.length;
      msgs.push(cat(u8(0x84), encodePage(board, head, now, stored.slice(i, j), j < stored.length, false)));
      i = j;
    }
    for (const s of socks) for (const m of msgs) this.send(s, m);
  }
  fencePart(board) {
    return cat(vb8(te.encode(board.fence_host)), board.fence_door, u32(board.home_gen));
  }
  welcome(dev, now) {
    const board = this.boardRow(), d = this.one(SQL.devGet, dev);
    if (!board || !d) return null;
    const head = this.head(board), fenced = board.state === 2;
    return cat(u8(0x81), u64(head.seq), head.chain, u64(d.last_ctr), u64(d.last_seq), u64(now),
      u64(Math.max(board.compact_floor, board.purge_floor)), u64(board.purge_floor), u8(fenced ? 2 : 1), u8(1),
      u32(appendsLeft(d, now)), door(this.config.tokenKey, this.board), u8(fenced ? 1 : 0), fenced ? this.fencePart(board) : EMPTY);
  }
  sockError(socket, req, e) {
    this.send(socket, cat(u8(0x88), u32(req), u16(e.status), vb8(te.encode(e.code))));
  }

  // A socket message (§11.3, §11.4). `data` is a Uint8Array, or a string for a text message.
  async onMessage(socket, data) {
    const now = this.clock();
    const att = this.sockets.attachment(socket) || {};
    if (data.length > 4194378) return this.close(socket, 1009, 'too big');
    if (typeof data === 'string') {
      if (data === 'ping') return undefined; // answered by the runtime (§11.5)
      if (att.ready) this.sockError(socket, 0, E('relay/bad_message'));
      return this.close(socket, 4004, 'protocol');
    }
    if (!this.board) {
      const b = this.boardRow();
      if (!b) return this.close(socket, 4004, 'protocol');
      this.board = new Uint8Array(b.board_id);
    }
    const type = data[0];
    if (!att.ready) { // the first message must be a hello; nothing else is answered (RELAY-SOCK-004)
      if (type !== 0x01 || data.length !== 21) return this.close(socket, 4004, 'protocol');
      return this.onHello(socket, att, data, now);
    }
    if (type === 0x01 && data.length === 21) return this.close(socket, 4004, 'protocol'); // a second hello
    try {
      if (type === 0x02 && data.length >= 5) return await this.onAppend(socket, att, data, now);
      if (type === 0x03 && data.length === 15) return this.onRead(socket, att, data, now);
    } catch (e) {
      if (!(e instanceof RelayError)) throw e;
      this.sockError(socket, readU32(data, 1), e);
      if (e.code === 'relay/revoked' || e.code === 'relay/token_stale' || e.code === 'relay/unauthorized') this.close(socket, 4003, 'credential');
      return undefined;
    }
    this.sockError(socket, 0, E('relay/bad_message'));
    return this.close(socket, 4004, 'protocol');
  }
  onHello(socket, att, data, now) {
    const dev = fromHex(att.dev), session = data.subarray(1, 17), sessionHex = hex(session);
    for (const other of this.sockets.list(dev)) {
      const oa = this.sockets.attachment(other);
      if (!oa || !oa.session) continue; // this socket, and any other that has not said hello yet
      if (oa.session === sessionHex) this.close(other, 4002, 'same session');
      else {
        this.send(other, cat(u8(0x86), session));
        this.close(other, 4001, 'displaced');
        this.send(socket, cat(u8(0x85), fromHex(oa.session)));
      }
    }
    const welcome = this.welcome(dev, now);
    if (!welcome) return this.close(socket, 4003, 'credential');
    this.sockets.setAttachment(socket, { dev: att.dev, gen: att.gen, session: sessionHex, features: readU32(data, 17), ready: true });
    return this.send(socket, welcome);
  }
  sockCred(att) {
    const cred = { cred: 'token', role: 1, dev: fromHex(att.dev), gen: att.gen };
    this.credRow(cred, false); // the device row again (RELAY-SOCK-007)
    return cred;
  }
  async onAppend(socket, att, data, now) {
    const cred = { cred: 'token', role: 1, dev: fromHex(att.dev), gen: att.gen };
    const own = this.credRow(cred, false); // the device row again (RELAY-SOCK-007)
    this.need(this.boardRow(), cred, 1);
    const out = await this.appendBatch(cred, parseBatch(data.subarray(5)), now, own);
    this.send(socket, cat(u8(0x82), data.subarray(1, 5), encodeResults(out.results)));
    this.push(out.stored, now); // after the result (RELAY-SOCK-012)
  }
  onRead(socket, att, data, now) {
    const cred = this.sockCred(att);
    const board = this.boardRow();
    this.need(board, cred, 1, 2);
    const after = readU64(data, 5), limit = (data[13] << 8) | data[14];
    if (after === null || limit < 1 || limit > 256) fail('relay/bad_query');
    this.send(socket, cat(u8(0x83), data.subarray(1, 5), this.readPage(board, after, limit, now)));
  }
  onClose() {
    // Nothing to forget: every state is in SQLite or in the attachments.
  }
}

function appendsLeft(d, now) {
  return 3000 - (d.app_day === Math.floor(now / DAY) ? d.app_count : 0);
}
function exact(body, n) {
  if (body.length !== n) fail('relay/bad_body');
  return body;
}

// ---------- the routes (§10, §12)

const HANDLERS = {
  // §10.2
  async create(c, req, now) {
    const r = new Reader(req.body, 'relay/bad_body');
    const rsign = r.bytes(32), salt = r.bytes(16), dev = r.bytes(16), signPk = r.bytes(32), flen = r.u32();
    if (flen !== r.left()) fail('relay/bad_body');
    const frame = r.bytes(flen), fh = frameHash(frame);
    // Checks 2 to 5, each `relay/bad_genesis`; they count only on an empty board.
    const devR = recoveryDev(c.board, rsign), f = frameHead(frame, c.board);
    const good = ctEqual(H(ctx('board'), rsign, salt).subarray(0, 16), c.board) && !f.code && ctEqual(f.dev, devR) && f.ctr === 1 &&
      (await verifyStrict(rsign, f.signed, f.sig)) && !ctEqual(dev, devR) && !ctEqual(signPk, rsign);
    return c.tx(() => {
      const board = c.boardRow();
      if (!board) {
        if (!good) fail('relay/bad_genesis');
        const c0 = chainZero(c.board), c1 = chainStep(c0, 1, fh);
        c.run(SQL.boardInsert, c.board, 1, now, 0, c0, 1, 1, 1, 1, null, null, null);
        c.run(SQL.devInsert, devR, rsign, 2, 1, 0, 1, 1, now);
        c.run(SQL.devInsert, dev, signPk, 1, 1, 1, 0, 0, now);
        c.run(SQL.logInsert, 1, devR, 1, fh, c1, now, frame);
        return json(200, { seq: 1, chain: hex(c1), token: c.token(1, dev, 1), door: c.doorHex() });
      }
      if (board.state === 3) fail('relay/seeding');
      // Repeat: the same Genesis frame and first device, on an open board.
      const row1 = board.state === 1 && board.purge_floor <= 1 ? c.one(SQL.logAt, 1) : null;
      const d = c.one(SQL.devGet, dev);
      if (!row1 || !ctEqual(row1.hash, fh) || !d || d.role !== 1 || d.state !== 1 || !ctEqual(d.sign_pk, signPk)) fail('relay/exists');
      if (d.gen >= MAXGEN) fail('relay/gen_exhausted');
      c.run(SQL.devGen, d.gen + 1, dev);
      return json(200, { seq: 1, chain: hex(row1.chain), token: c.token(1, dev, d.gen + 1), door: c.doorHex() });
    });
  },

  // §10.3 step 1
  'login-nonce'(c, req, now) {
    const dev = exact(req.body, 16);
    return c.tx(() => {
      const own = c.credRow(req.cred, true);
      c.need(c.boardRow(), req.cred, 1, 2);
      if (own && !ctEqual(dev, req.cred.dev)) fail('relay/forbidden');
      const d = own || c.one(SQL.devGet, dev);
      if (!d || d.role !== 1) fail('relay/unauthorized');
      if (d.state !== 1) fail('relay/revoked');
      if (d.gen >= MAXGEN) fail('relay/gen_exhausted');
      const nonce = c.issueNonce(1, dev, now);
      return json(200, { nonce: hex(nonce), gen: d.gen + 1, expires: now + 120000 });
    });
  },

  // §10.3 step 2
  async login(c, req, now) {
    const r = new Reader(exact(req.body, 148), 'relay/bad_body');
    const dev = r.bytes(16), gen = r.u32(), nonce = r.bytes(32), clientNonce = r.bytes(32), sig = r.bytes(64);
    const upToNonce = () => {
      const own = c.credRow(req.cred, true);
      c.need(c.boardRow(), req.cred, 1, 2);
      if (own && !ctEqual(dev, req.cred.dev)) fail('relay/forbidden');
      return c.findNonce(nonce, 1, dev, now);
    };
    upToNonce();
    const d0 = c.one(SQL.devGet, dev);
    const sigOk = !!d0 && (await verifyStrict(d0.sign_pk, loginMessage(c.config.origin, c.board, dev, gen, nonce, clientNonce), sig));
    const d = c.tx(() => {
      const n = upToNonce();
      c.run(SQL.nonceDelete, n.nonce); // tried once, whatever follows (RELAY-LOGIN-002)
      const row = c.one(SQL.devGet, dev);
      if (!row || row.role !== 1 || row.state !== 1) return E('relay/revoked');
      if (gen !== row.gen + 1) return E('relay/bad_gen');
      if (!sigOk || !ctEqual(row.sign_pk, d0.sign_pk)) return E('relay/bad_login_sig');
      c.run(SQL.devGen, gen, dev);
      return row;
    });
    c.closeSockets(dev, (a) => a.gen < gen, 4003);
    const out = { token: c.token(1, dev, gen), gen, push: c.config.push };
    if (d.wraps) {
      const w = d.wraps, n = (w[4] << 8) | w[5], wraps = [];
      for (let i = 0; i < n; i++) wraps.push(hex(w.subarray(6 + 96 * i, 102 + 96 * i)));
      out.wraps = { first_epoch: readU32(w, 0), wraps };
    }
    return json(200, out);
  },

  // §10.4
  hello(c, req, now) {
    const row = c.credRow(req.cred, false);
    const board = c.boardRow();
    c.need(board, req.cred, 1, 2);
    const head = c.head(board);
    const o = { head: { seq: head.seq, chain: hex(head.chain) } };
    if (req.cred.role === 1) { o.last_ctr = row.last_ctr; o.last_seq = row.last_seq; o.appends_left = appendsLeft(row, now); }
    o.relay_now = now;
    o.floor = Math.max(board.compact_floor, board.purge_floor);
    o.purge_floor = board.purge_floor;
    o.state = board.state === 2 ? 'fenced' : 'open';
    if (board.state === 2) o.fence = { host: board.fence_host, door: hex(board.fence_door), home_gen: board.home_gen };
    o.door = c.doorHex();
    o.push = c.config.push;
    return json(200, o);
  },

  // §11.1
  socket(c, req) {
    c.credRow(req.cred, false);
    c.need(c.boardRow(), req.cred, 1, 2);
    const { response } = c.sockets.accept(req.cred.dev, { dev: hex(req.cred.dev), gen: req.cred.gen, features: 0, ready: false });
    return { socket: true, reply: response };
  },

  // §10.5
  read(c, req, now) {
    c.credRow(req.cred, false);
    const board = c.boardRow();
    c.need(board, req.cred, 1, 2);
    return bin(200, c.readPage(board, req.query.after, req.query.limit, now));
  },

  // §10.6
  async append(c, req, now) {
    const own = c.credRow(req.cred, false);
    c.need(c.boardRow(), req.cred, 1);
    const out = await c.appendBatch(req.cred, parseBatch(req.body), now, own);
    c.push(out.stored, now);
    return bin(out.status, encodeResults(out.results));
  },

  // §10.7
  'blob-list'(c, req) {
    c.credRow(req.cred, false);
    c.need(c.boardRow(), req.cred, 1, 2);
    const done = c.all(SQL.blobAll).filter((b) => b.complete === 1);
    done.sort((x, y) => x.purpose - y.purpose || y.seq - x.seq || y.at - x.at || cmp(x.id, y.id));
    return json(200, { blobs: done.slice(0, 64).map(blobJson) });
  },
  'blob-begin'(c, req, now) {
    const b = exact(req.body, 65);
    const id = b.subarray(0, 16), purpose = b[16], seq = readU64(b, 17), size = readU64(b, 25), sha = b.subarray(33, 65);
    return c.tx(() => {
      c.credRow(req.cred, false);
      const board = c.boardRow();
      c.need(board, req.cred, 1);
      const blobs = [];
      for (const old of c.all(SQL.blobAll)) { // §13.2, kept whatever the checks decide
        if (old.at <= now - 3600000 && (old.complete !== 1 || old.purpose === 2)) c.deleteBlob(old); else blobs.push(old);
      }
      if (size === null || size < 67 || size > 67108864 || seq === null) return E('relay/bad_blob');
      const headSeq = purpose === 1 || purpose === 2 ? c.head(board).seq : 0;
      if (purpose === 1 ? seq < 64 || seq % 64 !== 0 || seq > headSeq
        : purpose === 2 ? seq < 1 || seq > headSeq
          : purpose === 3 ? seq !== 1
            : purpose === 4 ? seq !== 0 : true) return E('relay/bad_blob');
      if (purpose === 3 && blobs.some((x) => x.purpose === 3 && !ctEqual(x.id, id))) return E('relay/blob_exists');
      const parts = Math.ceil(size / MIB);
      const same = blobs.find((x) => ctEqual(x.id, id));
      if (same) {
        const repeat = ctEqual(same.author, req.cred.dev) && same.purpose === purpose && same.seq === seq && same.size === size && ctEqual(same.sha256, sha);
        return repeat ? json(200, { id: hex(id), parts }) : E('relay/blob_exists');
      }
      if (blobs.filter((x) => x.complete !== 1).length >= 3 || blobs.reduce((n, x) => n + x.size, 0) + size > 805306368) return E('relay/blob_quota');
      c.run(SQL.blobInsert, id, 1 + blobs.reduce((m, x) => Math.max(m, x.slot), 0), purpose, seq, req.cred.dev, size, parts, sha, now);
      return json(200, { id: hex(id), parts });
    });
  },
  'blob-meta'(c, req) {
    c.credRow(req.cred, false);
    c.need(c.boardRow(), req.cred, 1, 2);
    const b = c.one(SQL.blobGet, fromHex(req.params.id));
    if (!b) fail('relay/no_blob');
    return json(200, blobJson(b));
  },
  'blob-put'(c, req) {
    const id = fromHex(req.params.id), i = Number(req.params.part), data = req.body;
    return c.tx(() => {
      c.credRow(req.cred, false);
      const board = c.boardRow();
      c.need(board, req.cred, 1);
      const b = c.one(SQL.blobGet, id);
      if (!b) fail('relay/no_blob');
      if (!ctEqual(b.author, req.cred.dev)) fail('relay/forbidden');
      if (i >= b.parts || data.length !== (i < b.parts - 1 ? MIB : b.size - (b.parts - 1) * MIB)) fail('relay/bad_blob');
      const pk = b.slot * 64 + i;
      const old = c.one(SQL.partGet, pk);
      if (old) {
        if (!ctEqual(old.data, data)) fail('relay/part_conflict');
        return json(200, { received: b.received, complete: b.complete === 1 });
      }
      c.run(SQL.partInsert, pk, data);
      const received = b.received + 1, complete = received === b.parts;
      c.run(SQL.blobProgress, received, complete ? 1 : 0, id);
      if (complete) c.retain(board, b);
      return json(200, { received, complete });
    });
  },
  'blob-get'(c, req) {
    c.credRow(req.cred, false);
    c.need(c.boardRow(), req.cred, 1, 2);
    const b = c.one(SQL.blobGet, fromHex(req.params.id)), i = Number(req.params.part);
    if (!b) fail('relay/no_blob');
    if (b.complete !== 1) fail('relay/blob_incomplete');
    if (i >= b.parts) fail('relay/bad_blob');
    return bin(200, c.one(SQL.partGet, b.slot * 64 + i).data);
  },

  // §10.8
  'pair-open'(c, req, now) {
    const b = exact(req.body, 100);
    const tid = b.subarray(0, 16), a = b.subarray(16);
    return c.tx(() => {
      c.credRow(req.cred, false);
      c.need(c.boardRow(), req.cred, 1);
      if (!slotOk('a', a)) fail('relay/bad_slot');
      const live = [];
      for (const t of c.all(SQL.ticketAll)) {
        if (t.expires <= now) c.run(SQL.ticketDelete, t.tid); else live.push(t);
      }
      const same = live.find((t) => ctEqual(t.tid, tid));
      if (same) return ctEqual(same.creator, req.cred.dev) && ctEqual(same.a, a) ? json(200, { expires: same.expires }) : E('relay/ticket_exists');
      if (live.length >= 4) return E('relay/ticket_cap');
      c.run(SQL.ticketInsert, tid, req.cred.dev, now + 600000, a);
      return json(200, { expires: now + 600000 });
    });
  },
  'slot-post'(c, req, now) {
    const tid = fromHex(req.params.tid), label = req.params.label, body = req.body;
    let notify = null;
    const out = c.tx(() => {
      c.credRow(req.cred, false);
      c.need(c.boardRow(), req.cred, 1);
      const t = c.ticket(tid, now);
      if (!t) return E('relay/no_ticket');
      if (label !== 'b' && !ctEqual(t.creator, req.cred.dev)) fail('relay/forbidden');
      const tries = t.tries + 1;
      if (label === 'b' && tries >= 5) { // the fifth try burns the ticket
        c.run(SQL.ticketDelete, tid);
        return E('relay/tries');
      }
      const before = label === 'b' ? t.a : label === 'c' ? t.b : t.c, stored = t[label];
      const verdict = !slotOk(label, body) ? E('relay/bad_slot')
        : stored ? (ctEqual(stored, body) ? null : E('relay/slot_order'))
          : before ? 'store' : E('relay/slot_order');
      if (verdict === 'store') {
        if (label === 'b') { c.run(SQL.ticketSetB, tries, body, tid); notify = t.creator; }
        else c.run(label === 'c' ? SQL.ticketSetC : SQL.ticketSetD, body, tid);
      } else if (label === 'b') c.run(SQL.ticketTries, tries, tid); // the try stays counted
      return verdict instanceof RelayError ? verdict : json(200, {});
    });
    if (notify) for (const s of c.ready(notify)) c.send(s, cat(u8(0x87), tid, u8(0x42)));
    return out;
  },
  'slot-get'(c, req, now) {
    const tid = fromHex(req.params.tid), label = req.params.label;
    return c.tx(() => {
      c.credRow(req.cred, false);
      c.need(c.boardRow(), req.cred, 1);
      const t = c.ticket(tid, now);
      if (!t) return E('relay/no_ticket');
      if (label === 'b' && !ctEqual(t.creator, req.cred.dev)) fail('relay/forbidden');
      if (!t[label]) fail('relay/slot_empty');
      return bin(200, t[label]);
    });
  },
  'pair-close'(c, req, now) {
    const tid = fromHex(req.params.tid);
    return c.tx(() => {
      c.credRow(req.cred, false);
      c.need(c.boardRow(), req.cred, 1);
      const t = c.ticket(tid, now);
      if (!t) return E('relay/no_ticket');
      if (!ctEqual(t.creator, req.cred.dev)) fail('relay/forbidden');
      c.run(SQL.ticketDelete, tid);
      return json(200, {});
    });
  },

  // §12.1
  'admin-nonce'(c, req, now) {
    const signer = exact(req.body, 16), cred = req.cred;
    return c.tx(() => {
      const own = c.credRow(cred, false);
      const board = c.boardRow();
      if (cred.cred === 'create') c.need(board, cred, 0, 3);
      else {
        c.need(board, cred, 1);
        if (!(own && ctEqual(signer, cred.dev))) {
          const row = c.one(SQL.devGet, signer);
          if (!row) fail('relay/unauthorized');
          if (row.role !== 2) fail('relay/forbidden');
          if (row.state !== 1) fail('relay/revoked');
        }
      }
      const nonce = c.issueNonce(2, signer, now);
      return json(200, { nonce: hex(nonce), expires: now + 120000 });
    });
  },

  // §12.1 to §12.12
  async admin(c, req, now) {
    const op = req.params.op, cred = req.cred, spec = OPS.get(op);
    c.credRow(cred, false); // the token's row before any other check (RELAY-OBJ-003)
    const p = parseAdmin(op, req.body); // A1
    const upToNonce = () => {
      c.credRow(cred, false);
      const board = c.boardRow();
      if (op === 'seed') c.need(board, cred, 0, 3); else c.need(board, cred, 1); // A2
      let signer = null; // A3
      if (op === 'seed') {
        if (cred.cred !== 'create') fail('relay/forbidden');
      } else {
        signer = c.one(SQL.devGet, p.signer);
        const own = cred.cred === 'token' && ctEqual(p.signer, cred.dev);
        if (cred.cred === 'create' || (!own && !(signer && signer.role === 2))) fail('relay/forbidden');
      }
      return { board, signer, nonce: c.findNonce(p.nonce, 2, p.signer, now) }; // A4
    };
    // Phase 1 of §8.4: nothing costly happens for a request without a fresh nonce.
    const first = upToNonce();
    const seedEntry = op === 'seed' ? p.a.entries.find((e) => ctEqual(e.dev, p.signer)) : null;
    const key0 = op === 'seed' ? seedEntry && seedEntry.pk : first.signer && first.signer.sign_pk;
    // Phase 2: the signatures.
    const sigOk = !!key0 && (await verifyStrict(key0, adminMessage(op, c.board, p.signer, p.args, p.nonce), p.sig));
    let pre = null;
    if (p.x.frame) pre = await c.precheck([p.x.frame], cred);
    else if (p.x.frames) pre = await c.precheck(p.x.frames, null);
    // Phase 3: the one transaction, every check again in the order of §12.1.
    let after = null;
    const out = c.tx(() => {
      const { board, signer, nonce } = upToNonce();
      c.run(SQL.nonceDelete, nonce.nonce); // deleted now, whatever follows (A4)
      try {
        // A5
        const who = op === 'seed' ? seedEntry && { role: seedEntry.role, state: seedEntry.status, sign_pk: seedEntry.pk } : signer;
        if (!who || who.state !== 1 || (spec.signer !== 0 && who.role !== spec.signer)) fail('relay/not_signer');
        // A6
        if (!sigOk || !ctEqual(who.sign_pk, key0)) fail('relay/bad_admin_sig');
        // A7: every check of the operation stands before its first write.
        const done = ADMIN[op](c, { p, board, signer, cred, now, pre });
        after = done.after || null;
        return json(200, done.json);
      } catch (e) {
        if (e instanceof RelayError && e.code !== 'relay/storage_refused') return e;
        throw e;
      }
    });
    if (after) after();
    return out;
  },

  // §12.7
  async 'seed-tail'(c, req, now) {
    const cred = req.cred;
    const guard = () => {
      c.credRow(cred, false);
      const board = c.boardRow();
      c.need(board, cred, 3);
      if (!ctEqual(board.seed_signer, cred.dev)) fail('relay/forbidden');
      return board;
    };
    guard();
    const r = new Reader(req.body, 'relay/bad_body');
    const first = r.u64OrNull(), n = r.u16(), frames = [];
    if (n < 1 || n > 256) fail('relay/bad_body');
    let total = 0;
    for (let i = 0; i < n; i++) {
      const f = r.bytes(r.u32());
      total += f.length;
      if (total > MIB) fail('relay/bad_body');
      frames.push(f);
    }
    r.end();
    const pre = await c.precheck(frames, null);
    return c.tx(() => {
      const board = guard(), head = c.head(board);
      if (first !== head.seq + 1) fail('relay/not_next', { head: { seq: head.seq, chain: hex(head.chain) } });
      if (first + n - 1 > board.seed_last_seq) fail('relay/target');
      let open = false;
      const out = c.tail(pre, n, now, head, null, (end) => {
        if (end.seq !== board.seed_last_seq) return;
        if (!ctEqual(end.chain, board.seed_chain)) fail('relay/chain_mismatch');
        open = true;
      });
      if (open) c.run(SQL.boardOpen);
      return json(200, { head: { seq: out.head.seq, chain: hex(out.head.chain) }, state: open ? 'open' : 'seeding' });
    });
  },
};

// The operations' own checks and effects (A7), inside the request's transaction.
// Each returns { json, after? }; `after` runs once the transaction has committed.
const ADMIN = {
  // §12.3
  register(c, { p, board, now, pre }) {
    const { dev, pk, hash } = p.a, frame = p.x.frame;
    const devs = c.all(SQL.devAll);
    const same = devs.find((d) => ctEqual(d.dev, dev));
    const repeat = !!same && same.role === 1 && same.state === 1 && ctEqual(same.sign_pk, pk);
    if (!repeat) {
      if (same || devs.some((d) => ctEqual(d.sign_pk, pk))) fail('relay/duplicate_key');
      const r1 = devs.filter((d) => d.role === 1);
      if (r1.filter((d) => d.state === 1).length >= 16 || r1.length >= 256) fail('relay/device_cap');
    }
    let seq, chain, carry = null;
    if (frame) {
      const mine = ctEqual(frameHash(frame), hash) ? (ctEqual(frame.subarray(20, 36), p.signer) ? null : 'relay/not_author') : 'relay/frame_mismatch';
      if (repeat) { // the frame is already stored, or this is not a repeat
        if (mine || pre.stop) fail('relay/duplicate_key');
        const f = pre.ok[0];
        const r = c.decide(board, f, c.author(new Map(), f), Math.floor(now / DAY));
        if (r.t !== 1) fail('relay/duplicate_key');
        ({ seq, chain } = r);
      } else {
        if (mine) fail(mine);
        carry = c.carried(board, pre, now);
        if (carry.dup) fail('relay/conflict', { last_ctr: carry.a.last_ctr, last_seq: carry.a.last_seq }); // stored already, by a plain append
      }
    } else {
      const row = p.x.seq === null ? null : c.one(SQL.logAt, p.x.seq);
      if (!row || row.seq < board.purge_floor || !ctEqual(row.hash, hash)) fail(repeat ? 'relay/duplicate_key' : 'relay/frame_mismatch');
      ({ seq, chain } = row);
    }
    if (repeat) {
      if (same.gen >= MAXGEN) fail('relay/gen_exhausted');
      c.run(SQL.devGen, same.gen + 1, dev);
      return { json: { seq, chain: hex(chain), token: c.token(1, dev, same.gen + 1) } };
    }
    let stored = [];
    if (carry) {
      const row = c.store(carry.f, carry.a, c.head(board), now, carry.day, true);
      c.flush(carry.authors);
      stored = [row];
      ({ seq, chain } = row);
    }
    c.run(SQL.devInsert, dev, pk, 1, 1, 1, 0, 0, now);
    return { json: { seq, chain: hex(chain), token: c.token(1, dev, 1) }, after: () => c.push(stored, now) };
  },

  // §12.4
  revoke(c, { p, signer }) {
    const target = p.a.dev;
    const devs = c.all(SQL.devAll);
    const t = devs.find((d) => ctEqual(d.dev, target));
    if (!t || t.role !== 1 || t.state !== 1) fail('relay/target');
    if (signer.role === 1 && ctEqual(signer.dev, target)) fail('relay/not_signer');
    if (devs.filter((d) => d.role === 1 && d.state === 1).length < 2) fail('relay/last_device');
    c.run(SQL.devRevoke, Math.min(t.gen + 1, MAXGEN), target);
    c.run(SQL.rtRevokeAll);
    return { json: {}, after: () => c.closeSockets(target, () => true, 4003) };
  },

  // §12.5
  reinstate(c, { p }) {
    if (p.a.first < 1) fail('relay/bad_args');
    const devs = c.all(SQL.devAll);
    const t = devs.find((d) => ctEqual(d.dev, p.a.dev));
    if (!t || t.role !== 1 || t.state !== 2) fail('relay/target');
    if (devs.filter((d) => d.role === 1 && d.state === 1).length >= 16) fail('relay/device_cap');
    c.run(SQL.devReinstate, p.a.stored, p.a.dev);
    c.run(SQL.rtRevokeAll);
    return { json: {} };
  },

  // §12.6
  fence(c, { p, board, now, pre }) {
    const { host, door: newDoor, homeGen } = p.a, frame = p.x.frame;
    if (!validHost(host)) fail('relay/bad_args');
    if (homeGen <= board.home_gen) fail('relay/home_gen');
    let out = {}, stored = [];
    if (frame) {
      if (!ctEqual(frame.subarray(20, 36), p.signer)) fail('relay/not_author');
      const carry = c.carried(board, pre, now);
      if (carry.dup) out = { seq: carry.seq, chain: hex(carry.chain) };
      else {
        const row = c.store(carry.f, carry.a, c.head(board), now, carry.day, true);
        c.flush(carry.authors);
        stored = [row];
        out = { seq: row.seq, chain: hex(row.chain) };
      }
    }
    c.run(SQL.boardFence, homeGen, host, newDoor);
    return {
      json: out,
      after: () => {
        c.push(stored, now);
        const fenced = c.boardRow();
        const msg = cat(u8(0x89), u8(2), u8(1), c.fencePart(fenced));
        for (const s of c.ready()) c.send(s, msg);
      },
    };
  },

  // §12.7
  seed(c, { p, board, now }) {
    const a = p.a, e = a.entries;
    // 1
    if (a.homeGen < 2 || a.first === null || a.last === null || a.first < 1 || a.last < a.first - 1 || (a.last === a.first - 1 && !ctEqual(a.chainLast, a.chainBefore))) fail('relay/bad_args');
    // 2 (the count stood in A1)
    const keys = new Set();
    for (let i = 0; i < e.length; i++) {
      if (i && cmp(e[i - 1].dev, e[i].dev) >= 0) fail('relay/bad_args');
      if ((e[i].role !== 1 && e[i].role !== 2) || (e[i].status !== 1 && e[i].status !== 2) || e[i].last_ctr === null) fail('relay/bad_args');
      keys.add(hex(e[i].pk));
    }
    if (keys.size !== e.length) fail('relay/bad_args');
    // 3
    const rec = e.filter((x) => x.role === 2);
    if (rec.some((x) => !ctEqual(x.dev, recoveryDev(c.board, x.pk))) || rec.filter((x) => x.status === 1).length !== 1 || rec.length > 64) fail('relay/bad_args');
    // 4
    const devs = e.filter((x) => x.role === 1), current = devs.filter((x) => x.status === 1).length;
    if (current < 1 || current > 16 || devs.length > 256) fail('relay/bad_args');
    if (board) for (const q of SQL.wipe) c.run(q); // a new seed replaces an unfinished one
    const open = a.last === a.first - 1;
    c.run(SQL.boardInsert, c.board, open ? 1 : 3, now, a.first - 1, a.chainBefore, a.first, a.first, a.first, a.homeGen,
      open ? null : p.signer, open ? null : a.last, open ? null : a.chainLast);
    for (const x of e) c.run(SQL.devInsert, x.dev, x.pk, x.role, x.status, ctEqual(x.dev, p.signer) ? 1 : 0, x.last_ctr, 0, now);
    return { json: { door: c.doorHex(), token: c.token(1, p.signer, 1), next_seq: a.first, state: open ? 'open' : 'seeding' } };
  },

  // §12.8
  compact(c, { p, board, now }) {
    const a = p.a;
    const below = a.below === null ? Infinity : a.below;
    for (let i = 0; i < a.keep.length; i++) if (a.keep[i] === null || a.keep[i] >= below || (i && a.keep[i] <= a.keep[i - 1])) fail('relay/bad_args'); // 1
    const lo = Math.max(board.compact_floor, board.purge_floor), head = c.head(board);
    if (a.below === null || !(lo < a.below && a.below <= head.seq - 16384)) fail('relay/target'); // 2
    const snap = a.snapSeq === null ? null : c.one(SQL.logAt, a.snapSeq);
    if (!snap || snap.seq < board.purge_floor) fail('relay/no_snapshot'); // 3
    const blob = c.one(SQL.blobGet, a.blob);
    if (!blob || blob.complete !== 1 || blob.purpose !== 1 || !ctEqual(blob.author, snap.dev) || blob.seq < a.below || blob.seq >= a.snapSeq) fail('relay/no_snapshot'); // 4
    if (ctEqual(snap.dev, p.signer)) fail('relay/not_signer'); // 5
    const day = Math.floor(now / DAY), used = board.del_day === day ? board.del_count : 0;
    if (used >= 5000) fail('relay/delete_budget', { until: (day + 1) * DAY }); // 6
    const hi = Math.min(a.below, lo + 5000 - used);
    const edge = c.one(SQL.logAt, hi - 1);
    if (!edge || edge.at > now - 31536000000) fail('relay/too_recent'); // 7
    const keep = a.keep.filter((k) => k >= lo && k < hi);
    let from = lo;
    for (const k of keep) {
      if (k > from) c.run(SQL.logDeleteRange, from, k);
      from = k + 1;
    }
    if (hi > from) c.run(SQL.logDeleteRange, from, hi);
    const d = hi - lo - keep.length; // the rows at or above the floor are contiguous (RELAY-STORE-005)
    c.run(SQL.boardCompact, hi, a.blob, day, used + d);
    for (const b of c.all(SQL.blobAll)) if (b.purpose === 3) c.deleteBlob(b); // §13.3
    return { json: { deleted: d, floor: hi, more: hi < a.below } };
  },

  // §12.9
  restore_tail(c, { p, board, now, pre }) {
    const head = c.head(board), n = p.a.hashes.length;
    if (p.a.first !== head.seq + 1) fail('relay/not_next', { head: { seq: head.seq, chain: hex(head.chain) } });
    if (!ctEqual(p.x.chainBefore, head.chain)) fail('relay/chain_mismatch');
    const out = c.tail(pre, n, now, head, (i) => {
      if (!ctEqual(frameHash(p.x.frames[i]), p.a.hashes[i])) fail('relay/frame_mismatch', { index: i });
    }, null);
    return { json: { head: { seq: out.head.seq, chain: hex(out.head.chain) } }, after: () => c.push(out.stored, now) };
  },

  // §12.10
  purge(c, { p, board, now }) {
    const { before, epoch } = p.a;
    if (epoch < 1) fail('relay/bad_args');
    if (before === null || before < board.purge_floor || before > c.head(board).seq) fail('relay/target');
    let floor = board.purge_floor, snap = board.compact_snap, readEpoch = board.read_epoch;
    if (before > floor) { // 1: nothing below it is served from now on
      floor = before;
      for (const b of c.all(SQL.blobAll)) {
        if (b.purpose > 3 || b.seq >= before) continue;
        c.deleteBlob(b);
        if (snap && ctEqual(snap, b.id)) snap = null;
      }
      readEpoch = Math.max(readEpoch, epoch);
      c.run(SQL.rtRevokeAll);
    }
    // 2: the rows themselves, within the day's delete budget
    const day = Math.floor(now / DAY), used = board.del_day === day ? board.del_count : 0;
    const done = Math.min(floor, board.purge_done + Math.max(0, 5000 - used));
    const d = done > board.purge_done ? c.all(SQL.logDeleteCounted, board.purge_done, done).length : 0;
    c.run(SQL.boardPurge, floor, done, snap, day, used + d, readEpoch);
    return { json: { purge_floor: floor, done } };
  },

  // §12.11
  mint_read_token(c, { p, board }) {
    const { dev, epoch } = p.a;
    if (!ctEqual(dev, p.signer)) fail('relay/not_signer');
    if (epoch < 1) fail('relay/bad_args');
    if (epoch < board.read_epoch) fail('relay/stale_epoch', { read_epoch: board.read_epoch });
    const row = c.one(SQL.rtGet, dev);
    if (row && row.gen >= MAXGEN) fail('relay/gen_exhausted');
    if (epoch > board.read_epoch) {
      c.run(SQL.rtRevokeBelow, epoch, dev); // the writer's own row is written once, below
      c.run(SQL.boardReadEpoch, epoch);
    }
    const gen = row ? row.gen + 1 : 1;
    if (row) c.run(SQL.rtUpdate, gen, epoch, dev); else c.run(SQL.rtInsert, dev, gen, epoch);
    return { json: { token: c.token(2, dev, gen), gen } };
  },
  revoke_read_token(c, { p }) {
    if (!c.one(SQL.rtGet, p.a.dev)) fail('relay/target');
    c.run(SQL.rtRevoke, p.a.dev);
    return { json: {} };
  },

  // §12.12
  recovery_register(c, { p, board, now }) {
    const { dev, pk, hash } = p.a;
    if (!ctEqual(dev, recoveryDev(c.board, pk))) fail('relay/bad_args');
    const devs = c.all(SQL.devAll);
    const same = devs.find((d) => ctEqual(d.dev, dev));
    const row = p.x.seq === null ? null : c.one(SQL.logAt, p.x.seq);
    const named = !!row && row.seq >= board.purge_floor && ctEqual(row.hash, hash);
    if (same && same.role === 2 && same.state === 1 && ctEqual(same.sign_pk, pk) && named) return { json: { seq: row.seq, chain: hex(row.chain) } };
    if (same || devs.some((d) => ctEqual(d.sign_pk, pk))) fail('relay/duplicate_key');
    if (devs.filter((d) => d.role === 2).length >= 64) fail('relay/device_cap');
    if (!named) fail('relay/frame_mismatch');
    c.run(SQL.devRetire);
    c.run(SQL.devInsert, dev, pk, 2, 1, 0, 0, 0, now);
    c.run(SQL.rtRevokeAll);
    return { json: { seq: row.seq, chain: hex(row.chain) } };
  },
};
