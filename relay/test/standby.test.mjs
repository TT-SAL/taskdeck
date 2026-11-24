// What only the standby does (relay.md §15.2, §17.2): its configuration, its
// timeouts, one database file and one queue per board. Runs on the standby target.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import * as core from '../relay-core.js';
import { startStandby } from '../standby.js';
import { STD, TARGET, World, decodeResults, makeTarget, send } from './lib/harness.mjs';
import { KEYS } from './vectors/dsl.mjs';

// These cases belong to the standby target, as a vector case names its targets.
const only = TARGET === 'standby' ? test : () => {};
const tmp = () => fs.mkdtempSync(path.join(os.tmpdir(), 'td-standby-test-'));
const good = (dir) => ({ tokenKey: STD.token_key, createSecret: STD.create_secret, origin: STD.origin, dataDir: dir, listen: '127.0.0.1:0' });

// spec: RELAY-SHAPE-012
// spec: RELAY-GEN-005
only('the standby refuses to start on an invalid value and names the variable, never the value', async () => {
  const dir = tmp();
  try {
    const secret = 'zz'.repeat(32);
    const bad = [
      [{ tokenKey: undefined }, 'TD_TOKEN_KEY'], [{ tokenKey: secret }, 'TD_TOKEN_KEY'], [{ tokenKey: STD.token_key.toUpperCase() }, 'TD_TOKEN_KEY'],
      [{ createSecret: secret }, 'TD_CREATE_SECRET'], [{ createSecret: '' }, 'TD_CREATE_SECRET'],
      [{ origin: undefined }, 'TD_RELAY_ORIGIN'], [{ origin: 'http://relay.test' }, 'TD_RELAY_ORIGIN'], [{ origin: 'https://relay.test/' }, 'TD_RELAY_ORIGIN'], [{ origin: 'https://Relay.test' }, 'TD_RELAY_ORIGIN'],
      [{ dataDir: path.join(dir, 'missing') }, 'TD_DATA_DIR'], [{ dataDir: undefined }, 'TD_DATA_DIR'],
      [{ listen: '127.0.0.1' }, 'TD_LISTEN'], [{ listen: undefined }, 'TD_LISTEN'], [{ listen: '127.0.0.1:99999' }, 'TD_LISTEN'],
      [{ tlsCert: path.join(dir, 'cert.pem') }, 'TD_TLS_CERT'], [{ tlsKey: path.join(dir, 'key.pem') }, 'TD_TLS_KEY'],
    ];
    for (const [change, name] of bad) {
      await assert.rejects(startStandby({ ...good(dir), ...change }), (e) => {
        assert.match(e.message, new RegExp(name), JSON.stringify(change));
        assert.ok(!e.message.includes(secret) && !e.message.includes(STD.token_key.toUpperCase()), 'the value is not in the message');
        return true;
      });
    }
    // Without a create secret the standby starts, and the create family is disabled.
    const s = await startStandby({ ...good(dir), createSecret: undefined });
    await s.close();
    // `node standby.js`: status 2 and one line that names the variable.
    const file = fileURLToPath(new URL('../standby.js', import.meta.url));
    const env = { PATH: process.env.PATH, TD_TOKEN_KEY: secret, TD_RELAY_ORIGIN: STD.origin, TD_DATA_DIR: dir, TD_LISTEN: '127.0.0.1:0' };
    const run = spawnSync(process.execPath, ['--no-warnings', file], { env, encoding: 'utf8', timeout: 20000 });
    assert.equal(run.status, 2);
    assert.equal(run.stdout, '');
    assert.match(run.stderr, /^standby: TD_TOKEN_KEY is missing or invalid\n$/);
    assert.ok(!run.stderr.includes(secret));
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

// spec: RELAY-STBY-003
// spec: RELAY-STBY-002
// spec: RELAY-OBJ-008
// spec: RELAY-SHAPE-011
only('the standby\'s timeouts, its file per board, WAL and full synchronous', async () => {
  const target = makeTarget('standby');
  const world = new World({ keys: { rec: KEYS.rec, alice: KEYS.alice }, boards: { one: { salt: '01'.repeat(16), recovery: 'rec', first: 'alice' }, two: { salt: '02'.repeat(16), recovery: 'rec', first: 'alice' } } }, target);
  try {
    await world.init();
    assert.equal(target.server.server.headersTimeout, 10000);
    assert.equal(target.server.server.requestTimeout, core.bodyDeadlineMs(4194373) + 10000);
    assert.match(target.server.url, /^http:\/\/127\.0\.0\.1:[0-9]+$/);
    const files = fs.readdirSync(target.dir).filter((f) => f.endsWith('.sqlite')).sort();
    assert.deepEqual(files, [`${world.board('one').hex}.sqlite`, `${world.board('two').hex}.sqlite`].sort());
    for (const f of files) {
      const db = new DatabaseSync(path.join(target.dir, f), { readOnly: true });
      assert.equal(db.prepare('PRAGMA journal_mode').get().journal_mode, 'wal');
      db.close();
    }
    const source = fs.readFileSync(new URL('../standby.js', import.meta.url), 'utf8');
    assert.match(source, /PRAGMA journal_mode = WAL/);
    assert.match(source, /PRAGMA synchronous = FULL/);
    // A request for a board that was never handed to the core creates no file.
    const r = await send(target.server.url, { method: 'GET', path: `/v1/b/${'ab'.repeat(16)}/hello`, headers: {} });
    assert.equal(r.status, 401);
    assert.equal(fs.readdirSync(target.dir).filter((f) => f.endsWith('.sqlite')).length, 2);
  } finally {
    await world.stop();
  }
});

// spec: RELAY-STBY-002
only('the standby handles the requests of one board one at a time, in arrival order', async () => {
  const target = makeTarget('standby');
  const world = new World({ keys: { rec: KEYS.rec, alice: KEYS.alice }, boards: { main: { salt: '03'.repeat(16), recovery: 'rec', first: 'alice' } } }, target);
  try {
    await world.init();
    const cred = await world.credHeader({ token: 'alice' }, 'main'), b = world.board('main');
    const batch = async (frames) => world.bytes({ $batch: frames }, 'main');
    // A: sixteen large frames, slow to check. B: one small frame that needs A's counters. B is
    // sent once A's body is on the wire; without the queue B's transaction would run first.
    const big = await batch(Array.from({ length: 16 }, (_, i) => ({ $frame: { author: 'alice', ctr: 1 + i, body_len: 262032 } })));
    const small = await batch([{ $frame: { author: 'alice', ctr: 17 } }]);
    const post = (body) => send(target.server.url, { method: 'POST', path: `/v1/b/${b.hex}/append`, headers: { ...cred, 'Content-Length': String(body.length) }, body });
    const a = post(big);
    await new Promise((r) => setTimeout(r, 5));
    const second = post(small);
    const [ra, rb] = await Promise.all([a, second]);
    assert.equal(ra.status, 200);
    assert.deepEqual(decodeResults(ra.body).map((x) => x.status), Array(16).fill('new'));
    assert.equal(rb.status, 200, 'the second request waited for the first');
    assert.deepEqual(decodeResults(rb.body).map((x) => [x.status, x.seq]), [['new', 18]]);
  } finally {
    await world.stop();
  }
});
