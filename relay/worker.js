// The Worker and the board's Durable Object (relay.md §3.1, §6, §11, §17.1).
// The Worker is the gate and one call to the object; the object hands every
// request and socket message to the core. Neither decides anything itself.
import { DurableObject } from 'cloudflare:workers';
import { BoardCore, checkBody, gate, hex, makeConfig, refusal, unhex } from './relay-core.js';

// TOKEN_KEY and CREATE_SECRET are decoded once per isolate (RELAY-GEN-006): the Worker
// and every board object of the isolate take the same Config from here.
const configs = new WeakMap();
function configOf(env) {
  let c = configs.get(env);
  if (c === undefined) {
    c = makeConfig({ tokenKey: env.TOKEN_KEY, createSecret: env.CREATE_SECRET, origin: env.RELAY_ORIGIN, push: true });
    configs.set(env, c);
  }
  return c;
}

const respond = (reply) => new Response(reply.body, { status: reply.status, headers: reply.headers });

// Step 9 of §6.2: the whole body within its deadline, null when the deadline passed.
function readBody(request, deadlineMs) {
  const late = AbortSignal.timeout(deadlineMs);
  const never = new Promise((resolve) => late.addEventListener('abort', () => resolve(null)));
  const all = request.arrayBuffer().then((b) => new Uint8Array(b), () => null);
  return Promise.race([all, never]);
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const headers = {};
    for (const [name, value] of request.headers) (headers[name] || (headers[name] = [])).push(value);
    const g = await gate({ method: request.method, path: url.pathname, query: request.url.includes('?') ? url.search.slice(1) : null, headers }, configOf(env));
    if (g.reply) return respond(g.reply);
    const body = g.contentLength > 0 ? await readBody(request, g.deadlineMs) : new Uint8Array(0);
    const short = checkBody(g.contentLength, body);
    if (short) return respond(short);
    try {
      // Nothing the client sent goes on but the method, the route, the path parameters,
      // the query values and the body; the credential travels as the gate verified it (§6.4).
      const to = new Headers({ 'X-TD-Cred': g.cred.cred });
      if (g.cred.cred === 'token') {
        to.set('X-TD-Role', String(g.cred.role));
        to.set('X-TD-Dev', hex(g.cred.dev));
        to.set('X-TD-Gen', String(g.cred.gen));
      }
      if (g.route.name === 'socket') to.set('Upgrade', 'websocket');
      const q = new URLSearchParams({ ...g.params, ...g.query });
      const stub = env.BOARD.get(env.BOARD.idFromName(g.params.board));
      return await stub.fetch(`https://object/${g.route.name}?${q}`, { method: request.method, headers: to, body: body.length ? body : undefined });
    } catch {
      return respond(refusal('relay/object_unavailable'));
    }
  },
};

const row = (r) => {
  for (const k in r) if (r[k] instanceof ArrayBuffer) r[k] = new Uint8Array(r[k]);
  return r;
};

export class Board extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    const storage = {
      all: (q, ...p) => this.exec(q, ...p).toArray().map(row),
      // `changes` is the platform's count of rows written; the core does not read it.
      run: (q, ...p) => { const c = this.exec(q, ...p); c.toArray(); return { changes: c.rowsWritten }; },
      transaction: (fn) => ctx.storage.transactionSync(fn),
    };
    const sockets = {
      accept: (dev, attachment) => {
        const pair = new WebSocketPair();
        ctx.acceptWebSocket(pair[1], [hex(dev)]); // the hibernation API only (RELAY-SHAPE-008)
        pair[1].serializeAttachment(attachment);
        return { response: new Response(null, { status: 101, webSocket: pair[0] }), socket: pair[1] };
      },
      list: (dev) => (dev ? ctx.getWebSockets(hex(dev)) : ctx.getWebSockets()).filter((ws) => ws.readyState === 1),
      attachment: (ws) => ws.deserializeAttachment(),
      setAttachment: (ws, a) => ws.serializeAttachment(a),
      send: (ws, bytes) => ws.send(bytes),
      close: (ws, code, reason) => ws.close(code, reason),
    };
    this.core = new BoardCore({ storage, sockets, now: () => this.now(), config: configOf(env), board: null });
    ctx.blockConcurrencyWhile(async () => this.core.migrate());
    ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair('ping', 'pong'));
  }

  exec(query, ...params) {
    return this.ctx.storage.sql.exec(query, ...params);
  }
  now() {
    return Date.now();
  }

  // The internal request of §6.4; only the Worker can send it, through the binding.
  async fetch(request) {
    const url = new URL(request.url), h = request.headers;
    const params = Object.fromEntries(url.searchParams), query = {};
    for (const k of ['after', 'limit']) if (k in params) { query[k] = Number(params[k]); delete params[k]; }
    const cred = { cred: h.get('X-TD-Cred') };
    if (cred.cred === 'token') {
      cred.role = Number(h.get('X-TD-Role'));
      cred.dev = unhex(h.get('X-TD-Dev'), 16);
      cred.gen = Number(h.get('X-TD-Gen'));
    }
    const body = new Uint8Array(await request.arrayBuffer());
    const out = await this.core.handle({ route: url.pathname.slice(1), params, query, cred, body });
    return out.socket ? out.reply : respond(out);
  }

  async webSocketMessage(ws, message) {
    await this.core.onMessage(ws, typeof message === 'string' ? message : new Uint8Array(message));
  }
  async webSocketClose(ws, code) {
    this.core.onClose(ws, code);
    try { ws.close(1000, ''); } catch { /* already closed */ }
  }
  async webSocketError(ws) {
    this.core.onClose(ws, 1011);
    try { ws.close(1011, ''); } catch { /* already closed */ }
  }
}
