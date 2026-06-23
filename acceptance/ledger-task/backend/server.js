import http from 'node:http';
import { db, ensure_schema, stmts } from './db.js';

ensure_schema();

const ORIGIN = 'http://localhost:5501';
const PORT = 4401;
const HOST = '127.0.0.1';

// ---------- helpers ----------
function setCORS(res) {
  res.setHeader('Access-Control-Allow-Origin', ORIGIN);
  res.setHeader('Access-Control-Allow-Methods', 'GET, POST, DELETE, OPTIONS');
  res.setHeader('Access-Control-Allow-Headers', 'Content-Type');
  res.setHeader('Access-Control-Max-Age', '86400');
}

function send(res, status, body) {
  setCORS(res);
  res.statusCode = status;
  res.setHeader('Content-Type', 'application/json; charset=utf-8');
  res.end(JSON.stringify(body));
}

function readJson(req) {
  return new Promise((resolve, reject) => {
    let raw = '';
    req.on('data', (chunk) => {
      raw += chunk;
      if (raw.length > 1e6) {
        req.destroy();
        reject(new Error('payload too large'));
      }
    });
    req.on('end', () => {
      if (!raw) return resolve({});
      try {
        resolve(JSON.parse(raw));
      } catch (e) {
        reject(new Error('invalid JSON'));
      }
    });
    req.on('error', reject);
  });
}

function notFound(res) {
  send(res, 404, { error: 'not found' });
}

// ---------- router ----------
const server = http.createServer(async (req, res) => {
  // preflight
  if (req.method === 'OPTIONS') {
    setCORS(res);
    res.statusCode = 204;
    return res.end();
  }

  const url = new URL(req.url, `http://${req.headers.host}`);
  const { pathname } = url;
  const method = req.method;

  try {
    // --- users ---
    if (pathname === '/api/users' && method === 'GET') {
      return send(res, 200, stmts.listUsers.all());
    }

    if (pathname === '/api/users' && method === 'POST') {
      const body = await readJson(req);
      const name = (body.name || '').trim();
      const role = (body.role || 'member').trim();
      if (!name) return send(res, 400, { error: 'name required' });
      const r = stmts.insertUser.run(name, role);
      const user = stmts.getUser.get(r.lastInsertRowid);
      return send(res, 201, user);
    }

    // --- accounts under a user ---
    const userAccMatch = pathname.match(/^\/api\/users\/(\d+)\/accounts$/);
    if (userAccMatch && method === 'GET') {
      const uid = Number(userAccMatch[1]);
      const u = stmts.getUser.get(uid);
      if (!u) return send(res, 404, { error: 'user not found' });
      return send(res, 200, stmts.listAccountsByUser.all(uid));
    }

    // --- create account ---
    if (pathname === '/api/accounts' && method === 'POST') {
      const body = await readJson(req);
      const user_id = Number(body.user_id);
      const name = (body.name || '').trim();
      const kind = (body.kind || 'cash').trim();
      const balance = Number(body.balance ?? 0);
      if (!user_id || !name) {
        return send(res, 400, { error: 'user_id and name required' });
      }
      if (!stmts.getUser.get(user_id)) {
        return send(res, 404, { error: 'user not found' });
      }
      const r = stmts.insertAccount.run(user_id, name, kind, balance);
      const acc = stmts.getAccount.get(r.lastInsertRowid);
      return send(res, 201, acc);
    }

    // --- transactions for an account ---
    const accTxMatch = pathname.match(/^\/api\/accounts\/(\d+)\/transactions$/);
    if (accTxMatch && method === 'GET') {
      const aid = Number(accTxMatch[1]);
      if (!stmts.getAccount.get(aid)) {
        return send(res, 404, { error: 'account not found' });
      }
      return send(res, 200, stmts.listTxByAccount.all(aid));
    }

    // --- create transaction ---
    if (pathname === '/api/transactions' && method === 'POST') {
      const body = await readJson(req);
      const account_id = Number(body.account_id);
      const amount = Number(body.amount);
      const category = (body.category || '').trim();
      const note = body.note ?? '';
      const occurred_at = body.occurred_at || null;
      if (!account_id || !category || !Number.isFinite(amount)) {
        return send(res, 400, {
          error: 'account_id, amount, category required',
        });
      }
      if (!stmts.getAccount.get(account_id)) {
        return send(res, 404, { error: 'account not found' });
      }
      const r = stmts.insertTx.run(
        account_id,
        amount,
        category,
        note,
        occurred_at
      );
      const tx = stmts.getTx.get(r.lastInsertRowid);
      return send(res, 201, tx);
    }

    // --- delete transaction ---
    const delMatch = pathname.match(/^\/api\/transactions\/(\d+)$/);
    if (delMatch && method === 'DELETE') {
      const tid = Number(delMatch[1]);
      const exists = stmts.getTx.get(tid);
      if (!exists) return send(res, 404, { error: 'not found' });
      stmts.deleteTx.run(tid);
      return send(res, 200, { ok: true, id: tid });
    }

    // --- report summary ---
    if (pathname === '/api/report/summary' && method === 'GET') {
      const uid = Number(url.searchParams.get('user_id'));
      if (!uid) return send(res, 400, { error: 'user_id required' });
      if (!stmts.getUser.get(uid)) {
        return send(res, 404, { error: 'user not found' });
      }
      const total_in = stmts.sumIn.get(uid).s;
      const total_out = stmts.sumOut.get(uid).s;
      const by_category = {};
      for (const row of stmts.byCategory.all(uid)) {
        by_category[row.category] = row.s;
      }
      return send(res, 200, {
        user_id: uid,
        total_in,
        total_out,
        balance: Number(total_in) + Number(total_out),
        by_category,
      });
    }

    // health
    if (pathname === '/health' && method === 'GET') {
      return send(res, 200, { ok: true });
    }

    return notFound(res);
  } catch (err) {
    return send(res, 400, { error: err.message || 'bad request' });
  }
});

server.listen(PORT, HOST, () => {
  console.log(`ledger-backend listening on http://${HOST}:${PORT}`);
});