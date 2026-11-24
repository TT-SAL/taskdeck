// Writes the vector families of relay.md §21 as JSON, one file each:
//   node relay/test/vectors/build.mjs
// The suite reads the families from here and fails when a file is stale.
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import * as a from './cases-a.mjs';
import * as b from './cases-b.mjs';
import * as c from './cases-c.mjs';

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
  'relay-blobs.json': family('relay.md §10.7, §13', b.blobs),
  'relay-pairing.json': family('relay.md §10.8', b.pairing),
  'relay-admin.json': family('relay.md §12.1 to §12.6, §12.11, §12.12', b.adminF),
  'relay-seed.json': family('relay.md §12.7', c.seedF),
  'relay-compact.json': family('relay.md §12.8, §13.4', c.compactF),
  'relay-restore.json': family('relay.md §12.9', c.restoreF),
  'relay-purge.json': family('relay.md §12.10', c.purgeF),
  'relay-budget.json': family('relay.md §14', c.budgetF),
  'relay-socket.json': family('relay.md §11', c.socketF),
  'relay-standby.json': family('relay.md §15.2', c.standbyF),
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  for (const [file, f] of Object.entries(families)) fs.writeFileSync(new URL(file, import.meta.url), JSON.stringify(f, null, 1) + '\n');
}
