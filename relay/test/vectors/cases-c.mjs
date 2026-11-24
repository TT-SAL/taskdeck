// Families: seed, restore, purge, compact, budget, socket, standby (relay.md §21).
import { ALL, CORE, CREATE, DAY, DOOR, DUP, F, LOCAL, N, NOW, PUSHING, REF, RT, T, addDevice, admin, append, at, err, fillLog, gateErr, kase, login, range, req, step } from './dsl.mjs';

const ID = (n) => n.toString(16).padStart(2, '0').repeat(16);
const K = (n) => n.toString(16).padStart(2, '0').repeat(32);
const H = (x) => ({ $hash: x });
const used = (status, code, extra) => ({ ...err(status, code, 'object', extra), rows_written: 1 });
const nonce = (cred, signer, more) => step(req('admin-nonce', cred, { body: { $dev: signer } }), { status: 200 }, more);
const op = (name, cred, body, expect) => step(req('admin', cred, { params: { op: name }, body: { $admin: { op: name, ...body } } }), expect);
const small = (cred, id, purpose, seq, expect = { status: 200 }) => [
  step(req('blob-begin', cred, { body: { $blob_begin: { id, purpose, seq, size: 67 } } })),
  step(req('blob-put', cred, { params: { id, part: 0 }, body: { $part: { size: 67, index: 0 } } }), expect),
];
const NEXT_DAY = (Math.floor(NOW / DAY) + 1) * DAY;

// ---------- relay-seed.json (§12.7)
const sd = (name, clauses, steps, o) => kase('seed', name, clauses, steps, { boards: { main: { create: false } }, ...o });
const X = { $fill: { len: 32, seed: 'the chain of the old relay' } };
const ENT = (key, role, status, last_ctr, more = {}) => ({ dev: key, sign_pk: key, role, status, last_ctr, ...more });
const SET = [ENT('alice', 1, 1, 2), ENT('bob', 1, 1, 0), ENT('rec', 2, 1, 7)];
const noTail = (o = {}) => ({ home_gen: 2, first_seq: 101, chain_before: X, last_seq: 100, chain_last: X, entries: SET, ...o });
const seed = (args, expect, o = {}) => admin('seed', o.signer || 'alice', CREATE, args, expect, o);
const tail = (first, frames, expect, cred = T('alice')) => step(req('seed-tail', cred, { body: { $cat: [{ $u64: first }, { $u16: frames.length }, { $frames: frames }] } }), expect);
const T1 = F('alice', 3), T2 = F('bob', 1), T3 = F('rec', 8);
const withTail = (frames, o = {}) => ({ home_gen: 2, first_seq: 5, chain_before: X, last_seq: 4 + frames.length, chain_last: { $chain_from: { before: X, first: 5, frames } }, entries: SET, ...o });
const seeding = (r) => step(r, { ...err(409, 'relay/seeding'), rows_written: 0 });
export const seedF = [
  sd('a seed without a tail opens the board at the old relay\'s head', ['RELAY-SEED-001', 'RELAY-SEED-002', 'RELAY-OBJ-001', 'RELAY-OBJ-002', 'RELAY-ROWS-001', 'RELAY-SEQ-002', 'RELAY-READ-002', 'RELAY-DOOR-003'], [
    seed(noTail(), { status: 200, x_td_relay: 'object', json: { door: { $door: true }, token: { $token: { key: 'alice' } }, next_seq: 101, state: 'open' }, rows_written: 5,
      storage: {
        board: { schema: 1, state: 1, created_at: { $time: 0 }, base_seq: 100, base_chain: X, compact_floor: 101, purge_floor: 101, purge_done: 101, compact_snap: null, read_epoch: 1, home_gen: 2, fence_host: null, seed_signer: null, seed_last_seq: null, seed_chain: null },
        devices: { alice: { role: 1, state: 1, gen: 1, last_ctr: 2, last_seq: 0, wraps: null }, bob: { role: 1, state: 1, gen: 0, last_ctr: 0 }, rec: { role: 2, state: 1, gen: 0, last_ctr: 7, last_seq: 0 } },
        log: [], count: { devices: 3, nonces: 0 },
      } }),
    step(req('hello', T('alice')), { status: 200, json: { head: { seq: 100, chain: X }, last_ctr: 2, last_seq: 0, appends_left: 3000, relay_now: { $time: 0 }, floor: 101, purge_floor: 101, state: 'open', door: { $door: true }, push: '$push' } }),
    step(req('read', T('alice'), { query: 'after=0' }), { status: 200, page: { head_seq: 100, floor: 101, purge_floor: 101, more: false, truncated: true, seqs: [] } }),
    step(req('read', T('alice'), { query: 'after=100' }), { status: 200, page: { head_seq: 100, truncated: false, seqs: [] } }),
    append(T('alice'), [F('alice', 2)], [{ status: 'conflict', seq: 0, frame: null }], 409, { rows_written: 0 }),
    append(T('alice'), [F('alice', 3)], [N(101)], 200, { rows_written: 3 }),
    append(DOOR, [F('rec', 8)], [N(102)]),
    append(T('alice'), [F('bob', 1)], [REF(403, 'relay/not_author')], 403),
    login('bob', 1),
    append(T('bob'), [F('bob', 1)], [N(103)]),
    step(req('read', T('bob'), { query: 'after=100' }), { status: 200, page: { head_seq: 103, seqs: [101, 102, 103] } }),
    step(req('create', CREATE, { body: { $create: { board: 'main' } } }), { ...err(409, 'relay/exists'), rows_written: 0 }),
    step(req('admin-nonce', CREATE, { body: { $dev: 'alice' } }), { ...err(409, 'relay/exists'), rows_written: 0 }),
    admin('fence', 'alice', T('alice'), { host: 'old.relay.test', door: '0102030405', home_gen: 2 }, used(409, 'relay/home_gen')),
    tail(104, [F('alice', 4)], { ...err(409, 'relay/target'), rows_written: 0 }),
  ]),
  sd('a seed with a tail: the board is seeding until its last frame is stored', ['RELAY-SEED-003', 'RELAY-SEED-004', 'RELAY-SEED-005', 'RELAY-OBJ-001', 'RELAY-OBJ-002', 'RELAY-ROWS-001', 'RELAY-ENC-006'], [
    seed(withTail([T1, T2, T3]), { status: 200, json: { door: { $door: true }, token: { $token: { key: 'alice' } }, next_seq: 5, state: 'seeding' }, rows_written: 5,
      storage: { board: { state: 3, base_seq: 4, base_chain: X, compact_floor: 5, purge_floor: 5, purge_done: 5, seed_signer: { $dev: 'alice' }, seed_last_seq: 7, seed_chain: { $chain_from: { before: X, first: 5, frames: [T1, T2, T3] } } } } }),
    ...[req('hello', T('alice')), req('read', T('alice'), { query: 'after=0' }), req('append', T('alice'), { body: { $batch: [T1] } }), req('append', DOOR, { body: { $batch: [T3] } }),
      req('login-nonce', DOOR, { body: { $dev: 'bob' } }), req('create', CREATE, { body: { $create: { board: 'main' } } }), req('blob-list', T('alice')),
      req('blob-begin', T('alice'), { body: { $blob_begin: { id: ID(1), purpose: 4, seq: 0, size: 67 } } }), req('admin-nonce', T('alice'), { body: { $dev: 'alice' } }),
      req('admin-nonce', DOOR, { body: { $dev: 'rec' } }), req('pair-open', T('alice'), { body: { $fill: { len: 100, seed: 'x' } } }),
      req('admin', T('alice'), { params: { op: 'revoke' }, body: { $admin: { op: 'revoke', signer: 'alice', nonce: { $hex: K(0) }, args: { dev: 'bob' } } } }),
    ].map(seeding),
    tail(6, [T2], { ...err(409, 'relay/not_next', 'object', { head: { seq: 4, chain: X } }), rows_written: 0 }),
    tail(5, [T1, T2], { status: 200, x_td_relay: 'object', json: { head: { seq: 6, chain: { $chain: 6 } }, state: 'seeding' }, rows_written: 6, storage: { log: [5, 6], devices: { alice: { last_ctr: 3, last_seq: 5, app_count: 0 }, bob: { last_ctr: 1, last_seq: 6 } } } }),
    tail(5, [T1, T2], { ...err(409, 'relay/not_next', 'object', { head: { seq: 6, chain: { $chain: 6 } } }), rows_written: 0 }),
    tail(7, [T3, F('alice', 4)], { ...err(409, 'relay/target'), rows_written: 0 }),
    seeding(req('hello', T('alice'))),
    tail(7, [T3], { status: 200, json: { head: { seq: 7, chain: { $chain: 7 } }, state: 'open' }, rows_written: 4, storage: { board: { state: 1, seed_signer: null, seed_last_seq: null, seed_chain: null }, log: [5, 6, 7] } }),
    step(req('hello', T('alice')), { status: 200, json: { head: { seq: 7, chain: { $chain: 7 } }, last_ctr: 3, last_seq: 5, appends_left: 3000, relay_now: { $time: 0 }, floor: 5, purge_floor: 5, state: 'open', door: { $door: true }, push: '$push' } }),
    step(req('read', T('alice'), { query: 'after=4' }), { status: 200, page: { head_seq: 7, floor: 5, truncated: false, seqs: [5, 6, 7], frames: [T1, T2, T3] } }),
    tail(8, [F('alice', 4)], { ...err(409, 'relay/target'), rows_written: 0 }),
    append(T('alice'), [F('alice', 4)], [N(8)]),
  ]),
  sd('a tail frame that does not fit refuses the whole request; a new seed replaces an unfinished one', ['RELAY-SEED-003', 'RELAY-SEED-004', 'RELAY-TOKEN-005', 'RELAY-COMPACT-006', 'RELAY-ENC-006', 'RELAY-ENC-003'], [
    seed(withTail([T1, T2], { chain_last: { $fill: { len: 32, seed: 'not the chain of this tail' } } }), { status: 200 }),
    ...[
      [[F('mallory', 1)], 400, 'relay/unknown_author', 0],
      [[T1, F('bob', 1, { sig: 'flip' })], 400, 'relay/bad_signature', 1],
      [[F('alice', 4)], 409, 'relay/gap', 0],
      [[F('alice', 2)], 409, 'relay/conflict', 0],
      [[T1, F('alice', 3, { fill: 'again' })], 409, 'relay/conflict', 1],
      [[F('alice', 3, { magic: 'TDF2' })], 400, 'relay/bad_frame', 0],
      [[T1, F('bob', 1, { board_id: '00'.repeat(16) })], 400, 'relay/wrong_board', 1],
      [[F('alice', 0, { ctr_hex: 'ffffffffffffffff' })], 400, 'relay/bad_frame', 0],
    ].map(([frames, status, code, index]) => tail(5, frames, { ...err(status, code, 'object', { index }), rows_written: 0 })),
    step(req('seed-tail', T('alice'), { body: { $cat: [{ $u64: 5 }, { $u16: 0 }] } }), { ...err(400, 'relay/bad_body'), rows_written: 0 }),
    step(req('seed-tail', T('alice'), { body: { $cat: [{ $u64: 5 }, { $u16: 2 }, { $frames: [T1] }] } }), { ...err(400, 'relay/bad_body'), rows_written: 0 }),
    step(req('seed-tail', T('alice'), { body: { $cat: [{ $u64: 5 }, { $u16: 1 }, { $frames: [T1] }, { $hex: '00' }] } }), { ...err(400, 'relay/bad_body'), rows_written: 0 }),
    step(req('seed-tail', T('alice'), { body: { $cat: [{ $u64: 'ffffffffffffffff' }, { $u16: 1 }, { $frames: [T1] }] } }), { ...err(409, 'relay/not_next', 'object', { head: { seq: 4, chain: X } }), rows_written: 0 }),
    tail(5, [T1, T2], { ...err(409, 'relay/chain_mismatch'), rows_written: 0, storage: { log: [], board: { state: 3 } } }),
    tail(5, [T1], { status: 200, json: { head: { seq: 5, chain: { $chain: 5 } }, state: 'seeding' }, rows_written: 3 }),
    seed(withTail([T1, T2]), { status: 200, json: { door: { $door: true }, token: { $token: { key: 'alice' } }, next_seq: 5, state: 'seeding' }, rows_written: 10, storage: { log: [], count: { devices: 3 }, devices: { alice: { last_ctr: 2, gen: 1 } } } }),
    tail(5, [T1, T2], { status: 200, json: { head: { seq: 6, chain: { $chain: 6 } }, state: 'open' }, rows_written: 7 }),
  ]),
  sd('what seed refuses', ['RELAY-SEED-001', 'RELAY-SEED-002', 'RELAY-ADMIN-002', 'SEAL-SIG-004', 'SEAL-KEY-006'], [
    ...[
      noTail({ home_gen: 1 }), noTail({ first_seq: 0, last_seq: 0 }), noTail({ last_seq: 99 }), noTail({ chain_last: { $fill: { len: 32, seed: 'another' } } }),
      noTail({ sort: 'reverse' }), noTail({ entries: [ENT('alice', 1, 1, 0), ENT('alice', 1, 1, 0), ENT('rec', 2, 1, 1)], sort: false }),
      noTail({ entries: [...SET, { dev: { $dev: 'carol' }, sign_pk: 'bob', role: 1, status: 2, last_ctr: 0 }] }),
      noTail({ entries: [ENT('alice', 1, 1, 0), ENT('bob', 3, 1, 0), ENT('rec', 2, 1, 1)] }), noTail({ entries: [ENT('alice', 1, 1, 0), ENT('bob', 1, 0, 0), ENT('rec', 2, 1, 1)] }),
      noTail({ entries: [ENT('alice', 1, 1, 0), ENT('rec', 2, 1, 1), { dev: { $dev: 'carol' }, sign_pk: 'rec2', role: 2, status: 2, last_ctr: 0 }] }),
      noTail({ entries: [ENT('alice', 1, 1, 0), ENT('rec', 2, 1, 1), ENT('rec2', 2, 1, 0)] }), noTail({ entries: [ENT('alice', 1, 1, 0), ENT('rec', 2, 2, 1)] }),
      noTail({ entries: [ENT('alice', 1, 1, 0), ENT('rec', 2, 1, 1), ...range(16, (i) => ENT(`x${String(i + 1).padStart(2, '0')}`, 1, 1, 0))] }),
      noTail({ entries: [ENT('alice', 1, 1, 0, { last_ctr: '0020000000000000' }), ENT('rec', 2, 1, 1)] }),
    ].map((args) => seed(args, { ...used(400, 'relay/bad_args'), storage: { board: null, count: { devices: 0 } } })),
    seed(noTail(), used(403, 'relay/not_signer'), { signer: 'carol' }),
    seed(noTail({ entries: [ENT('alice', 1, 2, 0), ENT('bob', 1, 1, 0), ENT('rec', 2, 1, 1)] }), used(403, 'relay/not_signer')),
    seed(noTail(), used(403, 'relay/not_signer'), { signer: 'rec' }),
    seed(noTail(), used(401, 'relay/bad_admin_sig'), { admin: { sig: 'flip' } }),
    seed(noTail(), used(401, 'relay/bad_admin_sig'), { admin: { sign_key: 'bob' } }),
    nonce(CREATE, 'alice'),
    op('seed', CREATE, { signer: 'bob', args: noTail() }, { ...err(401, 'relay/no_nonce'), rows_written: 0 }),
    nonce(CREATE, 'alice', { save: { n: 'nonce' } }),
    op('seed', CREATE, { signer: 'alice', nonce: '$saved.n', args: noTail({ n: 0, entries: [] }) }, { ...err(400, 'relay/bad_args'), rows_written: 0 }),
    op('seed', CREATE, { signer: 'alice', nonce: '$saved.n', args: noTail({ n: 4 }) }, { ...err(400, 'relay/bad_args'), rows_written: 0 }),
    step(req('hello', T('alice')), { ...err(401, 'relay/unauthorized'), storage: { board: null, count: { devices: 0, log: 0 } } }),
    op('seed', CREATE, { signer: 'alice', nonce: '$saved.n', args: noTail() }, { status: 200 }),
  ]),
];

// ---------- relay-restore.json (§12.9)
const rs = (name, clauses, steps, o) => kase('restore', name, clauses, steps, { state: [addDevice('bob', 2)], ...o });
const restore = (signer, cred, first, frames, before, expect, o = {}) =>
  admin('restore_tail', signer, cred, { first_seq: first, hashes: (o.hashes || frames).map(H) }, expect, { extra: { $cat: [before, { $frames: frames }] } });
const R1 = F('alice', 2), R2 = F('rec', 2), R3 = F('bob', 1);
export const restoreF = [
  rs('frames are restored at the head when every one is signed by a registered author', ['RELAY-RESTORE-001', 'RELAY-RESTORE-002', 'RELAY-ROWS-001', 'RELAY-BUDGET-001', 'RELAY-SOCK-008'], [
    restore('alice', T('alice'), 3, [R1, R2, R3], { $chain: 2 }, { status: 200, x_td_relay: 'object', json: { head: { seq: 5, chain: { $chain: 5 } } }, rows_written: 10,
      storage: { log: [1, 2, 3, 4, 5], devices: { alice: { last_ctr: 2, last_seq: 3, app_count: 1 }, rec: { last_ctr: 2, last_seq: 4 }, bob: { last_ctr: 1, last_seq: 5, app_count: 0, state: 1 } } } }),
    step(req('read', T('bob'), { query: 'after=2' }), { status: 200, page: { head_seq: 5, seqs: [3, 4, 5], frames: [R1, R2, R3] } }),
    restore('rec', DOOR, 6, [F('rec', 3)], { $chain: 5 }, { status: 200, json: { head: { seq: 6, chain: { $chain: 6 } } }, rows_written: 4 }),
    admin('revoke', 'alice', T('alice'), { dev: 'bob' }, { status: 200 }),
    restore('alice', T('alice'), 7, [F('bob', 2)], { $chain: 6 }, { status: 200, json: { head: { seq: 7, chain: { $chain: 7 } } }, storage: { devices: { bob: { state: 2, gen: 2, last_ctr: 2, last_seq: 7, sign_pk: { $key: 'bob' } } } } }),
    append(T('alice'), [F('alice', 3)], [N(8)]),
    admin('register', 'alice', T('alice'), { dev: 'carol', sign_pk: 'carol', frame_hash: H(F('alice', 3)) }, { status: 200, json: { seq: 8, chain: { $chain: 8 }, token: { $token: { key: 'carol' } } }, rows_written: 2 }, { extra: { $u64: 8 } }),
  ]),
  rs('what restore_tail refuses, with the frame\'s index', ['RELAY-RESTORE-001', 'RELAY-RESTORE-002', 'RELAY-ENC-006', 'RELAY-OBJ-009'], [
    restore('alice', T('alice'), 5, [R1], { $chain: 2 }, used(409, 'relay/not_next', { head: { seq: 2, chain: { $chain: 2 } } })),
    restore('alice', T('alice'), 2, [R1], { $chain: 1 }, used(409, 'relay/not_next', { head: { seq: 2, chain: { $chain: 2 } } })),
    restore('alice', T('alice'), 3, [R1], { $chain: 1 }, used(409, 'relay/chain_mismatch')),
    restore('alice', T('alice'), 3, [R1], { $chain: 2 }, used(400, 'relay/frame_mismatch', { index: 0 }), { hashes: [F('alice', 3)] }),
    restore('alice', T('alice'), 3, [R1, R2], { $chain: 2 }, used(400, 'relay/frame_mismatch', { index: 1 }), { hashes: [R1, R3] }),
    restore('alice', T('alice'), 3, [F('mallory', 1)], { $chain: 2 }, used(400, 'relay/unknown_author', { index: 0 })),
    restore('alice', T('alice'), 3, [R1, F('rec', 2, { sig: 'zero-s' })], { $chain: 2 }, used(400, 'relay/bad_signature', { index: 1 })),
    restore('alice', T('alice'), 3, [F('alice', 4)], { $chain: 2 }, used(409, 'relay/gap', { index: 0 })),
    restore('alice', T('alice'), 3, [F('alice', 1)], { $chain: 2 }, used(409, 'relay/conflict', { index: 0 })),
    restore('alice', T('alice'), 3, [R1, F('alice', 2, { fill: 'twice' })], { $chain: 2 }, used(409, 'relay/conflict', { index: 1 })),
    restore('alice', T('alice'), 3, [F('alice', 2, { len_delta: 1 })], { $chain: 2 }, used(400, 'relay/bad_frame', { index: 0 })),
    restore('rec', T('bob'), 3, [R1], { $chain: 2 }, { status: 200, storage: { log: [1, 2, 3] } }),
  ]),
];

// ---------- relay-purge.json (§12.10)
const pg = (name, clauses, steps, o) => kase('purge', name, clauses, steps, o);
const purge = (before, epoch, expect, signer = 'alice') => admin('purge', signer, T('alice'), { before_seq: before, epoch }, expect);
export const purgeF = [
  pg('purge raises the floor at once and drops what lies below it', ['RELAY-PURGE-001', 'RELAY-PURGE-002', 'RELAY-PURGE-003', 'RELAY-STORE-005', 'RELAY-STORE-007', 'RELAY-SEQ-002', 'RELAY-ROWS-001', 'RELAY-READ-002', 'RELAY-TOKEN-004'], [
    purge(10, 2, used(403, 'relay/not_signer'), 'rec'),
    purge(10, 0, used(400, 'relay/bad_args')),
    purge(22, 2, used(409, 'relay/target')),
    purge(10, 2, { status: 200, x_td_relay: 'object', json: { purge_floor: 10, done: 10 }, rows_written: 14,
      storage: { board: { purge_floor: 10, purge_done: 10, compact_floor: 1, read_epoch: 2, del_count: 9 }, read_tokens: [{ state: 2 }], blobs: [{ id: ID(2), purpose: 4 }], count: { parts: 1 }, log: range(12, (i) => 10 + i) } }),
    step(req('read', T('alice'), { query: 'after=0' }), { status: 200, page: { head_seq: 21, floor: 10, purge_floor: 10, truncated: true, more: false, seqs: range(12, (i) => 10 + i) } }),
    step(req('read', T('alice'), { query: 'after=9' }), { status: 200, page: { truncated: false, seqs: range(12, (i) => 10 + i) } }),
    step(req('hello', RT('alice')), err(401, 'relay/token_stale')),
    step(req('blob-meta', T('alice'), { params: { id: ID(1) } }), err(404, 'relay/no_blob')),
    append(T('alice'), [F('alice', 3)], [{ status: 'conflict', seq: 0, frame: null }], 409, { rows_written: 0 }),
    append(T('alice'), [F('alice', 9)], [DUP(10)], 200, { rows_written: 0 }),
    admin('register', 'alice', T('alice'), { dev: 'carol', sign_pk: 'carol', frame_hash: H(F('alice', 4)) }, used(400, 'relay/frame_mismatch'), { extra: { $u64: 5 } }),
    purge(10, 2, { status: 200, json: { purge_floor: 10, done: 10 }, rows_written: 2 }),
    purge(9, 3, used(409, 'relay/target')),
    purge(21, 5, { status: 200, json: { purge_floor: 21, done: 21 }, rows_written: 13, storage: { board: { purge_floor: 21, purge_done: 21, read_epoch: 5 }, log: [21] } }),
    step(req('hello', T('alice')), { status: 200, json: { head: { seq: 21, chain: { $chain: 21 } }, last_ctr: 20, last_seq: 21, appends_left: 2980, relay_now: '$any', floor: 21, purge_floor: 21, state: 'open', door: { $door: true }, push: '$push' } }),
    admin('mint_read_token', 'alice', T('alice'), { dev: 'alice', epoch: 2 }, used(409, 'relay/stale_epoch', { read_epoch: 5 })),
    append(T('alice'), [F('alice', 21)], [N(22)]),
  ], { state: [addDevice('bob', 2), fillLog('alice', T('alice'), 2, 19), small(T('alice'), ID(1), 2, 5), small(T('alice'), ID(2), 4, 0), admin('mint_read_token', 'alice', T('alice'), { dev: 'alice', epoch: 1 })] }),
  pg('the rows themselves go within the day\'s delete budget', ['RELAY-PURGE-003', 'RELAY-COMPACT-005', 'RELAY-STORE-005', 'RELAY-SEQ-002'], [
    purge(5100, 2, { status: 200, json: { purge_floor: 5100, done: 5001 }, rows_written: 5002, storage: { board: { purge_floor: 5100, purge_done: 5001, del_count: 5000, del_day: Math.floor(NOW / DAY) } } }),
    step(req('read', T('alice'), { query: 'after=0&limit=5' }), { status: 200, page: { head_seq: 5102, floor: 5100, purge_floor: 5100, truncated: true, seqs: [5100, 5101, 5102] } }),
    step(req('read', T('alice'), { query: 'after=5050' }), { status: 200, page: { truncated: true, seqs: [5100, 5101, 5102] } }),
    append(DOOR, [F('rec', 5050)], [{ status: 'conflict', seq: 0, frame: null }], 409, { rows_written: 0 }),
    append(DOOR, [F('rec', 5100)], [DUP(5100)], 200),
    purge(5100, 2, { status: 200, json: { purge_floor: 5100, done: 5001 }, rows_written: 2 }),
    at(NEXT_DAY - 1, purge(5100, 2, { status: 200, json: { purge_floor: 5100, done: 5001 }, rows_written: 2 })),
    at(NEXT_DAY, purge(5100, 2, { status: 200, json: { purge_floor: 5100, done: 5100 }, rows_written: 101, storage: { board: { purge_done: 5100, del_count: 99, del_day: Math.floor(NOW / DAY) + 1 }, log: [5100, 5101, 5102] } })),
  ], { targets: LOCAL, state: [fillLog('rec', DOOR, 2, 5100), addDevice('bob', 5102)] }),
];

// ---------- relay-compact.json (§12.8, §13.4)
const YEAR = NOW + 366 * DAY, SNAP = ID(0x51), GEN = ID(0x33);
const compact = (args, expect, signer = 'bob') => admin('compact', signer, T(signer), { below_seq: 5100, snap_entry_seq: 21502, blob_id: SNAP, keep: [], ...args }, expect);
const refusedC = (args, status, code, signer) => compact(args, used(status, code), signer);
export const compactF = [
  kase('compact', 'compaction deletes old entries in daily slices, keeps what it is told to keep, and never the newest 16,384', [
    'RELAY-COMPACT-001', 'RELAY-COMPACT-002', 'RELAY-COMPACT-003', 'RELAY-COMPACT-004', 'RELAY-COMPACT-005', 'RELAY-COMPACT-006', 'RELAY-COMPACT-007', 'RELAY-STORE-005', 'RELAY-BLOB-005', 'RELAY-BLOB-007', 'RELAY-ROWS-001', 'RELAY-OBJ-005'], [
    at(NOW + 364 * DAY, compact({}, used(409, 'relay/too_recent'))),
    at(YEAR, refusedC({ keep: [5, 3] }, 400, 'relay/bad_args')),
    refusedC({ keep: [5, 5] }, 400, 'relay/bad_args'), refusedC({ below_seq: 100, keep: [100] }, 400, 'relay/bad_args'),
    refusedC({ below_seq: 1 }, 409, 'relay/target'), refusedC({ below_seq: 5120 }, 409, 'relay/target'), refusedC({ below_seq: 21503 }, 409, 'relay/target'),
    refusedC({ snap_entry_seq: 30000 }, 409, 'relay/no_snapshot'), refusedC({ blob_id: ID(0x99) }, 409, 'relay/no_snapshot'), refusedC({ blob_id: GEN }, 409, 'relay/no_snapshot'),
    refusedC({ blob_id: ID(0x52) }, 409, 'relay/no_snapshot'), refusedC({ snap_entry_seq: 100 }, 409, 'relay/no_snapshot'), refusedC({ snap_entry_seq: 5120 }, 409, 'relay/no_snapshot'),
    refusedC({}, 403, 'relay/not_signer', 'alice'),
    compact({ keep: [1, 7, 4999, 5000, 5050] }, { status: 200, x_td_relay: 'object', json: { deleted: 4996, floor: 5001, more: true }, rows_written: 5000,
      storage: { board: { compact_floor: 5001, purge_floor: 1, compact_snap: SNAP, del_count: 4996, del_day: Math.floor(YEAR / DAY) }, blobs: [{ id: SNAP }, { id: ID(0x52), complete: 0 }] } }),
    step(req('read', T('bob'), { query: 'after=0&limit=6' }), { status: 200, page: { head_seq: 21503, floor: 5001, purge_floor: 1, truncated: true, more: true, seqs: [1, 7, 4999, 5000, 5001, 5002] } }),
    step(req('read', T('bob'), { query: 'after=5000&limit=2' }), { status: 200, page: { truncated: false, seqs: [5001, 5002] } }),
    compact({ keep: [5050] }, { status: 200, json: { deleted: 4, floor: 5005, more: true }, rows_written: 6 }),
    compact({ keep: [5050] }, used(429, 'relay/delete_budget', { until: (Math.floor(YEAR / DAY) + 1) * DAY })),
    admin('purge', 'bob', T('bob'), { before_seq: 100, epoch: 2 }, { status: 200, json: { purge_floor: 100, done: 1 }, rows_written: 2 }),
    step(req('read', T('bob'), { query: 'after=0&limit=3' }), { status: 200, page: { floor: 5005, purge_floor: 100, truncated: true, seqs: [4999, 5000, 5005] } }),
    at(YEAR + DAY, compact({ keep: [5050] }, { status: 200, json: { deleted: 94, floor: 5100, more: false }, rows_written: 96, storage: { board: { compact_floor: 5100, del_count: 94 } } })),
    admin('purge', 'bob', T('bob'), { before_seq: 100, epoch: 2 }, { status: 200, json: { purge_floor: 100, done: 100 }, rows_written: 4, storage: { board: { purge_done: 100, del_count: 96 } } }),
    step(req('read', T('bob'), { query: 'after=0&limit=4' }), { status: 200, page: { floor: 5100, truncated: true, seqs: [4999, 5000, 5050, 5100] } }),
    compact({ keep: [5050] }, used(409, 'relay/target')),
    // The snapshot a compaction relied on outlives three newer ones (§13.4).
    ...[1, 2, 3].map((i) => at(YEAR + DAY + 1000 * i, small(T('alice'), ID(0x60 + i), 1, 5184))),
    at(YEAR + DAY + 4000, small(T('alice'), ID(0x64), 1, 5184, { status: 200, storage: { blobs: [{ id: SNAP }, { id: ID(0x62) }, { id: ID(0x63) }, { id: ID(0x64) }] } })),
  ], { targets: ALL, state: [fillLog('rec', DOOR, 2, 21500), small(T('alice'), GEN, 3, 1), small(T('alice'), SNAP, 1, 5120), append(T('alice'), [F('alice', 1)], [N(21502)]), addDevice('bob', 21503, 'alice', 2),
    step(req('blob-begin', T('alice'), { body: { $blob_begin: { id: ID(0x52), purpose: 1, seq: 5120, size: 67 } } }))] }),
];

// ---------- relay-budget.json (§14)
const U = NEXT_DAY;
export const budgetF = [
  kase('budget', 'a device stores 3,000 new frames in a UTC day of the relay clock, and no more', ['RELAY-BUDGET-001', 'RELAY-BUDGET-002', 'RELAY-HELLO-001', 'RELAY-ENC-006'], [
    step(req('hello', T('alice')), { status: 200, json: { head: '$any', last_ctr: 2999, last_seq: 3000, appends_left: 1, relay_now: '$any', floor: 1, purge_floor: 1, state: 'open', door: '$any', push: '$push' } }),
    append(T('alice'), [F('alice', 3000), F('alice', 3001), F('alice', 3002)], [N(3001), REF(429, 'relay/budget'), 'skipped'], 429, { rows_written: 3, storage: { devices: { alice: { app_count: 3000, app_day: Math.floor(NOW / DAY) } } } }),
    step(req('hello', T('alice')), { status: 200, json: { head: '$any', last_ctr: 3000, last_seq: 3001, appends_left: 0, relay_now: '$any', floor: 1, purge_floor: 1, state: 'open', door: '$any', push: '$push' } }),
    append(T('alice'), [F('alice', 3001)], [REF(429, 'relay/budget')], 429, { rows_written: 0 }),
    append(T('alice'), [F('alice', 5)], [DUP(6)], 200, { rows_written: 0 }),
    append(T('alice'), [F('rec', 2)], [N(3002)], 200, { rows_written: 3 }),
    admin('register', 'alice', T('alice'), { dev: 'bob', sign_pk: 'bob', frame_hash: H(F('alice', 3001)) }, used(429, 'relay/budget', { until: U }), { extra: F('alice', 3001) }),
    admin('fence', 'alice', T('alice'), { host: 'new.relay.test', door: '0102030405', home_gen: 2 }, used(429, 'relay/budget', { until: U }), { extra: F('alice', 3001) }),
    at(U - 1, append(T('alice'), [F('alice', 3001)], [REF(429, 'relay/budget')], 429, { rows_written: 0 })),
    at(U, step(req('hello', T('alice')), { status: 200, json: { head: '$any', last_ctr: 3000, last_seq: 3001, appends_left: 3000, relay_now: U, floor: 1, purge_floor: 1, state: 'open', door: '$any', push: '$push' }, storage: { devices: { alice: { app_count: 3000, app_day: Math.floor(NOW / DAY) } } } })),
    append(T('alice'), [F('alice', 3001)], [N(3003)], 200, { rows_written: 3, storage: { devices: { alice: { app_count: 1, app_day: Math.floor(NOW / DAY) + 1 } } } }),
    step(req('hello', T('alice')), { status: 200, json: { head: '$any', last_ctr: 3001, last_seq: 3003, appends_left: 2999, relay_now: U, floor: 1, purge_floor: 1, state: 'open', door: '$any', push: '$push' } }),
  ], { targets: LOCAL, state: [fillLog('alice', T('alice'), 1, 2999)] }),
  kase('budget', 'a storage call that throws rolls the request back', ['RELAY-BUDGET-003', 'RELAY-OBJ-006', 'RELAY-OBJ-007'], [
    { storage_fault: 'UPDATE devices' },
    append(T('alice'), [F('alice', 1), F('alice', 2)], undefined, 507, { ...err(507, 'relay/storage_refused'), results: undefined, storage: { log: [1], devices: { alice: { last_ctr: 0, last_seq: 0, app_count: 0 } } } }),
    { storage_fault: 'INSERT INTO log' },
    append(T('alice'), [F('alice', 1)], undefined, 507, { ...err(507, 'relay/storage_refused'), results: undefined, storage: { log: [1] } }),
    { storage_fault: 'INSERT INTO nonces' },
    step(req('login-nonce', DOOR, { body: { $dev: 'alice' } }), { ...err(507, 'relay/storage_refused'), storage: { count: { nonces: 0 } } }),
    { storage_fault: 'DELETE FROM nonces WHERE nonce = ?' },
    step(req('login-nonce', DOOR, { body: { $dev: 'alice' } }), { status: 200 }),
    step(req('login', DOOR, { body: { $login: { dev: 'alice', gen: 2 } } }), { ...err(507, 'relay/storage_refused'), storage: { count: { nonces: 1 }, devices: { alice: { gen: 1 } } } }),
    { storage_fault: null },
    append(T('alice'), [F('alice', 1), F('alice', 2)], [N(2), N(3)]),
    { entropy_fault: true },
    step(req('login-nonce', DOOR, { body: { $dev: 'alice' } }), { ...gateErr(503, 'relay/object_unavailable'), object_called: true, storage: { count: { nonces: 1 } } }),
    { entropy_fault: false },
    login('alice', 2),
  ], { targets: CORE }),
];

// ---------- relay-socket.json (§11)
const so = (name, clauses, steps, o) => kase('socket', name, clauses, steps, { targets: PUSHING, state: [addDevice('bob', 2)], ...o });
const S = (n) => n.toString(16).padStart(2, '0').repeat(16);
const sopen = (name, cred, expect = { status: 101 }) => ({ socket: name, open: { board: 'main', cred }, expect });
const ssend = (name, send, expect = {}) => ({ socket: name, send, expect });
const sexp = (name, expect) => ({ socket: name, expect });
const welcome = (o = {}) => ({ type: 'welcome', head_seq: o.head ?? 2, last_ctr: o.last_ctr ?? 0, last_seq: o.last_seq ?? 0, relay_now: { $time: 0 }, floor: 1, purge_floor: 1, state: 1, caps: 1, appends_left: o.left ?? 3000, door: { $door: true }, has_fence: 0 });
const hello = (name, session, w) => ssend(name, { $hello: { session: S(session) } }, { messages: [welcome(w)] });
const ALICE = { last_ctr: 1, last_seq: 2, left: 2999 };
const sappend = (name, reqId, frames, messages) => ssend(name, { $sock_append: { req: reqId, batch: frames } }, { messages });
const push = (seqs, o = {}) => ({ type: 'push', page: { seqs, truncated: false, ...o } });
const bad = { type: 'error', req: 0, status: 400, code: 'relay/bad_message' };
const FENCED = { host: 'new.relay.test', door: '0102030405', home_gen: 2 };
export const socketF = [
  so('hello and welcome; append, read and push over the socket', ['RELAY-SOCK-001', 'RELAY-SOCK-004', 'RELAY-SOCK-005', 'RELAY-SOCK-006', 'RELAY-SOCK-008', 'RELAY-SOCK-012', 'RELAY-APPEND-002', 'RELAY-CAPS-002', 'RELAY-SHAPE-008', 'RELAY-SHAPE-010', 'RELAY-OBJ-007'], [
    sopen('a', T('alice')), sopen('b', T('bob')),
    append(T('alice'), [F('alice', 2)], [N(3)]),
    sexp('a', { silent: true }), sexp('b', { silent: true }),
    hello('a', 1, { head: 3, last_ctr: 2, last_seq: 3, left: 2998 }), hello('b', 2, { head: 3 }),
    sappend('a', 7, [F('alice', 3), F('alice', 4)], [{ type: 'result', req: 7, results: [N(4), N(5)] }, push([4, 5], { head_seq: 5, more: false, fenced: false, frames: [F('alice', 3), F('alice', 4)] })]),
    sexp('b', { messages: [push([4, 5], { head_seq: 5 })] }),
    append(T('bob'), [F('bob', 1)], [N(6)]),
    sexp('a', { messages: [push([6], { head_seq: 6, frames: [F('bob', 1)] })] }), sexp('b', { messages: [push([6])] }),
    ssend('a', { $sock_read: { req: 9, after: 2, limit: 2 } }, { messages: [{ type: 'page', req: 9, page: { head_seq: 6, seqs: [3, 4], more: true, truncated: false } }] }),
    ssend('b', { $sock_read: { req: 4294967295, after: 6, limit: 256 } }, { messages: [{ type: 'page', req: 4294967295, page: { head_seq: 6, seqs: [], more: false } }] }),
    sappend('a', 8, [F('alice', 3)], [{ type: 'result', req: 8, results: [DUP(4)] }]),
    sappend('a', 10, [F('alice', 5), F('alice', 7)], [{ type: 'result', req: 10, results: [N(7), { status: 'gap', last_ctr: 5, last_seq: 7 }] }, push([7], { head_seq: 7 })]),
    sexp('b', { messages: [push([7])] }),
    sappend('b', 1, [F('alice', 6)], [{ type: 'result', req: 1, results: [REF(403, 'relay/not_author')] }]),
    ssend('a', { $sock_append: { req: 5, batch: { $hex: '00' } } }, { messages: [{ type: 'error', req: 5, status: 400, code: 'relay/bad_batch' }] }),
    ssend('a', { $sock_read: { req: 6, after: 0, limit: 0 } }, { messages: [{ type: 'error', req: 6, status: 400, code: 'relay/bad_query' }] }),
    ssend('a', { $sock_read: { req: 6, after: 0, limit: 257 } }, { messages: [{ type: 'error', req: 6, status: 400, code: 'relay/bad_query' }] }),
    ssend('a', { $sock_read: { req: 6, after: 'ffffffffffffffff', limit: 1 } }, { messages: [{ type: 'error', req: 6, status: 400, code: 'relay/bad_query' }] }),
    admin('register', 'alice', T('alice'), { dev: 'carol', sign_pk: 'carol', frame_hash: H(F('alice', 6)) }, { status: 200 }, { extra: F('alice', 6) }),
    sexp('a', { messages: [push([8], { head_seq: 8 })] }), sexp('b', { messages: [push([8])] }),
    admin('restore_tail', 'alice', T('alice'), { first_seq: 9, hashes: [H(F('rec', 2)), H(F('bob', 2))] }, { status: 200 }, { extra: { $cat: [{ $chain: 8 }, { $frames: [F('rec', 2), F('bob', 2)] }] } }),
    sexp('a', { messages: [push([9, 10], { head_seq: 10 })] }), sexp('b', { messages: [push([9, 10])] }),
    sexp('a', { silent: true }), sexp('b', { silent: true }),
  ]),
  so('a second process is told to both ends; a reconnect of the same process is silent', ['RELAY-SOCK-003', 'RELAY-SOCK-011'], [
    sopen('a1', T('alice')), hello('a1', 1, ALICE),
    sopen('a2', T('alice')), sexp('a1', { silent: true }),
    ssend('a2', { $hello: { session: S(2) } }, { messages: [{ type: 'second', session: S(1) }, welcome(ALICE)] }),
    sexp('a1', { messages: [{ type: 'displaced', session: S(2) }], closed: 4001 }),
    sopen('a3', T('alice')), hello('a3', 2, ALICE),
    sexp('a2', { closed: 4002 }),
    sopen('a4', T('alice')), sopen('a5', T('alice')),
    ssend('a5', { $hello: { session: S(3) } }, { messages: [{ type: 'second', session: S(2) }, welcome(ALICE)] }),
    sexp('a3', { messages: [{ type: 'displaced', session: S(3) }], closed: 4001 }),
    sexp('a4', { silent: true }),
    ssend('a4', { $hello: { session: S(4) } }, { messages: [{ type: 'second', session: S(3) }, welcome(ALICE)] }),
    sexp('a5', { messages: [{ type: 'displaced', session: S(4) }], closed: 4001 }),
    sopen('b', T('bob')), hello('b', 4, {}),
    sexp('a4', { silent: true }),
  ]),
  so('a socket that breaks the protocol is closed', ['RELAY-SOCK-002', 'RELAY-SOCK-004', 'RELAY-SOCK-009', 'RELAY-SOCK-011'], [
    sopen('x', T('alice')), ssend('x', { $sock_read: { req: 1, after: 0, limit: 1 } }, { closed: 4004 }),
    sopen('y', T('alice')), ssend('y', { $hex: '01' + '00'.repeat(19) }, { closed: 4004 }),
    sopen('y2', T('alice')), ssend('y2', { $hex: '01' + '00'.repeat(21) }, { closed: 4004 }),
    sopen('y3', T('alice')), ssend('y3', { $text: 'hello' }, { closed: 4004 }),
    sopen('z', T('alice')), hello('z', 1, ALICE), ssend('z', { $hello: { session: S(1) } }, { closed: 4004 }),
    sopen('w', T('alice')), hello('w', 2, ALICE), ssend('w', { $hex: '09000000' }, { messages: [bad], closed: 4004 }),
    sopen('v', T('alice')), hello('v', 3, ALICE), ssend('v', { $hex: '03' + '00'.repeat(13) }, { messages: [bad], closed: 4004 }),
    sopen('u', T('alice')), hello('u', 4, ALICE), ssend('u', { $text: 'hi' }, { messages: [bad], closed: 4004 }),
    sopen('t', T('alice')), hello('t', 5, ALICE), ssend('t', { $hex: '02000000' }, { messages: [bad], closed: 4004 }),
    sopen('s', T('alice')), hello('s', 6, ALICE), ssend('s', { $fill: { len: 4194379, seed: 'too big' } }, { closed: 1009 }),
    sopen('r', T('alice')), hello('r', 7, ALICE), ssend('r', { $hex: '' }, { messages: [bad], closed: 4004 }),
    step(req('hello', T('alice')), { status: 200, storage: { log: [1, 2] } }),
  ]),
  so('the runtime answers the text ping without waking the object', ['RELAY-SOCK-010'], [
    sopen('a', T('alice')), hello('a', 1, ALICE),
    ssend('a', { $text: 'ping' }, { text: 'pong' }),
    ssend('a', { $text: 'ping' }, { text: 'pong' }),
    ssend('a', { $sock_read: { req: 1, after: 2, limit: 1 } }, { messages: [{ type: 'page', req: 1, page: { seqs: [] } }] }),
  ], { targets: ['worker'] }),
  so('a socket is accepted only for a device token that is valid at the object', ['RELAY-SOCK-001', 'RELAY-SOCK-007', 'RELAY-LOGIN-004', 'RELAY-REVOKE-003', 'RELAY-SOCK-011'], [
    sopen('m', T('mallory'), { status: 401, error: 'relay/unauthorized', x_td_relay: 'object' }),
    sopen('n', RT('alice'), { status: 403, error: 'relay/forbidden', x_td_relay: 'gate' }),
    sopen('o', DOOR, { status: 401, error: 'relay/unauthorized', x_td_relay: 'gate' }),
    sopen('a', T('alice')), hello('a', 1, ALICE), sopen('a0', T('alice')),
    login('alice', 2),
    sexp('a', { closed: 4003 }), sexp('a0', { closed: 4003 }),
    sopen('p', T('alice', 1), { status: 401, error: 'relay/token_stale' }),
    sopen('a2', T('alice', 2)), hello('a2', 1, ALICE),
    sopen('b', T('bob')), hello('b', 2, {}),
    admin('revoke', 'alice', T('alice', 2), { dev: 'bob' }, { status: 200 }),
    sexp('b', { closed: 4003 }), sexp('a2', { silent: true }),
    sopen('q', T('bob'), { status: 403, error: 'relay/revoked' }),
    // A repeated create gives a new token and closes nothing: the socket of the old generation is refused at its next message.
    sopen('a3', T('alice', 2)), sopen('a4', T('alice', 2)),
    step(req('create', CREATE, { body: { $create: { board: 'main' } } }), { status: 200 }),
    ssend('a2', { $sock_append: { req: 3, batch: [F('alice', 2)] } }, { messages: [{ type: 'error', req: 3, status: 401, code: 'relay/token_stale' }], closed: 4003 }),
    hello('a3', 5, ALICE),
    ssend('a3', { $sock_read: { req: 4, after: 0, limit: 1 } }, { messages: [{ type: 'error', req: 4, status: 401, code: 'relay/token_stale' }], closed: 4003 }),
    step(req('hello', T('alice', 3)), { status: 200, storage: { log: [1, 2] } }),
  ]),
  so('the slot notice goes to the ticket\'s creator, the state notice to everyone', ['RELAY-SOCK-008', 'RELAY-FENCE-001', 'RELAY-FENCE-002', 'RELAY-SOCK-006', 'RELAY-SOCK-001'], [
    sopen('a', T('alice')), hello('a', 1, ALICE), sopen('b', T('bob')), hello('b', 2, {}),
    step(req('pair-open', T('alice'), { body: { $cat: [{ $tid: K(1) }, { $slot: { label: 'a', body_len: 48 } }] } })),
    step(req('slot-post', DOOR, { params: { tid: { $tid: K(1) }, label: 'b' }, proof: K(1), body: { $slot: { label: 'b', body_len: 119 } } })),
    sexp('a', { messages: [{ type: 'slot', tid: { $tid: K(1) }, label: 66 }] }), sexp('b', { silent: true }),
    step(req('slot-post', DOOR, { params: { tid: { $tid: K(1) }, label: 'b' }, proof: K(1), body: { $slot: { label: 'b', body_len: 119 } } })),
    step(req('slot-post', T('alice'), { params: { tid: { $tid: K(1) }, label: 'c' }, body: { $slot: { label: 'c', body_len: 64 } } })),
    sexp('a', { silent: true }),
    admin('fence', 'alice', T('alice'), FENCED, { status: 200 }, { extra: F('alice', 2) }),
    sexp('a', { messages: [push([3], { head_seq: 3, fenced: true }), { type: 'state', state: 2, has_fence: 1, fence: FENCED }] }),
    sexp('b', { messages: [push([3], { fenced: true }), { type: 'state', state: 2, has_fence: 1, fence: FENCED }] }),
    sappend('a', 2, [F('alice', 3)], [{ type: 'error', req: 2, status: 403, code: 'relay/fenced' }]),
    ssend('b', { $sock_read: { req: 3, after: 2, limit: 5 } }, { messages: [{ type: 'page', req: 3, page: { head_seq: 3, seqs: [3], fenced: true } }] }),
    sopen('c', T('bob')),
    ssend('c', { $hello: { session: S(2) } }, { messages: [{ ...welcome({ head: 3 }), state: 2, has_fence: 1, fence: FENCED }] }),
    sexp('b', { closed: 4002 }),
  ]),
  so('a push is split as pages are', ['RELAY-SOCK-008', 'RELAY-READ-001'], [
    sopen('b', T('bob')), hello('b', 1, {}),
    append(T('alice'), range(5, (i) => F('alice', 2 + i, { body_len: 262032 })), range(5, (i) => N(3 + i))),
    sexp('b', { messages: [push([3, 4, 5, 6], { head_seq: 7, more: true }), push([7], { head_seq: 7, more: false })] }),
    sopen('a', T('alice')), hello('a', 2, { head: 7, last_ctr: 6, last_seq: 7, left: 2994 }),
    sappend('a', 1, range(3, (i) => F('alice', 7 + i, { body_len: 262032 })), [{ type: 'result', req: 1, results: [N(8), N(9), N(10)] }, push([8, 9, 10], { head_seq: 10, more: false })]),
    sexp('b', { messages: [push([8, 9, 10])] }),
  ]),
];

// ---------- relay-standby.json (§15.2)
export const standbyF = [
  kase('standby', 'the standby has no socket and says so after the credential checks', ['RELAY-STBY-001', 'RELAY-CAPS-002', 'RELAY-SHAPE-011'], [
    step(req('socket', T('alice'), { headers: { Upgrade: 'websocket', Connection: 'Upgrade' } }), gateErr(501, 'relay/no_push')),
    step(req('socket', T('alice'), { headers: { Upgrade: 'WebSocket' } }), gateErr(501, 'relay/no_push')),
    step(req('socket', T('mallory'), { headers: { Upgrade: 'websocket' } }), gateErr(501, 'relay/no_push')),
    step(req('socket', { raw: 'Bearer TD1.' + 'A'.repeat(92) }, { headers: { Upgrade: 'websocket' } }), gateErr(401, 'relay/unauthorized')),
    step(req('socket', RT('alice'), { headers: { Upgrade: 'websocket' } }), gateErr(403, 'relay/forbidden')),
    step(req('socket', T('alice')), gateErr(400, 'relay/bad_upgrade')),
    step(req('caps', T('alice')), { status: 200, x_td_relay: 'gate', json: { v: 1, push: false, max_batch: 16, max_frame: 262144, page_entries: 256, page_bytes: 1048576, part_bytes: 1048576 } }),
    step(req('hello', T('alice')), { status: 200, json: { head: '$any', last_ctr: 0, last_seq: 0, appends_left: 3000, relay_now: '$any', floor: 1, purge_floor: 1, state: 'open', door: '$any', push: false } }),
    step(req('login-nonce', DOOR, { body: { $dev: 'alice' } }), { status: 200 }),
    step(req('login', DOOR, { body: { $login: { dev: 'alice', gen: 2 } } }), { status: 200, json: { token: '$any', gen: 2, push: false } }),
  ], { targets: ['standby'] }),
];
