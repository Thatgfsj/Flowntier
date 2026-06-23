import { DatabaseSync } from 'node:sqlite';

export const db = new DatabaseSync('users.db');
db.exec('PRAGMA journal_mode = WAL;');

export function ensure_schema() {
  db.exec(`
    CREATE TABLE IF NOT EXISTS users (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      name TEXT NOT NULL,
      email TEXT NOT NULL UNIQUE,
      role TEXT DEFAULT 'user',
      created_at TEXT DEFAULT CURRENT_TIMESTAMP
    );
  `);
}

ensure_schema();