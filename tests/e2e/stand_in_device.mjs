// A stand-in for `taskdeck-devicectl`, for the tests of the harness itself
// (smoke.rs): it speaks the line protocol of device.md §19.2 for a few
// commands, so that the harness's driver is tested without the real device.
// It is not a model of the device: it answers what the harness's tests ask.
import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';

const argv = process.argv.slice(2);
const arg = (name) => { const i = argv.indexOf(name); return i < 0 ? null : argv[i + 1]; };
const data = arg('--data');
const counter = path.join(data, 'starts');
let starts = 0;
try { starts = Number(fs.readFileSync(counter, 'utf8')); } catch { /* the first start */ }
starts += 1;
fs.writeFileSync(counter, String(starts));

let wall = 1767614400000; // 2026-01-05T12:00:00Z, the manual clock's start (§19.1)
let any = 0;
const out = (o) => process.stdout.write(JSON.stringify(o) + '\n');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const rl = readline.createInterface({ input: process.stdin });
let chain = Promise.resolve();
rl.on('line', (line) => { chain = chain.then(() => handle(line)); });

async function handle(line) {
  let req;
  try { req = JSON.parse(line); } catch { return out({ id: 0, ok: false, error: 'device/ctl_request' }); }
  const id = req.id;
  switch (req.cmd) {
    case 'status': return out({ id, ok: true, starts, argv_has_name: arg('--name') !== null });
    case 'board': return out({ id, ok: true, canonical: '00', state_hash: 'ab'.repeat(32) });
    case 'confirm_sheet': return out({ id, ok: true });
    case 'sheet': return out({ id, ok: true, sheet: 'seed words seed words', address: 'host/ADDR', recovery_fp: 'ab12' });
    case 'pair_start': return out({ id, ok: true, code: '123456789', address: 'host/ADDR' });
    case 'clock':
      wall += req.advance_ms ?? 0;
      if (req.set_ms !== undefined) wall = req.set_ms;
      any += 1;
      out({ event: 'changed', changes: { any } });
      return out({ id, ok: true, wall_ms: wall });
    case 'wait':
      if (any <= req.since) { await sleep(20); any = req.since + 1; }
      return out({ id, ok: true, any, view: any });
    case 'stall':
      process.stderr.write('stand-in: stalling\n');
      return undefined;
    case 'shutdown':
      out({ id, ok: true });
      out({ event: 'exit', code: 0 });
      return process.exit(0);
    case 'kill': return process.abort();
    default: return out({ id, ok: false, error: 'device/ctl_request' });
  }
}
