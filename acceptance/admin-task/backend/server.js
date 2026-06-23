import http from 'node:http';
import { db, ensure_schema } from './db.js';

ensure_schema();

const PORT = 4400;
const HOST = '127.0.0.1';
const CORS_ORIGIN = 'http://localhost:5500';

function setCors(res) {
  res.setHeader('Access-Control-Allow-Origin', CORS_ORIGIN);
  res.setHeader('Access-Control-Allow-Methods', 'GET,POST,PUT,DELETE,OPTIONS');
  res.setHeader('Access-Control-Allow-Headers', 'Content-Type');
}

function sendJson(res, status, payload) {
  setCors(res);
  res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8' });
  res.end(JSON.stringify(payload));
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    let data = '';
    req.on('data', chunk => { data += chunk; });
    req.on('end', () => {
      if (!data) return resolve({});
      try { resolve(JSON.parse(data)); }
      catch (e) { reject(new Error('Invalid JSON')); }
    });
    req.on('error', reject);
  });
}

const stmts = {
  all:    db.prepare('SELECT * FROM users ORDER BY id'),
  get:    db.prepare('SELECT * FROM users WHERE id = ?'),
  insert: db.prepare('INSERT INTO users (name, email, role) VALUES (?, ?, ?)'),
  update: db.prepare('UPDATE users SET name = ?, email = ?, role = ? WHERE id = ?'),
  delete: db.prepare('DELETE FROM users WHERE id = ?')
};

const server = http.createServer(async (req, res) => {
  if (req.method === 'OPTIONS') {
    setCors(res);
    res.writeHead(204);
    return res.end();
  }

  const url = new URL(req.url, `http://${req.host}`);
  const m = url.pathname.match(/^\/api\/users(?:\/(\d+))?$/);
  if (!m) return sendJson(res, 404, { error: 'Not Found' });

  const id = m[1] ? Number(m[1]) : null;

  try {
    if (req.method === 'GET' && !id) {
      return sendJson(res, 200, { users: stmts.all.all() });
    }
    if (req.method === 'GET' && id) {
      const row = stmts.get.get(id);
      if (!row) return sendJson(res, 404, { error: 'User not found' });
      return sendJson(res, 200, row);
    }
    if (req.method === 'POST' && !id) {
      const body = await readBody(req);
      if (!body.name || !body.email) {
        return sendJson(res, 400, { error: 'name and email are required' });
      }
      const role = body.role ?? 'user';
      try {
        const info = stmts.insert.run(body.name, body.email, role);
        const row = stmts.get.get(info.lastInsertRowid);
        return sendJson(res, 201, row);
      } catch (e) {
        if (String(e.message).includes('UNIQUE')) {
          return sendJson(res, 409, { error: 'email already exists' });
        }
        throw e;
      }
    }
    if (req.method === 'PUT' && id) {
      const existing = stmts.get.get(id);
      if (!existing) return sendJson(res, 404, { error: 'User not found' });
      const body = await readBody(req);
      const name  = body.name  ?? existing.name;
      const email = body.email ?? existing.email;
      const role  = body.role  ?? existing.role;
      try {
        stmts.update.run(name, email, role, id);
      } catch (e) {
        if (String(e.message).includes('UNIQUE')) {
          return sendJson(res, 409, { error: 'email already exists' });
        }
        throw e;
      }
      return sendJson(res, 200, stmts.get.get(id));
    }
    if (req.method === 'DELETE' && id) {
      const info = stmts.delete.run(id);
      if (info.changes === 0) return sendJson(res, 404, { error: 'User not found' });
      return sendJson(res, 200, { deleted: id });
    }
    return sendJson(res, 405, { error: 'Method Not Allowed' });
  } catch (e) {
    return sendJson(res, 400, { error: e.message });
  }
});

server.listen(PORT, HOST, () => {
  console.log(`Backend listening on http://${HOST}:${PORT}`);
});