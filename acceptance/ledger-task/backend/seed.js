// Seed minimal demo data. Idempotent: only inserts if DB is empty.
import { db, stmts } from './db.js';

const existing = stmts.listUsers.all();
if (existing.length > 0) {
  console.log('seed: users already exist, skip.');
  process.exit(0);
}

const u = stmts.insertUser.run('张三', 'admin');
const uid = Number(u.lastInsertRowid);

const a1 = stmts.insertAccount.run(uid, '现金', 'cash', 500);
const a2 = stmts.insertAccount.run(uid, '银行卡', 'bank', 8000);

stmts.insertTx.run(a1.lastInsertRowid, -35.5, 'food', '早餐');
stmts.insertTx.run(a1.lastInsertRowid, -120, 'transport', '地铁+打车');
stmts.insertTx.run(a2.lastInsertRowid, -89, 'food', '午饭外卖');
stmts.insertTx.run(a2.lastInsertRowid, 3000, 'salary', '月初工资');

console.log('seed: ok.');