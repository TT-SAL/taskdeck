// Family: rows per operation (relay.md §16.1), one case per row of the table.
// Rows written are exact; rows read are the bounds of the table, as formulas of
// D (devices rows), B (blobs rows), n (nonces rows), t (tickets) and what the
// request touches. Both are checked against the counting storage on the core
// and against the platform's counters on the Worker.
import { CREATE, DOOR, DUP, F, N, RT, T, addDevice, admin, append, err, gateErr, kase, login, range, req, step } from './dsl.mjs';
import { READS } from './reads.mjs';

const ROWS = ['core', 'worker'];
const ID = (n) => n.toString(16).padStart(2, '0').repeat(16);
const K = (n) => n.toString(16).padStart(2, '0').repeat(32);
const H = (x) => ({ $hash: x });
const rw = (name, steps, o) => kase('rows', name, ['RELAY-ROWS-001'], steps, { targets: ROWS, state: [addDevice('bob', 2)], ...o });
// One step with its exact rows written and its bound on rows read.
const m = (request, written, read, expect = { status: 200 }) => step(request, { ...expect, rows_written: written, rows_read_max: read });
const nonceStep = (cred, signer, read) => m(req('admin-nonce', cred, { body: { $dev: signer } }), 1, read);
const adm = (name, signer, cred, args, written, read, o = {}, expect = { status: 200 }) => [
  step(req('admin-nonce', cred, { body: { $dev: signer } })),
  m(req('admin', cred, { params: { op: name }, body: { $admin: { op: name, signer, args, ...(o.extra ? { extra: o.extra } : {}), ...(o.admin || {}) } } }), written, read, expect),
];
const D3 = 3; // the devices rows of the standard board: the recovery identity, alice, bob
const slotA = { $slot: { label: 'a', body_len: 48 } }, slotB = { $slot: { label: 'b', body_len: 119 } }, slotC = { $slot: { label: 'c', body_len: 64 } };
const tid = { $tid: K(1) };
export const rows = [
  rw('a gate refusal, caps and door: no object call, no row', [
    step(req('hello', {}), { ...gateErr(401, 'relay/unauthorized'), rows_written: 0, rows_read_max: 0 }),
    step(req('caps', T('alice')), { status: 200, x_td_relay: 'gate', object_called: false, rows_written: 0, rows_read_max: 0 }),
    step(req('door', CREATE), { status: 200, x_td_relay: 'gate', object_called: false, rows_written: 0, rows_read_max: 0 }),
  ]),
  rw('create and its repeat', [
    m(req('create', CREATE, { body: { $create: { board: 'main' } } }), 5, READS.create),
    m(req('create', CREATE, { body: { $create: { board: 'main' } } }), 1, READS.createRepeat),
  ], { state: [], boards: { main: { create: false } } }),
  rw('the nonces and login', [
    m(req('login-nonce', DOOR, { body: { $dev: 'alice' } }), 1, READS.nonce(0)),
    m(req('login', DOOR, { body: { $login: { dev: 'alice', gen: 2 } } }), 2, READS.login(1)),
    m(req('login-nonce', T('alice', 2), { body: { $dev: 'alice' } }), 1, READS.nonce(0) + 1),
    m(req('login', T('alice', 2), { body: { $login: { dev: 'alice', gen: 9 } } }), 1, READS.login(1) + 1, err(409, 'relay/bad_gen')),
    nonceStep(T('alice', 2), 'alice', READS.nonce(0) + 1),
    nonceStep(DOOR, 'rec', READS.nonce(1)),
    ...range(6, (i) => nonceStep(T('alice', 2), 'alice', READS.nonce(2 + i) + 1)),
    m(req('login-nonce', DOOR, { body: { $dev: 'alice' } }), 1, READS.nonce(8)),
    m(req('login', DOOR, { body: { $login: { dev: 'alice', gen: 3 } } }), 2, READS.login(9)),
  ]),
  rw('hello and read write nothing', [
    append(T('alice'), range(6, (i) => F('alice', 2 + i)), range(6, (i) => N(3 + i))),
    m(req('hello', T('alice')), 0, READS.hello),
    m(req('read', T('alice'), { query: 'after=2' }), 0, READS.read(6)),
    m(req('read', T('alice'), { query: 'after=8' }), 0, READS.read(0)),
  ]),
  rw('append: 2f + a rows for f new frames by a authors, none for a replay', [
    m(req('append', T('alice'), { body: { $batch: [F('alice', 2)] } }), 3, READS.append({ frames: 1, stored: 1, others: 0, dups: 0 })),
    m(req('append', T('alice'), { body: { $batch: [F('alice', 3), F('rec', 2), F('alice', 4), F('rec', 3)] } }), 10, READS.append({ frames: 4, stored: 4, others: 1, dups: 0 })),
    m(req('append', T('alice'), { body: { $batch: [F('alice', 3), F('rec', 2), F('alice', 4), F('rec', 3)] } }), 0, READS.append({ frames: 4, stored: 0, others: 1, dups: 4 })),
    m(req('append', DOOR, { body: { $batch: [F('rec', 4)] } }), 3, READS.append({ frames: 1, stored: 1, others: 1, dups: 0 }) - 2),
    m(req('append', T('alice'), { body: { $batch: [F('alice', 9)] } }), 0, READS.append({ frames: 1, stored: 0, others: 0, dups: 0 }), { status: 409 }),
    m(req('append', T('alice'), { body: { $batch: range(16, (i) => F('alice', 5 + i)) } }), 33, READS.append({ frames: 16, stored: 16, others: 0, dups: 0 })),
  ]),
  rw('blobs', [
    m(req('blob-begin', T('alice'), { body: { $blob_begin: { id: ID(1), purpose: 4, seq: 0, size: 67 } } }), 1, READS.blobBegin(0)),
    m(req('blob-begin', T('alice'), { body: { $blob_begin: { id: ID(1), purpose: 4, seq: 0, size: 67 } } }), 0, READS.blobBegin(1)),
    m(req('blob-put', T('alice'), { params: { id: ID(1), part: 0 }, body: { $part: { size: 67, index: 0 } } }), 2, READS.blobPut(1)),
    m(req('blob-put', T('alice'), { params: { id: ID(1), part: 0 }, body: { $part: { size: 67, index: 0 } } }), 0, READS.blobPutSame),
    m(req('blob-list', T('bob')), 0, READS.blobList(1)),
    m(req('blob-meta', T('bob'), { params: { id: ID(1) } }), 0, READS.blobMeta),
    m(req('blob-get', T('bob'), { params: { id: ID(1), part: 0 } }), 0, READS.blobGet),
    step(req('blob-begin', T('alice'), { body: { $blob_begin: { id: ID(2), purpose: 4, seq: 0, size: 67 } } })),
    step(req('blob-put', T('alice'), { params: { id: ID(2), part: 0 }, body: { $part: { size: 67, index: 0 } } })),
    step(req('blob-begin', T('alice'), { body: { $blob_begin: { id: ID(4), purpose: 4, seq: 0, size: 1048577 } } })),
    m(req('blob-put', T('alice'), { params: { id: ID(4), part: 0 }, body: { $part: { size: 1048577, index: 0 } } }), 2, READS.blobPut(3)),
    step(req('blob-begin', T('alice'), { body: { $blob_begin: { id: ID(3), purpose: 4, seq: 0, size: 67 } } })),
    m(req('blob-put', T('alice'), { params: { id: ID(3), part: 0 }, body: { $part: { size: 67, index: 0 } } }), 4, READS.blobPut(4) + READS.blobDelete(1)),
  ]),
  rw('pairing', [
    m(req('pair-open', T('alice'), { body: { $cat: [tid, slotA] } }), 1, READS.pairOpen(0)),
    m(req('pair-open', T('alice'), { body: { $cat: [tid, slotA] } }), 0, READS.pairOpen(1)),
    m(req('slot-get', DOOR, { params: { tid, label: 'a' }, proof: K(1) }), 0, READS.slotGet - 1),
    m(req('slot-post', DOOR, { params: { tid, label: 'b' }, proof: K(1), body: slotB }), 1, READS.slotPost - 1),
    m(req('slot-post', DOOR, { params: { tid, label: 'b' }, proof: K(1), body: slotB }), 1, READS.slotPost - 1),
    m(req('slot-post', T('alice'), { params: { tid, label: 'c' }, body: slotC }), 1, READS.slotPost),
    m(req('slot-post', T('alice'), { params: { tid, label: 'c' }, body: slotC }), 0, READS.slotPost),
    m(req('slot-get', T('alice'), { params: { tid, label: 'b' } }), 0, READS.slotGet),
    m(req('slot-post', DOOR, { params: { tid, label: 'b' }, proof: K(1), body: slotB }), 1, READS.slotPost - 1),
    m(req('slot-post', DOOR, { params: { tid, label: 'b' }, proof: K(1), body: slotB }), 1, READS.slotPost - 1),
    m(req('slot-post', DOOR, { params: { tid, label: 'b' }, proof: K(1), body: slotB }), 1, READS.slotPost - 1, err(409, 'relay/tries')),
    step(req('pair-open', T('alice'), { body: { $cat: [{ $tid: K(2) }, slotA] } })),
    m(req('pair-close', T('alice'), { params: { tid: { $tid: K(2) } } }), 1, READS.slotPost),
  ]),
  rw('register: frame mode, its repeat, seq mode', [
    adm('register', 'alice', T('alice'), { dev: 'carol', sign_pk: 'carol', frame_hash: H(F('alice', 2)) }, 5, READS.register(D3, true), { extra: F('alice', 2) }),
    adm('register', 'alice', T('alice'), { dev: 'carol', sign_pk: 'carol', frame_hash: H(F('alice', 2)) }, 2, READS.registerRepeat(D3 + 1), { extra: F('alice', 2) }),
    adm('register', 'alice', T('alice'), { dev: 'dave', sign_pk: 'dave', frame_hash: H(F('alice', 2)) }, 2, READS.register(D3 + 1, false), { extra: { $u64: 3 } }),
    adm('register', 'alice', T('alice'), { dev: 'x01', sign_pk: 'x01', frame_hash: H(F('alice', 9)) }, 1, READS.adminRefused(D3 + 2), { extra: { $u64: 3 } }, err(400, 'relay/frame_mismatch')),
  ]),
  rw('revoke and reinstate, with a backup-read token to revoke', [
    adm('mint_read_token', 'alice', T('alice'), { dev: 'alice', epoch: 1 }, 2, READS.mint(0)),
    adm('revoke', 'alice', T('alice'), { dev: 'bob' }, 3, READS.revoke(D3, 1)),
    adm('mint_read_token', 'alice', T('alice'), { dev: 'alice', epoch: 2 }, 3, READS.mint(0) + 1),
    adm('reinstate', 'rec', DOOR, { dev: 'bob', first_epoch: 2, wraps: 1 }, 3, READS.revoke(D3, 1) - 1),
    adm('revoke_read_token', 'alice', T('alice'), { dev: 'alice' }, 2, READS.revokeRead),
    adm('revoke', 'alice', T('alice'), { dev: 'alice' }, 1, READS.adminRefused(D3), {}, err(403, 'relay/not_signer')),
  ]),
  rw('fence without a frame', [
    adm('fence', 'alice', T('alice'), { host: 'new.relay.test', door: '0102030405', home_gen: 2 }, 2, READS.fence(false)),
  ]),
  rw('fence with its frame', [
    adm('fence', 'alice', T('alice'), { host: 'new.relay.test', door: '0102030405', home_gen: 2 }, 5, READS.fence(true), { extra: F('alice', 2) }),
  ]),
  rw('restore_tail: 1 + 2f + a', [
    adm('restore_tail', 'alice', T('alice'), { first_seq: 3, hashes: [H(F('alice', 2)), H(F('rec', 2)), H(F('alice', 3))] }, 9, READS.restore(3, 2),
      { extra: { $cat: [{ $chain: 2 }, { $frames: [F('alice', 2), F('rec', 2), F('alice', 3)] }] } }),
  ]),
  rw('purge: 2 + r + d and the blobs it deletes', [
    step(req('blob-begin', T('alice'), { body: { $blob_begin: { id: ID(1), purpose: 2, seq: 2, size: 67 } } })),
    step(req('blob-put', T('alice'), { params: { id: ID(1), part: 0 }, body: { $part: { size: 67, index: 0 } } })),
    append(T('alice'), range(8, (i) => F('alice', 2 + i)), range(8, (i) => N(3 + i))),
    adm('mint_read_token', 'alice', T('alice'), { dev: 'alice', epoch: 1 }, 2, READS.mint(0)),
    adm('purge', 'alice', T('alice'), { before_seq: 8, epoch: 2 }, 2 + 1 + 7 + 2, READS.purge(1, 7, 1)),
    adm('purge', 'alice', T('alice'), { before_seq: 8, epoch: 2 }, 2, READS.purge(0, 0, 0)),
  ]),
  rw('recovery_register and its repeat', [
    append(DOOR, [F('rec', 2)], [N(3)]),
    adm('recovery_register', 'rec', DOOR, { dev: 'rec2', sign_pk: 'rec2', frame_hash: H(F('rec', 2)) }, 3, READS.recovery(D3), { extra: { $u64: 3 } }),
    adm('recovery_register', 'alice', T('alice'), { dev: 'rec2', sign_pk: 'rec2', frame_hash: H(F('rec', 2)) }, 1, READS.recoveryRepeat(D3 + 1), { extra: { $u64: 3 } }),
  ]),
  rw('seed and its tail', [
    adm('seed', 'alice', CREATE, { home_gen: 2, first_seq: 5, chain_before: { $fill: { len: 32, seed: 'x' } }, last_seq: 6,
      chain_last: { $chain_from: { before: { $fill: { len: 32, seed: 'x' } }, first: 5, frames: [F('alice', 1), F('rec', 1)] } },
      entries: [{ dev: 'alice', sign_pk: 'alice', role: 1, status: 1, last_ctr: 0 }, { dev: 'rec', sign_pk: 'rec', role: 2, status: 1, last_ctr: 0 }] }, 4, READS.seed(2)),
    m(req('seed-tail', T('alice'), { body: { $cat: [{ $u64: 5 }, { $u16: 2 }, { $frames: [F('alice', 1), F('rec', 1)] }] } }), 7, READS.seedTail(2, 2)),
  ], { state: [], boards: { main: { create: false } } }),
];
