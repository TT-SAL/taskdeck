// Strict Ed25519 at the relay (relay.md §3.4, §18 item 5): Wycheproof's
// ed25519_test.json and seal's published vectors, run as they are.
// spec: RELAY-GEN-009
// spec: RELAY-GEN-010
// spec: SEAL-GEN-010
import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';
import * as core from '../relay-core.js';
import { STD, TARGET, World, decodePage, hex, makeTarget, send, unhex } from './lib/harness.mjs';
import { KEYS } from './vectors/dsl.mjs';

const root = new URL('../../', import.meta.url);
const load = (p) => JSON.parse(fs.readFileSync(new URL(p, root), 'utf8'));
const wycheproof = load('crates/taskdeck-seal/tests/kat/data/ed25519_test.json');
const sigs = load('docs/sync/vectors/seal-signatures.json').cases;
const frames = load('docs/sync/vectors/seal-frame.json').cases;
const chains = load('docs/sync/vectors/seal-chain.json').cases;

// verifyStrict where the target runs it: in this process, or inside workerd through the harness Worker.
async function verify(pk, msg, sig) {
  if (TARGET !== 'worker') return core.verifyStrict(unhex(pk), unhex(msg), unhex(sig));
  const body = Buffer.from(JSON.stringify({ pk, msg, sig }));
  const r = await send(process.env.TD_WORKER_URL, { method: 'POST', path: '/__test/verify', headers: { 'Content-Length': String(body.length) }, body });
  assert.equal(r.status, 200);
  return JSON.parse(Buffer.from(r.body).toString()).ok;
}

test('Wycheproof ed25519_test.json: every case as ed25519-dalek verify_strict answers it', async () => {
  const seen = { valid: 0, invalid: 0, acceptable: 0 }, refused = [];
  for (const group of wycheproof.testGroups) {
    for (const t of group.tests) {
      // seal's reading of the 29 "acceptable" cases (its status file, Q10): a small-order key fails, a mixed-order key verifies.
      const want = t.result === 'valid' || (t.result === 'acceptable' && t.flags.length === 1 && t.flags[0] === 'MixedOrderPublicKey');
      if (t.result === 'acceptable') assert.ok(['MixedOrderPublicKey', 'SmallOrderPublicKey'].includes(t.flags[0]) && t.flags.length === 1, `tcId ${t.tcId}: an acceptable case this test does not know`);
      const got = await verify(group.publicKey.pk, t.msg, t.sig);
      assert.equal(got, want, `tcId ${t.tcId} (${t.result}, ${t.flags.join(' ')}): ${t.comment}`);
      seen[t.result]++;
      if (t.result === 'acceptable' && !got) refused.push(t.tcId);
    }
  }
  assert.deepEqual(seen, { valid: 117, invalid: 64, acceptable: 29 });
  assert.equal(refused.length, 8, 'eight acceptable cases fail, twenty-one verify');
});

test('verifyStrict refuses keys and signatures of another length, and every small-order point', async () => {
  const k = sigs.find((c) => c.sig && !c.op);
  const pk = unhex(k.sign_pk), msg = unhex(k.message), sig = unhex(k.sig);
  assert.equal(await core.verifyStrict(pk, msg, sig), true);
  assert.equal(await core.verifyStrict(pk.subarray(0, 31), msg, sig), false);
  assert.equal(await core.verifyStrict(pk, msg, sig.subarray(0, 63)), false);
  assert.equal(await core.verifyStrict(pk, msg, new Uint8Array(65)), false);
  assert.equal(await core.verifyStrict(null, msg, sig), false);
  const small = ['00', '01', 'ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f', '26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05', 'c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a'];
  for (const y of small) {
    for (const sign of [0, 0x80]) {
      const point = unhex(y.padEnd(64, '0'));
      point[31] |= sign;
      assert.equal(await verify(hex(point), k.message, k.sig), false, `key ${hex(point)}`);
      assert.equal(await verify(k.sign_pk, k.message, hex(point) + k.sig.slice(64)), false, `R ${hex(point)}`);
    }
  }
  // A y that is not below p, and an S that is not below L.
  assert.equal(await verify('edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f', k.message, k.sig), false);
  assert.equal(await verify(k.sign_pk, k.message, k.sig.slice(0, 64) + 'edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010'), false);
});

// spec: SEAL-CHAIN-001
test('seal-chain.json: the chain the relay computes', () => {
  const withSteps = chains.filter((c) => c.steps); // the other cases are move codes and fingerprints, the clients' alone
  assert.equal(withSteps.length, 2);
  for (const c of withSteps) {
    const board = unhex(c.board_id);
    let chain = core.chainZero(board);
    assert.equal(hex(chain), c.c_0, c.name);
    for (const s of c.steps) {
      assert.equal(hex(core.frameHash(unhex(s.frame))), s.frame_hash, `${c.name}: frame hash at ${s.seq}`);
      chain = core.chainStep(chain, s.seq, unhex(s.frame_hash));
      assert.equal(hex(chain), s.chain, `${c.name}: chain at ${s.seq}`);
    }
  }
});

// spec: SEAL-SIG-001
// spec: SEAL-SIG-003
// spec: SEAL-ENC-005
test('seal-signatures.json: the login and admin messages are the bytes seal signs', async () => {
  let n = 0;
  for (const c of sigs) {
    if (!c.sig) continue; // a case seal refuses to sign: nothing to verify
    const board = unhex(c.board_id);
    const msg = c.op
      ? core.adminMessage(c.op, board, unhex(c.signer), unhex(c.args), unhex(c.relay_nonce))
      : core.loginMessage(c.origin, board, unhex(c.dev), c.gen, unhex(c.relay_nonce), unhex(c.client_nonce));
    assert.equal(hex(msg), c.message, c.name);
    if (c.op) assert.equal(hex(core.frameHash(unhex(c.args))), c.args_hash, `${c.name}: H(args)`);
    assert.equal(await verify(c.sign_pk, c.message, c.sig), true, c.name);
    const other = Buffer.from(c.message, 'hex');
    other[other.length - 1] ^= 1;
    assert.equal(await verify(c.sign_pk, other.toString('hex'), c.sig), false, `${c.name}: another message`);
    n++;
  }
  assert.equal(n, 15);
});

const VDEV = '808182838485868788898a8b8c8d8e8f';
const device = frames.filter((c) => c.frame && c.author === 'device');
const genesis = frames.find((c) => c.frame && c.author === 'recovery');
const seedArgs = (pk, tail) => ({
  home_gen: 2, first_seq: 1, chain_before: { $chain: 0 }, last_seq: tail.length,
  chain_last: tail.length ? { $chain_from: { before: { $chain: 0 }, first: 1, frames: tail } } : { $chain: 0 },
  entries: [{ dev: 'alice', sign_pk: 'alice', role: 1, status: 1, last_ctr: 0 }, { dev: 'rec', sign_pk: 'rec', role: 2, status: 1, last_ctr: 0 }, { dev: { $hex: VDEV }, sign_pk: { $hex: pk }, role: 1, status: 1, last_ctr: 0 }],
});
const seedSteps = (args) => [
  { request: { route: 'admin-nonce', board: 'v', cred: { create: true }, body: { $dev: 'alice' } }, expect: { status: 200 } },
  { request: { route: 'admin', board: 'v', cred: { create: true }, params: { op: 'seed' }, body: { $admin: { op: 'seed', signer: 'alice', args } } }, expect: { status: 200 } },
];
const vectorBoard = (id, more = {}) => ({ keys: { alice: KEYS.alice, rec: KEYS.rec }, boards: { v: { id, salt: '00'.repeat(16), recovery: 'rec', first: 'alice', create: false } }, ...more });

// spec: SEAL-FRAME-001
// spec: SEAL-FRAME-002
// spec: SEAL-FRAME-003
test('seal-frame.json: seal\'s device frames are stored by the relay, and its damaged frames refused', async () => {
  assert.equal(device.length, 4);
  const world = new World(vectorBoard(device[0].board_id), makeTarget());
  try {
    await world.init();
    const tail = device.map((c) => ({ $hex: c.frame }));
    for (const s of seedSteps(seedArgs(device[0].sign_pk, tail))) await world.step(s, 'seed');
    const post = (list) => ({ route: 'seed-tail', board: 'v', cred: { token: 'alice' }, body: { $cat: [{ $u64: 1 }, { $u16: list.length }, { $frames: list }] } });
    let damaged = 0;
    for (const c of frames) {
      if (!c.frame_bytes) continue;
      // The relay refuses a counter of 0 as a frame error before it looks at the signature (§9.3 P3).
      const code = c.error === 'seal/frame_format' || c.name === 'ctr flipped' ? 'relay/bad_frame' : 'relay/bad_signature';
      await world.step({ request: post([{ $hex: c.frame_bytes }]), expect: { status: 400, json: { error: code, index: 0 }, rows_written: 0 } }, c.name);
      if (c.sign_pk) assert.equal(await verify(c.sign_pk, hex(Buffer.concat([Buffer.from('taskdeck/v1/frame\0'), Buffer.from(c.frame_bytes, 'hex').subarray(0, -64)])), c.frame_bytes.slice(-128)), false, c.name);
      damaged++;
    }
    assert.equal(damaged, 7);
    await world.step({ request: post(tail), expect: { status: 200, json: { head: { seq: 4, chain: { $chain: 4 } }, state: 'open' } } }, 'the tail');
    const r = await world.http({ route: 'read', board: 'v', cred: { token: 'alice' }, query: 'after=0' });
    assert.deepEqual(decodePage(r.body).entries.map((e) => hex(e.frame)), device.map((c) => c.frame));
    for (const c of device) {
      assert.equal(hex(core.frameHash(unhex(c.frame))), c.frame_hash, c.name);
      assert.equal(await verify(c.sign_pk, c.signed, c.frame.slice(-128)), true, c.name);
    }
  } finally {
    await world.stop();
  }
});

// spec: SEAL-KEY-005
// spec: SEAL-KEY-006
test('seal-frame.json: seal\'s Genesis frame creates its board', async () => {
  const item = JSON.parse(Buffer.from(genesis.payload, 'hex').toString()).items[0];
  const world = new World(vectorBoard(genesis.board_id), makeTarget());
  try {
    await world.init();
    const body = { $cat: [{ $hex: item.recovery.sign_pk }, { $hex: item.salt }, { $hex: item.device.dev }, { $hex: item.device.sign_pk }, { $u32: genesis.frame.length / 2 }, { $hex: genesis.frame }] };
    await world.step({ request: { route: 'create', board: 'v', cred: { create: true }, body }, expect: { status: 200, json: { seq: 1, chain: { $chain: 1 }, token: '$any', door: { $door: true } }, rows_written: 5 } }, 'create');
    const token = core.makeToken(unhex(STD.token_key), 1, unhex(genesis.board_id), unhex(item.device.dev), 1);
    assert.equal(world.last.token, token);
    await world.step({ request: { route: 'read', board: 'v', cred: { raw: `Bearer ${token}` }, query: 'after=0' }, expect: { status: 200, page: { head_seq: 1, seqs: [1], frames: [{ $hex: genesis.frame }] } } }, 'read');
  } finally {
    await world.stop();
  }
});

// The login route itself with seal's signatures: the core's nonce is the vector's (core target only).
test('seal-signatures.json: seal\'s login signatures log in', async (t) => {
  if (TARGET !== 'core') return t.diagnostic('the nonce can be chosen only in process: this case runs on the core target');
  for (const c of sigs.filter((x) => x.sig && !x.op)) {
    const target = makeTarget('core');
    const world = new World(vectorBoard(c.board_id, { origin: c.origin }), target);
    try {
      await world.init();
      for (const s of seedSteps(seedArgs(c.sign_pk, []))) await world.step(s, 'seed');
      const b = world.board('v');
      if (c.gen > 1) target.obj(b.hex).db.prepare('UPDATE devices SET gen = ? WHERE dev = ?').run(c.gen - 1, unhex(c.dev));
      target.fixedRandom = () => unhex(c.relay_nonce);
      await world.step({ request: { route: 'login-nonce', board: 'v', cred: { door: true }, body: { $hex: c.dev } }, expect: { status: 200, json: { nonce: c.relay_nonce, gen: c.gen, expires: { $time: 120000 } } } }, c.name);
      const body = { $cat: [{ $hex: c.dev }, { $u32: c.gen }, { $hex: c.relay_nonce }, { $hex: c.client_nonce }, { $hex: c.sig }] };
      const token = core.makeToken(unhex(STD.token_key), 1, b.id, unhex(c.dev), c.gen);
      await world.step({ request: { route: 'login', board: 'v', cred: { door: true }, body }, expect: { status: 200, json: { token, gen: c.gen, push: true } } }, c.name);
      if (c.gen === 4294967295) { // the generation cannot be raised again (§5.2)
        await world.step({ request: { route: 'login-nonce', board: 'v', cred: { door: true }, body: { $hex: c.dev } }, expect: { status: 409, json: { error: 'relay/gen_exhausted' }, rows_written: 0 } }, 'gen_exhausted');
        await world.step({ request: { route: 'hello', board: 'v', cred: { raw: `Bearer ${token}` } }, expect: { status: 200 } }, 'the last token');
      }
    } finally {
      await world.stop();
    }
  }
  return undefined;
});
