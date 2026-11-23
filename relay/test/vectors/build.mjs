// Writes the vector families of relay.md §21 as JSON, one file each:
//   node relay/test/vectors/build.mjs
// The suite reads the families from here and fails when a file is stale.
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import * as a from './cases-a.mjs';

const family = (spec, cases) => {
  const names = new Set();
  for (const c of cases) { if (names.has(c.name)) throw new Error(`two cases named "${c.name}"`); names.add(c.name); }
  return { spec, cases };
};
export const families = {
  'relay-gate.json': family('relay.md §5, §6', a.gate),
  'relay-token.json': family('relay.md §5.1, §5.2, §8.2', a.token),
  'relay-address.json': family('relay.md §5.4', a.address),
  'relay-create.json': family('relay.md §10.2', a.create),
  'relay-login.json': family('relay.md §10.3', a.loginF),
  'relay-append.json': family('relay.md §9', a.appendF),
  'relay-read.json': family('relay.md §10.4, §10.5', a.read),
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  for (const [file, f] of Object.entries(families)) fs.writeFileSync(new URL(file, import.meta.url), JSON.stringify(f, null, 1) + '\n');
}
