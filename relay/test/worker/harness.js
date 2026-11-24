// The suite's harness Worker (relay.md §18 item 2): the shipped Worker and Board,
// plus test-only routes under /__test/ that count object calls and rows, move the
// object's clock and read its tables. The shipped worker.js has no such route.
import worker, { Board as ShippedBoard } from '../../worker.js';
import { hex, unhex, verifyStrict } from '../../relay-core.js';

const json = (o) => new Response(JSON.stringify(o), { headers: { 'Content-Type': 'application/json' } });
const fresh = () => ({ boot: crypto.randomUUID(), calls: 0, messages: 0, rowsRead: 0, rowsWritten: 0 });

export class Board extends ShippedBoard {
  count() {
    return this.stats || (this.stats = fresh());
  }
  exec(query, ...params) {
    const cursor = super.exec(query, ...params), stats = this.count();
    return {
      get rowsWritten() { return cursor.rowsWritten; },
      toArray: () => {
        const rows = cursor.toArray();
        stats.rowsRead += cursor.rowsRead;
        stats.rowsWritten += cursor.rowsWritten;
        return rows;
      },
    };
  }
  now() {
    return Date.now() + (this.offset || 0);
  }
  async fetch(request) {
    const path = new URL(request.url).pathname;
    if (path === '/__test/stats') return json({ ...this.count(), offset: this.offset || 0 });
    if (path === '/__test/clock') {
      this.offset = Number(await request.text());
      return json({ offset: this.offset });
    }
    if (path.startsWith('/__test/dump/')) {
      const table = path.slice(13);
      if (!/^[a-z_]+$/.test(table)) return new Response('no such table', { status: 404 });
      const rows = this.ctx.storage.sql.exec(`SELECT * FROM ${table} ORDER BY 1`).toArray();
      return json(rows.map((r) => Object.fromEntries(Object.entries(r).map(([k, v]) => [k, v instanceof ArrayBuffer ? hex(new Uint8Array(v)) : v]))));
    }
    this.count().calls++;
    return super.fetch(request);
  }
  async webSocketMessage(ws, message) {
    this.count().messages++;
    return super.webSocketMessage(ws, message);
  }
}

const VARIANTS = {
  'no-create': (env) => ({ ...env, CREATE_SECRET: undefined }),
  'bad-key': (env) => ({ ...env, TOKEN_KEY: env.TOKEN_KEY.slice(2) }),
  'bad-origin': (env) => ({ ...env, RELAY_ORIGIN: 'https://relay.test:443' }),
};
const variants = new Map();

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    if (url.pathname.startsWith('/__test/')) {
      const [, , what, board, ...rest] = url.pathname.split('/');
      if (what === 'ping') return json({ ok: true });
      if (what === 'verify') {
        const { pk, msg, sig } = await request.json();
        return json({ ok: await verifyStrict(unhex(pk, pk.length / 2), unhex(msg, msg.length / 2), unhex(sig, sig.length / 2)) });
      }
      if (!/^[0-9a-f]{32}$/.test(board || '')) return new Response('no such test route', { status: 404 });
      const stub = env.BOARD.get(env.BOARD.idFromName(board));
      return stub.fetch(`https://object/__test/${[what, ...rest].join('/')}`, { method: request.method, body: request.method === 'POST' ? await request.text() : undefined });
    }
    // A case that needs another configuration names a variant of this Worker's one.
    const name = request.headers.get('X-Test-Env');
    if (!name) return worker.fetch(request, env, ctx);
    if (!variants.has(name)) variants.set(name, VARIANTS[name](env));
    return worker.fetch(request, variants.get(name), ctx);
  },
};
