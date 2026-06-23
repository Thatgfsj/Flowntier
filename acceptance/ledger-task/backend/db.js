import { DatabaseSync } from 'node:sqlite';

const DB_PATH = new URL('./ledger.db', import.meta.url);

export const db = new DatabaseSync(DB_PATH);
db.exec('PRAGMA foreign_keys = ON;');
db.exec('PRAGMA journal_mode = WAL;');

export function ensure_schema() {
  db.exec(`
    CREATE TABLE IF NOT EXISTS users (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      name TEXT NOT NULL,
      role TEXT DEFAULT 'member',
      created_at TEXT DEFAULT CURRENT_TIMESTAMP
    );
    CREATE TABLE IF NOT EXISTS accounts (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      user_id INTEGER NOT NULL,
      name TEXT NOT NULL,
      kind TEXT DEFAULT 'cash',
      balance REAL DEFAULT 0,
      created_at TEXT DEFAULT CURRENT_TIMESTAMP,
      FOREIGN KEY(user_id) REFERENCES users(id)
    );
    CREATE TABLE IF NOT EXISTS transactions (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      account_id INTEGER NOT NULL,
      amount REAL NOT NULL,
      category TEXT NOT NULL,
      note TEXT DEFAULT '',
      occurred_at TEXT DEFAULT CURRENT_TIMESTAMP,
      created_at TEXT DEFAULT CURRENT_TIMESTAMP,
      FOREIGN KEY(account_id) REFERENCES accounts(id)
    );
  `);
}

// Run schema immediately at module load. Without this, the
// prepared statements below would fail with `no such table`
// when the module is imported before any code path called
// ensure_schema().
ensure_schema();

// Prepared statements (must come AFTER ensure_schema())
export const stmts = {
  listUsers: db.prepare('SELECT * FROM users ORDER BY id'),
  insertUser: db.prepare('INSERT INTO users (name, role) VALUES (?, ?)'),
  getUser: db.prepare('SELECT * FROM users WHERE id = ?'),
  listAccountsByUser: db.prepare(
    'SELECT * FROM accounts WHERE user_id = ? ORDER BY id'
  ),
  getAccount: db.prepare('SELECT * FROM accounts WHERE id = ?'),
  insertAccount: db.prepare(
    'INSERT INTO accounts (user_id, name, kind, balance) VALUES (?, ?, ?, ?)'
  ),
  listTxByAccount: db.prepare(
    'SELECT * FROM transactions WHERE account_id = ? ORDER BY occurred_at DESC, id DESC LIMIT 200'
  ),
  insertTx: db.prepare(
    'INSERT INTO transactions (account_id, amount, category, note, occurred_at) VALUES (?, ?, ?, ?, COALESCE(?, CURRENT_TIMESTAMP))'
  ),
  deleteTx: db.prepare('DELETE FROM transactions WHERE id = ?'),
  getTx: db.prepare('SELECT * FROM transactions WHERE id = ?'),
  // Report
  sumIn: db.prepare(
    `SELECT COALESCE(SUM(t.amount), 0) AS s
     FROM transactions t JOIN accounts a ON t.account_id = a.id
     WHERE a.user_id = ? AND t.amount > 0`
  ),
  sumOut: db.prepare(
    `SELECT COALESCE(SUM(t.amount), 0) AS s
     FROM transactions t JOIN accounts a ON t.account_id = a.id
     WHERE a.user_id = ? AND t.amount < 0`
  ),
  byCategory: db.prepare(
    `SELECT t.category AS category, SUM(t.amount) AS s
     FROM transactions t JOIN accounts a ON t.account_id = a.id
     WHERE a.user_id = ?
     GROUP BY t.category`
  ),
};