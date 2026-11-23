// The conformance suite's harness (relay.md §18, §21): it expands the builders of
// the vector files, signs its own frames, login and admin messages from seal.md's
// layouts, and drives one of three targets: the core in process, the standby over
// HTTP, the Worker under `wrangler dev`. Test code only; nothing here ships.
import assert from 'node:assert/strict';
import { createHash, randomBytes } from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import * as core from '../../relay-core.js';

export const TARGET = process.env.TD_TARGET || 'core';
export const STD = Object.freeze({
  token_key: '7461736b6465636b2d72656c61792d746573742d6b65792d30303030303030a1',
  create_secret: '7461736b6465636b2d72656c61792d746573742d6372656174652d3030303032',
  origin: 'https://relay.test',
  clock_ms: 1795000000000,
});

// ---------- bytes and seal.md's layouts, written again here on purpose

const te = new TextEncoder();
export const hex = (b) => Buffer.from(b).toString('hex');
export const unhex = (s) => new Uint8Array(Buffer.from(s, 'hex'));
export const cat = (...p) => new Uint8Array(Buffer.concat(p.map((x) => Buffer.from(x.buffer, x.byteOffset, x.length))));
export const u8 = (v) => Uint8Array.of(v);
export const u16 = (v) => Uint8Array.of(v >>> 8, v & 255);
export const u32 = (v) => Uint8Array.of(v >>> 24, (v >>> 16) & 255, (v >>> 8) & 255, v & 255);
export const u64 = (v) => { const b = Buffer.alloc(8); b.writeBigUInt64BE(BigInt(v)); return new Uint8Array(b); };
export const sha = (...p) => { const h = createHash('sha256'); for (const x of p) h.update(x); return new Uint8Array(h.digest()); };
export const ctx = (use) => te.encode(`taskdeck/v1/${use}\0`);
export const boardId = (rsignPk, salt) => sha(ctx('board'), rsignPk, salt).subarray(0, 16);
export const devR = (board, pk) => sha(ctx('recovery-dev'), board, pk).subarray(0, 16);
export const tidOf = (ticketKey) => sha(ctx('pair-tid'), ticketKey).subarray(0, 16);
export const chain0 = (board) => sha(ctx('chain'), board);
export const chainNext = (prev, seq, fh) => sha(ctx('chain'), prev, u64(seq), fh);
export function fill(len, seed) {
  const out = Buffer.alloc(len);
  for (let i = 0, o = 0; o < len; i++, o += 32) Buffer.from(sha(te.encode(String(seed)), u32(i))).copy(out, o);
  return new Uint8Array(out);
}

const PKCS8 = Buffer.from('302e020100300506032b657004220420', 'hex');
const keyCache = new Map();
export async function makeKey(seedHex) {
  if (!keyCache.has(seedHex)) {
    const priv = await crypto.subtle.importKey('pkcs8', Buffer.concat([PKCS8, Buffer.from(seedHex, 'hex')]), { name: 'Ed25519' }, true, ['sign']);
    const jwk = await crypto.subtle.exportKey('jwk', priv);
    const pk = new Uint8Array(Buffer.from(jwk.x, 'base64url'));
    keyCache.set(seedHex, { pk, dev: sha(te.encode('taskdeck-test-dev'), pk).subarray(0, 16), sign: async (m) => new Uint8Array(await crypto.subtle.sign({ name: 'Ed25519' }, priv, m)) });
  }
  return keyCache.get(seedHex);
}
const RUN = randomBytes(8);
const ED_L = 2n ** 252n + 27742317777372353535851937790883648493n;

// ---------- a world: the keys, boards and clock of one case

export class World {
  constructor(setup, target) {
    this.setup = { ...STD, ...setup };
    this.target = target;
    this.clock = this.setup.clock_ms;
    this.keys = new Map();
    this.boards = new Map();
    this.sockets = new Map();
    this.saved = {};
    this.last = null;
  }
  async init() {
    for (const [name, k] of Object.entries(this.setup.keys || {})) this.keys.set(name, await makeKey(k.seed));
    for (const [name, b] of Object.entries(this.setup.boards || {})) {
      // The Worker target keeps its objects for the whole run: every run draws its own board ids.
      const salt = this.target.realClock ? sha(unhex(b.salt), RUN).subarray(0, 16) : unhex(b.salt), id = b.id ? unhex(b.id) : boardId(this.keys.get(b.recovery).pk, salt);
      this.boards.set(name, { name, id, hex: hex(id), salt, recovery: b.recovery, first: b.first, hashAt: new Map(), chainAt: new Map([[0, chain0(id)]]) });
    }
    await this.target.start(this);
    for (const [name, b] of Object.entries(this.setup.boards || {})) {
      if (b.create === false) continue;
      await this.step({ request: { route: 'create', board: name, cred: { create: true }, body: { $create: { board: name } } }, expect: { status: 200 } }, `create of board ${name}`);
    }
    for (const step of this.setup.state || []) await this.step(step, 'setup');
  }
  async stop() {
    for (const s of this.sockets.values()) s.close();
    await this.target.stop();
  }
  board(name) { const b = this.boards.get(name); assert.ok(b, `board ${name}`); return b; }
  key(name) { const k = this.keys.get(name); assert.ok(k, `key ${name}`); return k; }
  // The dev of a key on a board: dev_R for a recovery key (a name that starts with "rec").
  dev(boardName, keyName) { return keyName.startsWith('rec') ? devR(this.board(boardName).id, this.key(keyName).pk) : this.key(keyName).dev; }
  chain(boardName, seq) {
    const b = this.board(boardName);
    let s = seq;
    while (!b.chainAt.has(s)) { assert.ok(s > 0, `no chain base below seq ${seq}`); s--; }
    let c = b.chainAt.get(s);
    for (let i = s + 1; i <= seq; i++) { assert.ok(b.hashAt.has(i), `the harness saw no frame at seq ${i}`); c = chainNext(c, i, b.hashAt.get(i)); b.chainAt.set(i, c); }
    return c;
  }

  // ----- builders (relay.md §21). `bn` is the board the value belongs to.
  async bytes(v, bn) {
    const x = await this.val(v, bn);
    if (typeof x === 'string') return /^([0-9a-f]{2})*$/.test(x) ? unhex(x) : te.encode(x);
    assert.ok(x instanceof Uint8Array, `not bytes: ${JSON.stringify(v)}`);
    return x;
  }
  async val(v, bn) {
    if (typeof v === 'string') {
      if (v.startsWith('$last.')) return this.last[v.slice(6)];
      if (v.startsWith('$saved.')) return this.saved[v.slice(7)];
      return v;
    }
    if (v === null || typeof v !== 'object' || v instanceof Uint8Array) return v;
    const [k] = Object.keys(v), a = v[k];
    const B = (x) => this.bytes(x, bn), N = async (x) => Number(await this.val(x, bn));
    switch (k) {
      case '$hex': return unhex(a);
      case '$text': return a;
      case '$cat': { const parts = []; for (const x of a) parts.push(await B(x)); return cat(...parts); }
      case '$u8': return u8(await N(a));
      case '$u16': return u16(await N(a));
      case '$u32': return u32(await N(a));
      case '$u64': return typeof a === 'string' && /^[0-9a-f]{16}$/.test(a) ? unhex(a) : u64(await N(a));
      case '$fill': return fill(a.len, a.seed);
      case '$hash': return sha(await B(a));
      case '$key': return this.key(a).pk;
      case '$dev': return this.dev(a.board || bn, a.key || a);
      case '$board': return this.board(a).id;
      case '$tid': return tidOf(unhex(a));
      case '$door': return hex(core.door(unhex(this.setup.token_key), this.board(a === true ? bn : a).id));
      case '$token': return core.makeToken(unhex(this.setup.token_key), a.role || 1, this.board(a.board || bn).id, this.dev(a.board || bn, a.key), a.gen || 1);
      case '$chain': return this.chain(a.board || bn, a.seq === undefined ? a : a.seq);
      case '$frame': return this.frame({ board: bn, ...a });
      case '$batch': { const fs_ = []; for (const x of a) fs_.push(await B(x)); return cat(te.encode('TDA1'), u8(fs_.length), ...fs_.flatMap((f) => [u32(f.length), f])); }
      case '$frames': { const fs_ = []; for (const x of a) fs_.push(await B(x)); return cat(...fs_.flatMap((f) => [u32(f.length), f])); }
      case '$create': return this.createBody(a);
      case '$login': return this.loginBody({ board: bn, ...a });
      case '$admin': return this.adminBody({ board: bn, ...a });
      case '$slot': { const [first, role] = { a: [0x41, 0x45], b: [0x42, 0x4e], c: [0x43, 0x45], d: [0x44, 0x45] }[a.label]; return cat(u8(a.first ?? first), u8(a.role ?? role), u16(a.len_field ?? a.body_len), fill(a.body_len + 32, a.fill || 'slot')); }
      case '$blob_begin': return cat(await B(a.id), u8(a.purpose), await B({ $u64: a.seq }), await B({ $u64: a.size }), a.sha256 ? await B(a.sha256) : sha(await B(a.id)));
      case '$part': { const parts = Math.ceil(a.size / 1048576), n = a.index < parts - 1 ? 1048576 : a.size - (parts - 1) * 1048576; return new Uint8Array(Buffer.alloc(a.len ?? n, (a.fill ?? 7) + a.index)); }
      case '$hello': return cat(u8(1), await B(a.session), u32(a.features || 0));
      case '$sock_append': return cat(u8(2), u32(a.req), await B(a.batch.$hex ? a.batch : { $batch: a.batch }));
      case '$sock_read': return cat(u8(3), u32(a.req), await B({ $u64: a.after }), u16(a.limit));
      default: return v;
    }
  }
  async frame(a) {
    const b = this.board(a.board), key = this.key(a.author), bodyLen = a.body_len ?? 558;
    const dev = a.dev ? await this.bytes(a.dev, a.board) : this.dev(a.board, a.author);
    const body = cat(u8(1), u8(1), u32(1), new Uint8Array(24), fill(bodyLen - 30, a.fill ?? `${a.author}/${a.ctr}`));
    const head = cat(te.encode(a.magic || 'TDF1'), a.board_id ? unhex(a.board_id) : b.id, dev, a.ctr_hex ? unhex(a.ctr_hex) : u64(a.ctr), u32(bodyLen + (a.len_delta || 0)), body);
    const sig = await (a.signer ? this.key(a.signer) : key).sign(cat(ctx('frame'), head));
    const how = a.sig || 'good';
    if (how === 'flip') sig[10] ^= 1;
    else if (how === 'zero-s') sig.fill(0, 32);
    else if (how === 'high-s') { // S + L: the same signature to a verifier that does not check S < L
      let s = BigInt('0x' + hex(sig.subarray(32).slice().reverse())) + ED_L;
      for (let i = 32; i < 64; i++) { sig[i] = Number(s & 255n); s >>= 8n; }
    } else assert.equal(how, 'good');
    return cat(head, sig, a.tail ? unhex(a.tail) : new Uint8Array(0));
  }
  async createBody(a) {
    const b = this.board(a.board), rec = this.key(a.rsign || b.recovery), first = a.dev || b.first;
    const frame = a.frame ? await this.bytes(a.frame, a.board) : await this.frame({ board: a.board, author: a.author || b.recovery, ctr: 1, fill: 'genesis' });
    return cat(rec.pk, a.salt ? unhex(a.salt) : b.salt, a.dev_id ? unhex(a.dev_id) : this.dev(a.board, first), this.key(a.sign_pk || first).pk, u32(a.flen ?? frame.length), frame);
  }
  async loginBody(a) {
    const b = this.board(a.board), dev = this.dev(a.board, a.dev), nonce = await this.bytes(a.nonce ?? '$last.nonce', a.board);
    const cn = a.client_nonce ? unhex(a.client_nonce) : fill(32, 'client-nonce'), origin = te.encode(a.origin ?? this.setup.origin);
    const msg = cat(ctx(a.context || 'login'), u16(origin.length), origin, b.id, dev, u32(a.sign_gen ?? a.gen), nonce, cn);
    const sig = await this.key(a.signer || a.dev).sign(msg);
    if (a.sig === 'flip') sig[3] ^= 1;
    return cat(dev, u32(a.gen), nonce, cn, sig);
  }
  async adminArgs(op, f, bn) {
    if (f.$hex !== undefined || f.$cat) return this.bytes(f, bn);
    const B = (x) => this.bytes(x, bn);
    const dev = async (x) => (typeof x === 'string' && this.keys.has(x) ? this.dev(bn, x) : B(x));
    const pk = async (x) => (typeof x === 'string' && this.keys.has(x) ? this.key(x).pk : B(x));
    const num = async (x) => Number(await this.val(x, bn));
    switch (op) {
      case 'register': case 'recovery_register': return cat(await dev(f.dev), await pk(f.sign_pk), await B(f.frame_hash));
      case 'revoke': case 'revoke_read_token': return dev(f.dev);
      case 'reinstate': return cat(await dev(f.dev), u32(f.first_epoch), u16(f.n ?? f.wraps), fill(96 * f.wraps, f.fill || 'wraps'));
      case 'fence': return cat(u8(te.encode(f.host).length), te.encode(f.host), await B(f.door), u32(f.home_gen));
      case 'seed': {
        const entries = [];
        for (const e of f.entries) entries.push(cat(await dev(e.dev), await pk(e.sign_pk), u8(e.role), u8(e.status), await B({ $u64: e.last_ctr })));
        if (f.sort !== false) entries.sort((x, y) => Buffer.compare(x, y));
        return cat(u32(f.home_gen), await B({ $u64: f.first_seq }), await B(f.chain_before), await B({ $u64: f.last_seq }), await B(f.chain_last), f.blob_id ? await B(f.blob_id) : new Uint8Array(16), u16(f.n ?? entries.length), ...entries);
      }
      case 'compact': return cat(await B({ $u64: f.below_seq }), await B({ $u64: f.snap_entry_seq }), await B(f.blob_id), u32(f.n ?? f.keep.length), ...f.keep.map((k) => u64(k)));
      case 'restore_tail': { const hs = []; for (const h of f.hashes) hs.push(await B(h)); return cat(await B({ $u64: f.first_seq }), u32(f.n ?? hs.length), ...hs); }
      case 'purge': return cat(await B({ $u64: f.before_seq }), u32(f.epoch));
      case 'mint_read_token': return cat(await dev(f.dev), u32(await num(f.epoch)));
      default: throw new Error(`no args layout for ${op}`);
    }
  }
  async adminBody(a) {
    const b = this.board(a.board), signer = a.signer_dev ? await this.bytes(a.signer_dev, a.board) : this.dev(a.board, a.signer);
    const nonce = await this.bytes(a.nonce ?? '$last.nonce', a.board), args = await this.adminArgs(a.op, a.args, a.board);
    const extra = a.extra ? await this.bytes(a.extra, a.board) : new Uint8Array(0), op = te.encode(a.sign_op || a.op);
    const sig = await this.key(a.sign_key || a.signer).sign(cat(ctx(a.context || 'admin'), u8(op.length), op, b.id, signer, sha(a.sign_args ? await this.bytes(a.sign_args, a.board) : args), nonce));
    if (a.sig === 'flip') sig[3] ^= 1;
    return cat(signer, nonce, sig, u32(a.args_len ?? args.length), args, u32(a.extra_len ?? extra.length), extra, a.tail ? unhex(a.tail) : new Uint8Array(0));
  }

  // ----- requests
  async credHeader(cred, bn) {
    if (!cred) return {};
    if (cred.raw !== undefined) return { Authorization: cred.raw };
    if (cred.token) return { Authorization: 'Bearer ' + (await this.val({ $token: { key: cred.token, gen: cred.gen, board: cred.board } }, bn)) };
    if (cred.read_token) return { Authorization: 'Bearer ' + (await this.val({ $token: { key: cred.read_token, gen: cred.gen, role: 2, board: cred.board } }, bn)) };
    if (cred.door) return { Authorization: 'Door ' + (await this.val({ $door: cred.door === true ? bn : cred.door }, bn)) };
    if (cred.create) return { Authorization: 'Create ' + (typeof cred.create === 'string' ? cred.create : this.setup.create_secret || STD.create_secret) };
    return {};
  }
  async http(rq) {
    const bn = rq.board, b = this.board(bn);
    let method, p;
    if (rq.raw) ({ method, path: p } = rq.raw);
    else {
      const r = core.ROUTES.find((x) => x.name === rq.route);
      assert.ok(r, `route ${rq.route}`);
      method = r.method;
      const params = { board: b.hex };
      for (const [k, v] of Object.entries(rq.params || {})) { const x = await this.val(v, bn); params[k] = x instanceof Uint8Array ? hex(x) : String(x); }
      p = r.pattern.replace(/\{(\w+)\}/g, (_, k) => params[k]);
      if (rq.query !== undefined) p += '?' + (typeof rq.query === 'string' ? rq.query : Object.entries(rq.query).map(([k, v]) => `${k}=${v}`).join('&'));
    }
    p = p.replace('{board}', b.hex);
    const body = rq.body === undefined ? null : await this.bytes(rq.body, bn);
    const headers = { ...(await this.credHeader(rq.cred, bn)) };
    if (rq.proof) headers['X-TD-Proof'] = rq.proof;
    if (body && !rq.slow) headers['Content-Length'] = String(body.length);
    if (rq.slow) headers['Content-Length'] = String(rq.slow.declare);
    for (const [k, v] of Object.entries(rq.headers || {})) {
      if (v === null) delete headers[k];
      else headers[k] = typeof v === 'object' && !Array.isArray(v) ? hex(await this.bytes(v, bn)) : v;
    }
    this.sent = { rq, body, board: b };
    return this.target.request({ method, path: p, headers, body, slow: rq.slow, board: b.hex });
  }

  // ----- one step of a case
  async step(s, where) {
    try {
      if (s.clock_ms !== undefined) { this.clock = s.clock_ms; await this.target.setClock(this, this.clock); }
      if (s.socket) return await this.socketStep(s);
      const rq = s.request, b = this.board(rq.board), e = s.expect || {};
      const stats = e.object_called !== undefined || e.rows_written !== undefined || e.rows_read_max !== undefined;
      const before = stats ? await this.target.stats(b.hex) : null;
      const r = await this.http(rq);
      const after = stats ? await this.target.stats(b.hex) : null;
      const isJson = (r.headers['content-type'] || '').startsWith('application/json');
      const j = isJson ? JSON.parse(Buffer.from(r.body).toString()) : null;
      this.track(rq, r, j);
      if (j) this.last = j;
      if (s.save) for (const [name, field] of Object.entries(s.save)) this.saved[name] = j[field];
      await this.check(e, r, j, rq.board, before, after);
      return r;
    } catch (err) {
      err.message = `${where}: ${JSON.stringify(s).slice(0, 300)}\n${err.message}`;
      throw err;
    }
  }
  // The harness's own model of the log: which frame hash sits at which seq.
  track(rq, r, j) {
    const b = this.sent.board, body = this.sent.body;
    const framesOf = (bytes) => { const out = []; for (let o = 0; o < bytes.length;) { const n = Buffer.from(bytes).readUInt32BE(o); out.push(bytes.subarray(o + 4, o + 4 + n)); o += 4 + n; } return out; };
    if (rq.route === 'create' && r.status === 200) b.hashAt.set(1, sha(body.subarray(100)));
    if (rq.route === 'append' && !j) noteResults(b, decodeResults(r.body), framesOf(body.subarray(5)));
    if (r.status !== 200 || !j) return;
    if (rq.route === 'seed-tail') { const fs_ = framesOf(body.subarray(10)); fs_.forEach((f, i) => b.hashAt.set(j.head.seq - fs_.length + 1 + i, sha(f))); }
    if (rq.route !== 'admin') return;
    const alen = Buffer.from(body).readUInt32BE(112), args = body.subarray(116, 116 + alen), extra = body.subarray(120 + alen);
    const op = rq.params.op;
    if ((op === 'register' || op === 'fence') && extra.length > 8 && j.seq) b.hashAt.set(j.seq, sha(extra));
    if (op === 'restore_tail') { const fs_ = framesOf(extra.subarray(32)); fs_.forEach((f, i) => b.hashAt.set(j.head.seq - fs_.length + 1 + i, sha(f))); }
    if (op === 'seed') { const first = Number(Buffer.from(args).readBigUInt64BE(4)); b.hashAt.clear(); b.chainAt.clear(); b.chainAt.set(first - 1, args.slice(12, 44)); }
  }
  async expectValue(exp, got, bn, where) {
    if (exp === '$any') return assert.notEqual(got, undefined, `${where}: missing`);
    if (exp === '$push') return assert.equal(got, this.target.name !== 'standby', where);
    if (exp !== null && typeof exp === 'object' && !Array.isArray(exp)) {
      const [k] = Object.keys(exp);
      if (k === '$time') { const want = (this.target.realClock ? Date.now() : this.clock) + exp.$time; return assert.ok(Math.abs(got - want) <= (this.target.realClock ? 60000 : 0), `${where}: time ${got}, expected ${want}`); }
      if (k && k.startsWith('$')) { const x = await this.val(exp, bn); return assert.equal(got, x instanceof Uint8Array ? hex(x) : x, where); }
      assert.ok(got !== null && typeof got === 'object', `${where}: not an object`);
      assert.deepEqual(Object.keys(got), Object.keys(exp), `${where}: the fields and their order`);
      for (const key of Object.keys(exp)) await this.expectValue(exp[key], got[key], bn, `${where}.${key}`);
      return undefined;
    }
    if (Array.isArray(exp)) {
      assert.ok(Array.isArray(got) && got.length === exp.length, `${where}: array of ${exp.length}, got ${JSON.stringify(got)}`);
      for (let i = 0; i < exp.length; i++) await this.expectValue(exp[i], got[i], bn, `${where}[${i}]`);
      return undefined;
    }
    const want = typeof exp === 'string' && exp.startsWith('$') ? await this.val(exp, bn) : exp;
    return assert.deepEqual(got, want, where);
  }
  async check(e, r, j, bn, before, after) {
    const text = () => (j ? JSON.stringify(j) : `${r.body.length} bytes`);
    if (e.status !== undefined) assert.equal(r.status, e.status, `status (${text()})`);
    if (e.x_td_relay !== undefined) assert.equal(r.headers['x-td-relay'], e.x_td_relay, 'X-TD-Relay');
    if (r.status !== 101) { // RELAY-ENC-005, RELAY-SHAPE-004
      assert.equal(r.headers['cache-control'], 'no-store', 'Cache-Control');
      assert.match(r.headers['x-td-relay'] || '', /^(gate|object)$/, 'X-TD-Relay');
      assert.match(r.headers['content-type'] || '', /^application\/(json|octet-stream)$/, 'Content-Type');
      assert.ok(!Object.keys(r.headers).some((h) => h.startsWith('access-control-')), 'no CORS header');
    }
    if (e.json !== undefined) { assert.ok(j, `a JSON body (${text()})`); await this.expectValue(e.json, j, bn, 'json'); }
    if (e.error !== undefined) { assert.ok(j, 'a JSON error body'); assert.equal(j.error, e.error, text()); }
    if (e.body !== undefined) assert.equal(hex(r.body), hex(await this.bytes(e.body, bn)), 'body');
    if (e.results !== undefined) { assert.ok(!j, `a results body (${text()})`); await this.expectResults(e.results, decodeResults(r.body), bn); }
    if (e.page !== undefined) await this.expectPage(e.page, decodePage(r.body), bn);
    if (e.elapsed_ms !== undefined) assert.ok(r.elapsedMs >= e.elapsed_ms[0] && r.elapsedMs <= e.elapsed_ms[1], `elapsed ${r.elapsedMs} ms`);
    if (e.object_called !== undefined && after.calls !== null) assert.equal(after.calls - before.calls > 0, e.object_called, 'object called');
    if (e.rows_written !== undefined && after.rowsWritten !== null) assert.equal(after.rowsWritten - before.rowsWritten, e.rows_written, 'rows written');
    if (e.rows_read_max !== undefined && after.rowsRead !== null) assert.ok(after.rowsRead - before.rowsRead <= e.rows_read_max, `rows read ${after.rowsRead - before.rowsRead} > ${e.rows_read_max}`);
    if (e.storage !== undefined) await this.expectStorage(e.storage, bn);
  }
  async expectResults(exp, got, bn) {
    assert.equal(got.length, exp.length, `results: ${JSON.stringify(got.map((g) => g.status))}`);
    for (let i = 0; i < exp.length; i++) {
      const x = typeof exp[i] === 'string' ? { status: exp[i] } : exp[i], g = got[i];
      assert.equal(g.status, x.status, `result ${i}: ${JSON.stringify(g, (k, v) => (v instanceof Uint8Array ? hex(v).slice(0, 16) : v))}`);
      for (const k of ['seq', 'last_ctr', 'last_seq', 'http_status', 'code']) if (x[k] !== undefined) assert.equal(g[k], x[k], `result ${i} ${k}`);
      if (g.status === 'new' || g.status === 'dup') assert.equal(hex(g.chain), hex(this.chain(bn, g.seq)), `result ${i} chain`);
      if (x.frame !== undefined) assert.equal(hex(g.frame), x.frame === null ? '' : hex(await this.bytes(x.frame, bn)), `result ${i} frame`);
    }
  }
  async expectPage(exp, got, bn) {
    for (const k of ['head_seq', 'floor', 'purge_floor', 'more', 'truncated', 'fenced']) if (exp[k] !== undefined) assert.equal(got[k], exp[k], `page ${k}`);
    if (exp.head_seq !== undefined) assert.equal(hex(got.head_chain), hex(this.chain(bn, exp.head_seq)), 'page head_chain');
    if (exp.relay_now !== undefined) await this.expectValue(exp.relay_now, got.relay_now, bn, 'page relay_now');
    if (exp.seqs !== undefined) assert.deepEqual(got.entries.map((x) => x.seq), exp.seqs, 'page seqs');
    if (exp.frames !== undefined) for (let i = 0; i < exp.frames.length; i++) assert.equal(hex(got.entries[i].frame), hex(await this.bytes(exp.frames[i], bn)), `page frame ${i}`);
    for (let i = 1; i < got.entries.length; i++) assert.ok(got.entries[i].at >= got.entries[i - 1].at, 'at never decreases');
  }
  async expectStorage(exp, bn) {
    const b = this.board(bn), rows = async (t) => this.target.sql(b.hex, t);
    const same = async (want, row, where) => { for (const [k, v] of Object.entries(want)) await this.expectValue(v, row[k], bn, `${where}.${k}`); };
    if (exp.board !== undefined) { const r = await rows('board'); if (exp.board === null) assert.equal(r.length, 0, 'no board row'); else { assert.equal(r.length, 1, 'one board row'); await same(exp.board, r[0], 'board'); } }
    if (exp.devices) { const r = await rows('devices'); for (const [name, want] of Object.entries(exp.devices)) { const d = hex(this.dev(bn, name)), row = r.find((x) => x.dev === d); if (want === null) assert.equal(row, undefined, `no devices row for ${name}`); else { assert.ok(row, `devices row of ${name}`); await same(want, row, `devices.${name}`); } } }
    if (exp.log) assert.deepEqual((await rows('log')).map((x) => x.seq), exp.log, 'log seqs');
    if (exp.count) for (const [t, n] of Object.entries(exp.count)) assert.equal((await rows(t)).length, n, `rows in ${t}`);
    for (const t of ['blobs', 'tickets', 'read_tokens', 'nonces']) {
      if (!exp[t]) continue;
      const r = await rows(t);
      assert.equal(r.length, exp[t].length, `rows in ${t}: ${JSON.stringify(r).slice(0, 400)}`);
      for (let i = 0; i < exp[t].length; i++) await same(exp[t][i], r[i], `${t}[${i}]`);
    }
  }

  // ----- sockets
  async socketStep(s) {
    const e = s.expect || {};
    if (s.open) {
      const bn = s.open.board, b = this.board(bn);
      const headers = { ...(await this.credHeader(s.open.cred, bn)), ...(s.open.headers || {}) };
      const r = await this.target.openSocket(this, b.hex, `/v1/b/${b.hex}/socket`, headers);
      if (r.socket) { r.socket.board = bn; this.sockets.set(s.socket, r.socket); }
      const j = r.socket ? null : JSON.parse(Buffer.from(r.body).toString());
      if (e.status !== undefined) assert.equal(r.status, e.status, 'upgrade status ' + JSON.stringify(j));
      if (e.error !== undefined) assert.equal(j && j.error, e.error);
      if (e.x_td_relay !== undefined) assert.equal(r.headers['x-td-relay'], e.x_td_relay);
      return undefined;
    }
    const sock = this.sockets.get(s.socket);
    assert.ok(sock, `socket ${s.socket}`);
    if (s.send !== undefined) { const v = await this.val(s.send, sock.board); await sock.send(typeof v === 'string' ? v : await this.bytes(s.send, sock.board)); }
    if (s.close) sock.close(typeof s.close === 'number' ? s.close : 1000);
    for (const m of e.messages || (e.message ? [e.message] : [])) {
      const got = await sock.next(5000);
      assert.ok(got && got.data, `socket ${s.socket}: expected ${m.type}, got ${JSON.stringify(got)}`);
      await this.expectMessage(m, decodeMessage(got.data), sock.board);
    }
    if (e.text !== undefined) { const got = await sock.next(5000); assert.equal(got && got.text, e.text); }
    if (e.closed !== undefined) { const got = await sock.next(5000); assert.deepEqual(got, { close: e.closed }, `socket ${s.socket} close`); }
    if (e.silent) { const got = await sock.next(this.target.realClock ? 400 : 0); assert.equal(got, null, `socket ${s.socket}: unexpected ${JSON.stringify(got && got.data ? decodeMessage(got.data).type : got)}`); }
    return undefined;
  }
  async expectMessage(exp, got, bn) {
    assert.equal(got.type, exp.type, `message ${JSON.stringify(got, (k, v) => (v instanceof Uint8Array ? hex(v).slice(0, 16) : v)).slice(0, 300)}`);
    for (const [k, v] of Object.entries(exp)) {
      if (k === 'type') continue;
      if (k === 'results') await this.expectResults(v, got.results, bn);
      else if (k === 'page') await this.expectPage(v, got.page, bn);
      else if (k === 'head_seq') { assert.equal(got.head_seq, v); assert.equal(hex(got.head_chain), hex(this.chain(bn, v)), 'welcome head_chain'); }
      else await this.expectValue(v, got[k] instanceof Uint8Array ? hex(got[k]) : got[k], bn, `message.${k}`);
    }
  }
}

function noteResults(b, results, frames) {
  results.forEach((r, i) => { if ((r.status === 'new' || r.status === 'dup') && frames[i]) b.hashAt.set(r.seq, sha(frames[i])); });
}

// ---------- decoders of the relay's binary answers (§9.6, §10.5, §11.3)

class Rd {
  constructor(b) { this.b = Buffer.from(b.buffer, b.byteOffset, b.length); this.o = 0; }
  u8() { return this.b[this.o++]; }
  u16() { this.o += 2; return this.b.readUInt16BE(this.o - 2); }
  u32() { this.o += 4; return this.b.readUInt32BE(this.o - 4); }
  u64() { this.o += 8; return Number(this.b.readBigUInt64BE(this.o - 8)); }
  bytes(n) { assert.ok(this.o + n <= this.b.length, 'short body'); this.o += n; return new Uint8Array(this.b.subarray(this.o - n, this.o)); }
  end() { assert.equal(this.o, this.b.length, 'trailing bytes'); }
}
function readResults(r) {
  assert.equal(Buffer.from(r.bytes(4)).toString(), 'TDR1');
  const n = r.u8(), out = [];
  for (let i = 0; i < n; i++) {
    const t = r.u8();
    if (t === 0 || t === 1) out.push({ status: t ? 'dup' : 'new', seq: r.u64(), chain: r.bytes(32) });
    else if (t === 2) { const seq = r.u64(); out.push({ status: 'conflict', seq, frame: r.bytes(r.u32()) }); }
    else if (t === 3) out.push({ status: 'gap', last_ctr: r.u64(), last_seq: r.u64() });
    else if (t === 4) { const http_status = r.u16(); out.push({ status: 'refused', http_status, code: Buffer.from(r.bytes(r.u8())).toString() }); }
    else { assert.equal(t, 5); out.push({ status: 'skipped' }); }
  }
  return out;
}
export function decodeResults(body) { const r = new Rd(body), out = readResults(r); r.end(); return out; }
function readPage(r) {
  assert.equal(Buffer.from(r.bytes(4)).toString(), 'TDP1');
  const p = { head_seq: r.u64(), head_chain: r.bytes(32), relay_now: r.u64(), floor: r.u64(), purge_floor: r.u64() };
  const flags = r.u8(), n = r.u16();
  assert.equal(flags & 0xf8, 0, 'unknown page flags');
  Object.assign(p, { more: !!(flags & 1), truncated: !!(flags & 2), fenced: !!(flags & 4), entries: [] });
  for (let i = 0; i < n; i++) { const seq = r.u64(), at = r.u64(); p.entries.push({ seq, at, frame: r.bytes(r.u32()) }); }
  return p;
}
export function decodePage(body) { const r = new Rd(body), p = readPage(r); r.end(); return p; }
function readFence(r) { const host = Buffer.from(r.bytes(r.u8())).toString(); return { host, door: hex(r.bytes(5)), home_gen: r.u32() }; }
export function decodeMessage(data) {
  const r = new Rd(data), t = r.u8();
  let m;
  if (t === 0x81) {
    m = { type: 'welcome', head_seq: r.u64(), head_chain: r.bytes(32), last_ctr: r.u64(), last_seq: r.u64(), relay_now: r.u64(), floor: r.u64(), purge_floor: r.u64(), state: r.u8(), caps: r.u8(), appends_left: r.u32(), door: hex(r.bytes(5)), has_fence: r.u8() };
    if (m.has_fence) m.fence = readFence(r);
  } else if (t === 0x82) m = { type: 'result', req: r.u32(), results: readResults(r) };
  else if (t === 0x83) m = { type: 'page', req: r.u32(), page: readPage(r) };
  else if (t === 0x84) m = { type: 'push', page: readPage(r) };
  else if (t === 0x85) m = { type: 'second', session: hex(r.bytes(16)) };
  else if (t === 0x86) m = { type: 'displaced', session: hex(r.bytes(16)) };
  else if (t === 0x87) m = { type: 'slot', tid: hex(r.bytes(16)), label: r.u8() };
  else if (t === 0x88) { m = { type: 'error', req: r.u32(), status: r.u16() }; m.code = Buffer.from(r.bytes(r.u8())).toString(); }
  else if (t === 0x89) { m = { type: 'state', state: r.u8(), has_fence: r.u8() }; if (m.has_fence) m.fence = readFence(r); }
  else throw new Error(`unknown socket message type ${t}`);
  r.end();
  return m;
}

// ---------- the counting storage (§18): rows written as the platform counts them

export function countingStorage(inner, stats) {
  const cost = (sql, n) => { const m = /^\s*(?:INSERT INTO|UPDATE|DELETE FROM)\s+(\w+)/i.exec(sql); return m ? n * (m[1] === 'log' ? 2 : 1) : 0; };
  return {
    all(sql, ...p) { const rows = inner.all(sql, ...p); stats.rowsRead += rows.length; stats.rowsWritten += cost(sql, rows.length); return rows; },
    run(sql, ...p) { const r = inner.run(sql, ...p); stats.rowsWritten += cost(sql, r.changes); return r; },
    transaction: (fn) => inner.transaction(fn),
  };
}

// ---------- target: the core in process

class FakeSockets {
  constructor() { this.socks = []; }
  accept(dev, attachment) { const s = { dev: hex(dev), open: true, out: [] }; this.setAttachment(s, attachment); this.socks.push(s); return { response: { accepted: s }, socket: s }; }
  list(dev) { return this.socks.filter((s) => s.open && (!dev || s.dev === hex(dev))); }
  attachment(s) { return JSON.parse(s.att); }
  setAttachment(s, a) { s.att = JSON.stringify(a); assert.ok(s.att.length <= 2048, 'attachment over 2,048 bytes'); }
  send(s, bytes) { if (!s.open) throw new Error('socket closed'); s.out.push({ data: new Uint8Array(bytes) }); }
  close(s, code) { if (s.open) { s.open = false; s.out.push({ close: code }); } }
}

function headerLists(headers) {
  const out = {};
  for (const [k, v] of Object.entries(headers)) out[k.toLowerCase()] = Array.isArray(v) ? v.map(String) : [String(v)];
  return out;
}

export class CoreTarget {
  constructor() { this.name = 'core'; this.realClock = false; this.boards = new Map(); }
  config(world) {
    const s = world.setup;
    return core.makeConfig({ tokenKey: s.token_key, createSecret: s.create_secret === null ? undefined : s.create_secret, origin: s.origin, push: true });
  }
  async start(world) { this.world = world; this.cfg = this.config(world); }
  async stop() { for (const b of this.boards.values()) b.db.close(); this.boards.clear(); }
  async setClock() {}
  obj(boardHex) {
    let b = this.boards.get(boardHex);
    if (!b) {
      const db = new DatabaseSync(':memory:'), stats = { calls: 0, rowsRead: 0, rowsWritten: 0 }, sockets = new FakeSockets();
      const storage = countingStorage(core.sqliteStorage(db), stats);
      const world = this.world;
      const c = new core.BoardCore({ storage, sockets, now: () => world.clock, random: this.random, config: this.cfg, board: unhex(boardHex) });
      c.migrate();
      stats.rowsWritten = 0;
      b = { db, stats, sockets, core: c, storage };
      this.boards.set(boardHex, b);
    }
    return b;
  }
  async request({ method, path: p, headers, body, board }) {
    const t0 = performance.now(), q = p.indexOf('?');
    const g = await core.gate({ method, path: q < 0 ? p : p.slice(0, q), query: q < 0 ? null : p.slice(q + 1), headers: headerLists(headers) }, this.cfg);
    let reply = g.reply;
    if (!reply) {
      const bytes = body || new Uint8Array(0);
      reply = core.checkBody(g.contentLength, bytes);
      if (!reply) {
        const o = this.obj(board);
        o.stats.calls++;
        try { reply = await o.core.handle({ route: g.route.name, params: g.params, query: g.query, cred: g.cred, body: bytes }); } catch (e) { if (process.env.TD_DEBUG) console.error(e); reply = core.refusal('relay/object_unavailable'); }
        if (reply.socket) return { status: 101, headers: {}, body: new Uint8Array(0), accepted: reply.reply.accepted, elapsedMs: 0 };
      }
    }
    return { status: reply.status, headers: Object.fromEntries(Object.entries(reply.headers).map(([k, v]) => [k.toLowerCase(), v])), body: reply.body, elapsedMs: performance.now() - t0 };
  }
  async stats(boardHex) { return { ...this.obj(boardHex).stats }; }
  async sql(boardHex, table) { return this.obj(boardHex).db.prepare(`SELECT * FROM ${table} ORDER BY 1`).all().map(hexRow); }
  async openSocket(world, boardHex, p, headers) {
    const r = await this.request({ method: 'GET', path: p, headers: { Upgrade: 'websocket', ...headers }, body: null, board: boardHex });
    if (!r.accepted) return r;
    const s = r.accepted, o = this.obj(boardHex);
    return { status: 101, headers: {}, socket: {
      send: async (data) => { if (s.open) { o.stats.calls += 1 / 20; await o.core.onMessage(s, data); } },
      next: async () => s.out.shift() || null,
      close: () => { if (s.open) { s.open = false; o.core.onClose(s, 1000); } },
    } };
  }
}
const hexRow = (row) => Object.fromEntries(Object.entries(row).map(([k, v]) => [k, v instanceof Uint8Array ? hex(v) : v]));

// ---------- HTTP and WebSocket clients for the two targets behind a port

const agent = new http.Agent({ keepAlive: true, maxSockets: 8 });
export function send(base, { method, path: p, headers, body, slow }) {
  return new Promise((resolve, reject) => {
    const u = new URL(base), t0 = performance.now();
    const req = http.request({ host: u.hostname, port: u.port, method, path: p, headers, agent: slow ? false : agent }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => { if (slow) req.destroy(); resolve({ status: res.statusCode, headers: res.headers, body: new Uint8Array(Buffer.concat(chunks)), elapsedMs: performance.now() - t0 }); });
    });
    req.on('error', reject);
    if (slow) { req.flushHeaders(); req.write(Buffer.alloc(slow.send, 1)); } else req.end(body ? Buffer.from(body) : undefined);
  });
}

// A small WebSocket client (RFC 6455) that can send any header, any size and text.
export function wsOpen(base, p, headers) {
  return new Promise((resolve, reject) => {
    const u = new URL(base);
    const req = http.request({ host: u.hostname, port: u.port, method: 'GET', path: p, agent: false, headers: { Connection: 'Upgrade', Upgrade: 'websocket', 'Sec-WebSocket-Version': '13', 'Sec-WebSocket-Key': randomBytes(16).toString('base64'), ...headers } });
    req.on('error', reject);
    req.on('response', (res) => { const chunks = []; res.on('data', (c) => chunks.push(c)); res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, body: new Uint8Array(Buffer.concat(chunks)) })); });
    req.on('upgrade', (res, sock, head) => {
      const queue = [], waiters = [];
      let buf = Buffer.from(head), parts = [], kind = 0, open = true;
      const put = (m) => { const w = waiters.shift(); if (w) w(m); else queue.push(m); };
      const frame = (op, payload) => {
        const n = payload.length, mask = randomBytes(4);
        const h = n < 126 ? Buffer.from([0x80 | op, 0x80 | n]) : n < 65536 ? Buffer.from([0x80 | op, 0xfe, n >> 8, n & 255]) : Buffer.concat([Buffer.from([0x80 | op, 0xff]), Buffer.from(u64(n))]);
        const body = Buffer.from(payload);
        for (let i = 0; i < n; i++) body[i] ^= mask[i & 3];
        if (open) sock.write(Buffer.concat([h, mask, body]));
      };
      sock.on('data', (d) => {
        buf = Buffer.concat([buf, d]);
        for (;;) {
          if (buf.length < 2) return;
          let n = buf[1] & 127, o = 2;
          if (n === 126) { if (buf.length < 4) return; n = buf.readUInt16BE(2); o = 4; } else if (n === 127) { if (buf.length < 10) return; n = Number(buf.readBigUInt64BE(2)); o = 10; }
          if (buf.length < o + n) return;
          const fin = buf[0] & 0x80, op = buf[0] & 15, payload = buf.subarray(o, o + n);
          buf = buf.subarray(o + n);
          if (op === 8) { put({ close: n >= 2 ? payload.readUInt16BE(0) : 1005 }); frame(8, payload.subarray(0, 2)); open = false; sock.end(); return; }
          if (op === 9) { frame(10, payload); continue; }
          if (op === 10) continue;
          if (op) kind = op;
          parts.push(Buffer.from(payload));
          if (fin) { const all = Buffer.concat(parts); parts = []; put(kind === 1 ? { text: all.toString() } : { data: new Uint8Array(all) }); }
        }
      });
      sock.on('close', () => { if (open) { open = false; put({ close: 1006 }); } });
      sock.on('error', () => {});
      resolve({ status: 101, headers: res.headers, socket: {
        send: async (data) => frame(typeof data === 'string' ? 1 : 2, typeof data === 'string' ? Buffer.from(data) : data),
        next: (ms) => new Promise((done) => { if (queue.length) return done(queue.shift()); const t = setTimeout(() => { waiters.splice(waiters.indexOf(w), 1); done(null); }, ms); const w = (m) => { clearTimeout(t); done(m); }; waiters.push(w); return undefined; }),
        close: (code = 1000) => { if (open) { frame(8, Buffer.from(u16(code))); } },
      } });
    });
    req.end();
  });
}

// ---------- target: the standby over HTTP on loopback

export class StandbyTarget {
  constructor() { this.name = 'standby'; this.realClock = false; }
  async start(world) {
    const { startStandby } = await import('../../standby.js');
    const s = world.setup;
    this.dir = fs.mkdtempSync(path.join(os.tmpdir(), 'td-standby-'));
    this.server = await startStandby({ tokenKey: s.token_key, createSecret: s.create_secret === null ? undefined : s.create_secret, origin: s.origin, dataDir: this.dir, listen: '127.0.0.1:0', now: () => world.clock });
  }
  async stop() { await this.server.close(); fs.rmSync(this.dir, { recursive: true, force: true }); }
  async setClock() {}
  request(r) { return send(this.server.url, r); }
  file(boardHex) { return path.join(this.dir, `${boardHex}.sqlite`); }
  // Without a counter in the shipped standby: a board whose file does not exist was never handed to the core.
  async stats(boardHex) { return { calls: fs.existsSync(this.file(boardHex)) ? null : 0, rowsRead: null, rowsWritten: null }; }
  async sql(boardHex, table) {
    if (!fs.existsSync(this.file(boardHex))) return [];
    const db = new DatabaseSync(this.file(boardHex), { readOnly: true });
    try { return db.prepare(`SELECT * FROM ${table} ORDER BY 1`).all().map(hexRow); } finally { db.close(); }
  }
  async openSocket(world, boardHex, p, headers) { return wsOpen(this.server.url, p, headers); }
}

// ---------- target: the Worker under `wrangler dev`, through the suite's harness Worker

export class WorkerTarget {
  constructor() { this.name = 'worker'; this.realClock = true; this.base = process.env.TD_WORKER_URL; }
  async start(world) {
    assert.ok(this.base, 'TD_WORKER_URL is not set: run the suite with `bash relay/test/run.sh worker`');
    const s = world.setup;
    // The harness Worker has one configuration; a case that needs another one names a variant of it.
    this.variant = s.create_secret === null ? 'no-create' : s.token_key !== STD.token_key ? 'bad-key' : s.origin !== STD.origin ? 'bad-origin' : null;
    if (!this.variant) assert.deepEqual([s.token_key, s.create_secret, s.origin], [STD.token_key, STD.create_secret, STD.origin], 'the Worker target runs with the standard configuration');
  }
  async stop() {}
  async setClock(world, ms) { for (const b of world.boards.values()) await this.test('POST', `/__test/clock/${b.hex}`, String(ms - Date.now())); }
  request(r) { return send(this.base, this.variant ? { ...r, headers: { ...r.headers, 'X-Test-Env': this.variant } } : r); }
  async test(method, p, body) {
    const r = await send(this.base, { method, path: p, headers: body ? { 'Content-Length': String(Buffer.byteLength(body)) } : {}, body: body ? Buffer.from(body) : null });
    assert.equal(r.status, 200, `harness route ${p}: ${Buffer.from(r.body).toString()}`);
    return JSON.parse(Buffer.from(r.body).toString());
  }
  async stats(boardHex) { const s = await this.test('GET', `/__test/stats/${boardHex}`); return { ...s, calls: s.calls + s.messages / 20 }; }
  async sql(boardHex, table) { return this.test('GET', `/__test/dump/${boardHex}/${table}`); }
  async openSocket(world, boardHex, p, headers) { return wsOpen(this.base, p, this.variant ? { ...headers, 'X-Test-Env': this.variant } : headers); }
}

export function makeTarget(name = TARGET) {
  if (name === 'core') return new CoreTarget();
  if (name === 'standby') return new StandbyTarget();
  if (name === 'worker') return new WorkerTarget();
  throw new Error(`TD_TARGET must be core, standby or worker, not ${name}`);
}

// Runs one case of a vector family against a target.
export async function runCase(c, targetName = TARGET) {
  const world = new World(c.setup, makeTarget(targetName));
  try {
    await world.init();
    for (let i = 0; i < c.steps.length; i++) await world.step(c.steps[i], `step ${i}`);
  } finally {
    await world.stop();
  }
}
