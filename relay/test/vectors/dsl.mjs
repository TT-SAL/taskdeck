// Short forms for writing the cases of the vector families (relay.md §21).
// build.mjs expands them into the JSON files; the harness never sees this file.
import { createHash } from 'node:crypto';

const sha = (s) => createHash('sha256').update(s).digest('hex');

export const ALL = ['core', 'standby', 'worker'];
export const LOCAL = ['core', 'standby']; // time-dependent, or a request shape `wrangler dev` does not pass on
export const CORE = ['core'];
export const PUSHING = ['core', 'worker']; // needs sockets
export const KEYS = Object.fromEntries(['rec', 'rec2', 'alice', 'bob', 'carol', 'dave', 'mallory', ...Array.from({ length: 16 }, (_, i) => `x${String(i + 1).padStart(2, '0')}`)].map((n) => [n, { seed: sha(`taskdeck relay test key ${n}`) }]));
export const NOW = 1795000000000;
export const DAY = 86400000;

// One case. Its board gets a salt of its own, so that cases never share an object.
export function kase(family, name, clauses, steps, o = {}) {
  const salt = (b) => sha(`${family}/${name}/${b}`).slice(0, 32);
  const boards = {};
  for (const [b, v] of Object.entries(o.boards || { main: {} })) boards[b] = { salt: salt(b), recovery: 'rec', first: 'alice', ...v };
  const setup = { keys: KEYS, boards, ...(o.setup || {}) };
  if (o.state) setup.state = o.state.flat(Infinity);
  return { name, clauses, targets: o.targets || ALL, setup, steps: steps.flat(Infinity) };
}
export const fn = (name, clauses, call, args, out) => ({ name, clauses, targets: CORE, call, args, ...(out.error ? { error: out.error } : { expect: out.expect }) });

export const T = (key, gen = 1) => ({ token: key, gen });
export const RT = (key, gen = 1) => ({ read_token: key, gen });
export const DOOR = { door: true };
export const CREATE = { create: true };
export const req = (route, cred, extra = {}) => ({ route, board: 'main', cred, ...extra });
export const step = (request, expect = { status: 200 }, more = {}) => ({ request, expect, ...more });
export const at = (clock_ms, s) => (Array.isArray(s) ? [{ clock_ms, ...s[0] }, ...s.slice(1)] : { clock_ms, ...s });
export const err = (status, code, by = 'object', extra = {}) => ({ status, x_td_relay: by, json: { error: code, ...extra } });
export const gateErr = (status, code) => ({ ...err(status, code, 'gate'), object_called: false });
export const F = (author, ctr, extra = {}) => ({ $frame: { author, ctr, ...extra } });
export const N = (seq) => ({ status: 'new', seq });
export const DUP = (seq) => ({ status: 'dup', seq });
export const REF = (http_status, code) => ({ status: 'refused', http_status, code });
export const append = (cred, frames, results, status = 200, more = {}) =>
  step(req('append', cred, { body: { $batch: frames } }), { status, x_td_relay: 'object', results, ...more });

// The two steps of an admin operation: the nonce, then the signed request.
export function admin(op, signer, cred, args, expect = { status: 200 }, o = {}) {
  return [
    step(req('admin-nonce', o.nonceCred || cred, { body: { $dev: signer } }), { status: 200, json: { nonce: '$any', expires: { $time: 120000 } } }),
    step(req('admin', cred, { params: { op }, body: { $admin: { op, signer, args, ...(o.extra ? { extra: o.extra } : {}), ...(o.admin || {}) } } }), expect),
  ];
}
// Registers `key` with a DeviceAdded frame by `by` (its counter `ctr`), stored at `seq`.
export const fillLog = (author, cred, from_ctr, count) => ({ fill_log: { board: 'main', cred, author, from_ctr, count } });
export const addDevice = (key, seq, by = 'alice', ctr = 1) =>
  admin('register', by, T(by), { dev: key, sign_pk: key, frame_hash: { $hash: F(by, ctr) } },
    { status: 200, json: { seq, chain: { $chain: seq }, token: { $token: { key } } } }, { extra: F(by, ctr) });
export const login = (key, gen, cred = DOOR, more = {}) => [
  step(req('login-nonce', cred, { body: { $dev: key } }), { status: 200, json: { nonce: '$any', gen, expires: { $time: 120000 } } }),
  step(req('login', cred, { body: { $login: { dev: key, gen } } }), { status: 200, json: { token: { $token: { key, gen } }, gen, push: '$push', ...more } }),
];
export const range = (n, f) => Array.from({ length: n }, (_, i) => f(i));
