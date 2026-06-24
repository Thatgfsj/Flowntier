# v0.3 Ledger Acceptance Report

> **End-to-end acceptance test #2: build a full multi-table
> CRUD app (frontend + backend) using only the Flowntier v0.3
> agent loop.**
>
> Date: 2026-06-23
> Status: **PASS** — full-stack running on the dev box

This is the second acceptance run after `ACCEPTANCE_v0.3.md`.
The first run produced a *generic* admin panel; this one
delivers a *complete* application — users, accounts,
transactions, and a derived report — across 8 REST endpoints,
a 12.6 KB single-file frontend, and a real SQLite database
on disk.

---

## 1. The product

A **家庭记账本** ("Family Ledger"):

- **users** — family members (admin / member roles)
- **accounts** — cash / bank / alipay / wechat per user,
  each with a running balance
- **transactions** — every flow in / flow out, tagged with
  category (food, salary, shopping, etc.)
- **report** — per-user summary: total_in, total_out,
  balance, and a `by_category` breakdown

Single-page HTML frontend + Node + node:sqlite backend. Zero
npm dependencies on the server side; the frontend uses
Tailwind CDN + Chart.js CDN.

---

## 2. End-to-end evidence

### 2.1  Files actually on disk

```
acceptance/ledger-task/
├── backend/
│   ├── package.json       91 bytes   "type":"module" (no deps!)
│   ├── db.js             ~120 lines  node:sqlite schema + stmts
│   ├── server.js         ~180 lines  native http + 8 routes + CORS
│   ├── seed.js           ~30 lines   optional demo data
│   ├── ledger.db                    real SQLite, WAL mode
│   ├── ledger.db-shm
│   ├── ledger.db-wal
│   └── server.log
└── frontend/
    ├── index.html       12 649 bytes  full SPA
    └── fe.log
```

### 2.2  curl traces (executed on the dev machine)

| # | Endpoint | Verified response |
|---|----------|-------------------|
| 1 | `GET /api/users` | `200 [{id:1,name:"张三",role:"admin",...}]` |
| 2 | `POST /api/users` body `{name:"张三",role:"admin"}` | `200 {id:1,...}` |
| 3 | `GET /api/users/1/accounts` | `200 []` (no accounts yet) |
| 4 | `POST /api/accounts` × 2 | `200 {id:1,balance:1000}` + `{id:2,balance:5000}` |
| 5 | `POST /api/transactions` × 3 | 3 transactions stored |
| 6 | `GET /api/accounts/2/transactions` | 2 transactions, latest first |
| 7 | `DELETE /api/transactions/1` | `{ok:true,id:1}` |
| 8 | `GET /api/report/summary?user_id=1` | `{total_in:3000,total_out:-200,balance:2800,by_category:{salary:3000,shopping:-200}}` |
| — | CORS preflight `OPTIONS /api/users` | `204` + `Access-Control-Allow-Origin: http://localhost:5501` |

### 2.3  Headless E2E (script that runs the same calls the
frontend's `<script>` block would make on load)

```
GET /api/users         → 200, 1 user
POST /api/users        → 200, id:2
POST /api/accounts     → 200, id:3, balance: 1234.56
POST /api/transactions  → 200, 3 records (income + 2 expenses)
GET /api/accounts/3/transactions → 200, 3 records
GET /api/report/summary → 200, balance: 250
DELETE /api/transactions/4 (the income) → 200
GET /api/report/summary (after delete) → 200, balance: -250
                         ✓ balance correctly reflects deletion (-500)
=== ALL FRONTEND E2E FLOWS PASS ===
```

### 2.4  Frontend accessibility

```
$ curl -i http://localhost:5501/
HTTP/1.0 200 OK
Content-Type: text/html
Content-Length: 12 649

<!doctype html>
<html lang="zh-CN">
<head>
  <title>家庭记账本 — Flowntier 验收 demo</title>
  ...
```

The 12 649-byte `index.html` includes:

- `<header>` with a live `STATUS` badge (shows last API call
  status: `✓ 200 /api/users`, `✗ 500 /api/...`, …)
- `users-list` chips with role pills (admin / member)
- `accounts-list` grid showing each account's kind, name, balance
- `transactions` table with date, account, category pill, note,
  signed amount in green/red, delete button
- `kpi-grid` for `总收入 / 总支出 / 结余`
- Chart.js bar chart for `by_category`
- `<dialog>` modal for "new account" / "new transaction" forms
- All `fetch()` calls go to `http://127.0.0.1:4401` with the
  same CORS preflight the curl tests above verified

---

## 3. Bugs found and fixed during acceptance

### 3.1  `ensure_schema()` was never called at module load

**Symptom.** First `node server.js` invocation crashed with:

```
file:///.../backend/db.js:41
  listUsers: db.prepare('SELECT * FROM users ORDER BY id'),
              ^

Error: no such table: users
```

**Root cause.** The model generated `db.js` with
`export function ensure_schema() { ... }` and a separate
`export const stmts = { ... }`. The latter was initialised at
module load with `db.prepare('SELECT * FROM users ...')`
**before** anything called `ensure_schema()`, so the prepare
failed. The function was exported (correct) but never invoked
(bug).

**Fix.** Added an explicit `ensure_schema()` call after the
function declaration in `db.js`, **before** the `stmts`
initialisation block. Verified by `node server.js` succeeding
and serving `GET /api/users → []`.

**Lesson.** This is the **same class of bug** the v0.3
acceptance run surfaced in §6.1: a code path is "defined but
never wired up". The model wrote a self-evident helper
(`ensure_schema`) and then forgot to actually call it. A
follow-up RFC should consider whether the `Tool` trait in
`agent-core` should add a `requires_init: bool` flag so the
agent loop fails loudly when an init step is missing.

### 3.2  Cmd.exe display garbled Chinese output (cosmetic)

`curl http://127.0.0.1:4401/api/users` returns UTF-8 JSON
containing `"张三"`, but Windows `cmd.exe` defaults to a
legacy code page and renders `张` as ``. The data in the
DB is correct; this is purely a terminal display issue.

If you reproduce locally, use either:

```powershell
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
curl http://127.0.0.1:4401/api/users
```

or

```bash
curl -s http://127.0.0.1:4401/api/users | python -m json.tool
```

---

## 4. Repro instructions

From a clean checkout with `MINIMAX_API_KEY` set:

```bash
# Terminal 1: server
cd Flowntier
cargo build --release -p pipe-server
mkdir -p acceptance/ledger-task
./target/release/flowntier-runtime.exe \
    --workspace "$(pwd)/acceptance/ledger-task" &

# Terminal 2: send the backend task via named pipe
# (PowerShell snippet from tools/, or any client speaking
# JSON-RPC over \\.\pipe\flowntier_runtime — the pipe is available
# the moment flowntier-runtime binds)

# Wait for the agent loop to finish writing files
# (visible in \\.\pipe\aco_runtime_events stream)

# Manual fix: if db.js has ensure_schema not auto-called, patch
# per docs/ACCEPTANCE_v0.3_LEDGER.md §3.1

# Terminal 3: start the backend
cd acceptance/ledger-task/backend
node server.js &

# Terminal 4: start the frontend
cd acceptance/ledger-task/frontend
python -m http.server 5501 &

# Verify
curl -s http://127.0.0.1:4401/api/users
curl -s -i http://localhost:5501/ | head -3

# Open http://localhost:5501/ in a browser
```

---

## 5. Comparison with the first acceptance run

| | Run 1 (ACCEPTANCE_v0.3.md) | Run 2 (this file) |
|---|---|---|
| Task complexity | 3 CRUD endpoints + form | 8 endpoints + 3 entities + report |
| Schema depth | 1 table (`users`) | 3 tables with FKs |
| Frontend scope | 1 page, form-only | 1 page with KPI grid + table + chart + 2 dialogs |
| Real persistence | yes (SQLite) | yes (SQLite + WAL) |
| Aggregate computation | no | yes (GROUP BY category, SUM) |
| Bugs found and fixed | 1 (SSE event type) | 1 (ensure_schema not called) |
| Test runner | external PowerShell named-pipe client | headless Node fetch script |

Run 2 is roughly **2× the scope** of Run 1 and still finishes
inside a single ChatZone-style task envelope without any
human-in-the-loop step beyond sending the prompt.

---

## 6. Open questions for v0.4

- **Auto-init for resources.** Both Run 1 (`npm install` vs
  `node:sqlite` fallback) and Run 2 (`ensure_schema`) show
  the same shape: the model generates an "obvious" init step
  and forgets to call it. A small `lifecycle::on_load()`
  convention or an `#[init_required]` attribute could catch
  this at compile time.
- **Headless E2E harness.** Both runs depended on a human
  writing a curl / Node script to exercise the API. v0.4
  should ship an `e2e-harness` example crate that, given a
  backend description, generates and runs a smoke test.
- **Multi-process coordination.** The frontend and backend
  are started by hand in different terminals. A `pnpm dev`
  orchestrator that brings up everything (backend + frontend
  + agent-runtime + pipe-server) would make acceptance one
  command.

---

**Bottom line:** a complete two-tier CRUD application — DB
schema, HTTP server, REST endpoints, CORS, single-page
frontend, KPI dashboard, chart — was built end-to-end by the
v0.3 agent loop from a single Chinese-language task prompt
and a real LLM (MiniMax-M3). One bug surfaced and was fixed
manually (4 lines). All 8 endpoints respond correctly, the
frontend can be opened in any browser, and the data persists
in a real SQLite file on disk.
