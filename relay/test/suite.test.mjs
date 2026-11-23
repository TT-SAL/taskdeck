// The conformance suite (relay.md §18): every case of every family under
// vectors/, on the target TD_TARGET names (core, standby or worker).
// spec: RELAY-TEST-001
import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';
import * as core from '../relay-core.js';
import { TARGET, hex, runCase, unhex } from './lib/harness.mjs';
import { families } from './vectors/build.mjs';

const dir = new URL('./vectors/', import.meta.url);

test('the published vector files are what vectors/build.mjs writes', () => {
  for (const [file, family] of Object.entries(families)) {
    assert.equal(fs.readFileSync(new URL(file, dir), 'utf8'), JSON.stringify(family, null, 1) + '\n', `${file} is stale: run node relay/test/vectors/build.mjs`);
  }
  const extra = fs.readdirSync(dir).filter((f) => f.endsWith('.json') && !(f in families));
  assert.deepEqual(extra, [], 'vector files without a family');
});

// A case without steps calls one pure function of the core (the token and address families).
function callCase(c) {
  const a = c.args, key = a.token_key && unhex(a.token_key);
  const run = {
    makeToken: () => core.makeToken(key, a.role, unhex(a.board), unhex(a.dev), a.gen),
    checkToken: () => { const t = core.checkToken(key, a.text, unhex(a.board)); return { role: t.role, dev: hex(t.dev), gen: t.gen }; },
    door: () => hex(core.door(key, unhex(a.board))),
    formatAddress: () => core.formatAddress(a.host, unhex(a.board), unhex(a.door)),
    displayAddress: () => core.displayAddress(a.host, unhex(a.board), unhex(a.door)),
    parseAddress: () => { const p = core.parseAddress(a.text); return { host: p.host, origin: p.origin, board: hex(p.board), door: hex(p.door) }; },
    bodyDeadlineMs: () => core.bodyDeadlineMs(a.length),
  }[c.call];
  if (c.error) assert.throws(run, (e) => e instanceof core.RelayError && e.code === c.error, c.error);
  else assert.deepEqual(run(), c.expect);
}

for (const [file, family] of Object.entries(JSON.parse(JSON.stringify(families)))) {
  for (const c of family.cases) {
    if (!c.targets.includes(TARGET)) continue;
    test(`${file.replace(/\.json$/, '')}: ${c.name}`, () => (c.call ? callCase(c) : runCase(c)));
  }
}
