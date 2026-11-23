// The standby (relay.md §15.2, §17.2): the same gate and core as the Worker, on
// Node 22 with node:sqlite, over plain HTTPS polling. No sockets, no push.
//   node standby.js      reads TD_TOKEN_KEY, TD_CREATE_SECRET, TD_RELAY_ORIGIN,
//                        TD_DATA_DIR, TD_LISTEN, TD_TLS_CERT, TD_TLS_KEY
import fs from 'node:fs';
import http from 'node:http';
import https from 'node:https';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { BoardCore, ROUTES, bodyDeadlineMs, checkBody, configError, gate, makeConfig, noSockets, refusal, sqliteStorage } from './relay-core.js';

const NAMES = { tokenKey: 'TD_TOKEN_KEY', createSecret: 'TD_CREATE_SECRET', origin: 'TD_RELAY_ORIGIN' };
const GONE = Symbol('the client went away');

// Step 9 of §6.2: the whole body within its deadline, null when the deadline passed.
function readBody(req, length, deadlineMs) {
  if (length === 0) return Promise.resolve(new Uint8Array(0));
  return new Promise((resolve) => {
    const chunks = [];
    const late = AbortSignal.timeout(deadlineMs);
    const done = (v) => {
      late.removeEventListener('abort', onLate);
      resolve(v);
    };
    const onLate = () => done(null);
    late.addEventListener('abort', onLate);
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => done(new Uint8Array(Buffer.concat(chunks))));
    req.on('error', () => done(GONE));
    req.on('close', () => done(GONE));
  });
}

export async function startStandby({ tokenKey, createSecret, origin, dataDir, listen, tlsCert, tlsKey, now }) {
  const wrong = configError({ tokenKey, createSecret, origin });
  if (wrong) throw new Error(`${NAMES[wrong]} is missing or invalid`);
  if (typeof dataDir !== 'string' || !fs.existsSync(dataDir) || !fs.statSync(dataDir).isDirectory()) throw new Error('TD_DATA_DIR is not an existing directory');
  const where = /^(.+):([0-9]{1,5})$/.exec(typeof listen === 'string' ? listen : '');
  if (!where || Number(where[2]) > 65535) throw new Error('TD_LISTEN is not host:port');
  if (!tlsCert !== !tlsKey) throw new Error('TD_TLS_CERT and TD_TLS_KEY go together');
  const config = makeConfig({ tokenKey, createSecret, origin, push: false });
  const clock = now || Date.now;

  // One core and one database file per board; its requests run one at a time, in arrival order.
  const boards = new Map();
  const boardOf = (id) => {
    let b = boards.get(id);
    if (!b) {
      const db = new DatabaseSync(path.join(dataDir, `${id}.sqlite`));
      db.exec('PRAGMA journal_mode = WAL');
      db.exec('PRAGMA synchronous = FULL');
      const core = new BoardCore({ storage: sqliteStorage(db), sockets: noSockets, now: clock, config, board: null });
      core.migrate();
      b = { db, core, tail: Promise.resolve() };
      boards.set(id, b);
    }
    return b;
  };
  const call = (g, body) => {
    const b = boardOf(g.params.board);
    const run = b.tail.then(() => b.core.handle({ route: g.route.name, params: g.params, query: g.query, cred: g.cred, body }));
    b.tail = run.catch(() => {});
    return run;
  };

  const answer = (req, res, reply, unread) => {
    if (unread) { // the body was not read: the connection ends with this answer
      res.setHeader('Connection', 'close');
      res.on('finish', () => req.destroy());
    }
    res.writeHead(reply.status, reply.headers);
    res.end(reply.body);
  };
  const handle = async (req, res) => {
    const headers = {};
    for (let i = 0; i < req.rawHeaders.length; i += 2) {
      const name = req.rawHeaders[i].toLowerCase();
      (headers[name] || (headers[name] = [])).push(req.rawHeaders[i + 1]);
    }
    const q = req.url.indexOf('?');
    const g = await gate({ method: req.method, path: q < 0 ? req.url : req.url.slice(0, q), query: q < 0 ? null : req.url.slice(q + 1), headers }, config);
    if (g.reply) return answer(req, res, g.reply, true);
    const body = await readBody(req, g.contentLength, g.deadlineMs);
    if (body === GONE) return undefined;
    const short = checkBody(g.contentLength, body);
    if (short) return answer(req, res, short, true);
    let reply;
    try { reply = await call(g, body); } catch { reply = refusal('relay/object_unavailable'); }
    return answer(req, res, reply, false);
  };
  const onRequest = (req, res) => {
    handle(req, res).catch(() => {
      if (!res.headersSent) answer(req, res, refusal('relay/object_unavailable'), true);
    });
  };

  const tls = tlsCert ? { cert: fs.readFileSync(tlsCert), key: fs.readFileSync(tlsKey) } : null;
  const server = tls ? https.createServer(tls, onRequest) : http.createServer(onRequest);
  server.headersTimeout = 10000;
  server.requestTimeout = bodyDeadlineMs(Math.max(...ROUTES.map((r) => r.cap))) + 10000;
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(Number(where[2]), where[1].replace(/^\[|\]$/g, ''), resolve);
  });
  const port = server.address().port;
  return {
    url: `${tls ? 'https' : 'http'}://${where[1]}:${port}`,
    server, // for the suite, which reads the two timeouts of §15.2 from it
    close: () => new Promise((resolve) => {
      server.close(() => {
        for (const b of boards.values()) b.db.close();
        boards.clear();
        resolve();
      });
      server.closeAllConnections();
    }),
  };
}

const main = process.argv[1] ? path.resolve(process.argv[1]) : '';
if (main && main === path.resolve(decodeURIComponent(new URL(import.meta.url).pathname))) {
  const e = process.env;
  startStandby({ tokenKey: e.TD_TOKEN_KEY, createSecret: e.TD_CREATE_SECRET, origin: e.TD_RELAY_ORIGIN, dataDir: e.TD_DATA_DIR, listen: e.TD_LISTEN, tlsCert: e.TD_TLS_CERT, tlsKey: e.TD_TLS_KEY }).then(
    (s) => process.on('SIGTERM', () => s.close().then(() => process.exit(0))),
    (err) => { process.stderr.write(`standby: ${err.message}\n`); process.exit(2); },
  );
}
