# UI Guidelines

> Mission Control design system for Agent Company OS

**Version:** v0.1 RFC
**Status:** Draft
**Author:** Thatgfsj
**Supersedes:** PROJECT_SPEC.md §8, §9
**Last updated:** 2026-06-18

---

## 1. Design Philosophy

ACO's UI is a **Mission Control**, not an IDE.

The user is a **product owner** monitoring a team of agents. They do not
edit code directly — they observe, intervene, and approve.

### Anti-patterns to avoid

* ❌ Infinite canvas (Figma/Miro style)
* ❌ Complex node-and-edge graphs
* ❌ Token-level streaming as the primary surface
* ❌ "AI IDE" chrome (file tree on the left, editor in the middle)

### Principles

* **Show milestones, not tokens.** Summarize. Reveal details on demand.
* **Agents are first-class characters.** They have names, avatars, voices.
* **Workflow is geography.** The 4 phases occupy 4 zones the user learns once.
* **One screen, one job.** No tabs. No modals over the main view.
* **Reversible actions.** Every Chief decision has an "undo" or "revert" affordance.

---

## 2. Layout Grid

The screen is divided into **5 fixed zones** plus a **horizontal timeline**.

```
┌────────────────────────────────────────────────────────────────────┐
│ Z1 — TOP BAR   │ Command input + project name + user menu         │ 64px
├──────────┬─────────────────────────────────────────────┬───────────┤
│          │                                             │           │
│ Z2 LEFT  │ Z3 CENTER                                   │ Z4 RIGHT  │
│ Agent    │ Discussion / Reasoning / Review / Task     │ Task List │
│ Roster   │                                             │ Progress  │
│ 280px    │ flex                                        │ 360px     │
│          │                                             │           │
├──────────┴─────────────────────────────────────────────┴───────────┤
│ Z5 BOTTOM    │ Claude Code console — streaming logs (collapsible)  │ 240px
└────────────────────────────────────────────────────────────────────┘
              T0 TIMELINE  ──▶  Requirement · Planning · Review · Workers · Repair · Done
```

### Breakpoints

| Width         | Behavior                                            |
|---------------|-----------------------------------------------------|
| ≥ 1440 px     | Full 5-zone layout, timeline visible                |
| 1024–1439 px  | Bottom console auto-collapses; user toggles         |
| 768–1023 px   | Right panel becomes a slide-over drawer             |
| < 768 px      | **Not supported in v0.1** — show "use desktop" hint |

---

## 3. Zone Specifications

### Z1 — Top Bar

* Project name (left)
* Command input (center) — `/command` style
* User menu + settings (right)
* Height: 64 px, fixed

### Z2 — Left Roster (Agent Organization)

Vertical list of agent cards, grouped by role:

```
▼ Chief Agent          [active]
   Calm strategist · blue

▼ Critics
   Critic A           [idle]  red
   Critic B           [idle]  purple

▼ Workers
   Backend Worker     [working]  teal
   Frontend Worker    [queued]   amber
   Database Worker    [done]     green
```

* Each card: avatar (40×40), name, status pill, role icon
* Click → opens that agent's history in Z3
* Drag → reorder priority (Chief Agent only)

### Z3 — Center (Discussion / Reasoning / Review / Task)

Tabbed surface, **but tabs are agent-scoped, not feature-scoped**.

* Default view: **Chief Agent's current reasoning** (a transcript card with
  the most recent milestone + a "Show full log" expand button)
* When a Critic speaks → switches to Critic's review (red/purple accent)
* When a Worker reports → switches to Worker's diff summary
* Switching back to Chief is a single click; tabs are ephemeral

The user **never sees raw model output by default** — only structured
deliverables (Plans, Reviews, Diffs, Reports).

### Z4 — Right Panel (Task List + Progress)

* Current task (title, owner, ETA, blockers)
* Task tree (parent / children, indentation ≤ 3 levels)
* Progress: bar + percentage + "X of Y subtasks"
* File being edited (path + line range)
* Per-task "Open in console" button

### Z5 — Bottom Console

* Streaming logs from Claude Code (and future adapters)
* Collapsible to a 32 px bar showing "Console: 12 events"
* Color-coded by source: Chief=blue, Worker=teal, Critic=red/purple,
  System=gray
* User can **mute** non-critical sources
* Free-text search across all logs

### T0 — Timeline (under console)

* Horizontal stepper, 8 phases (see [WORKFLOW_SPEC.md](./WORKFLOW_SPEC.md))
* States: `pending` (gray) / `active` (pulsing accent) / `done` (green) / `failed` (red)
* Hover a step → shows duration, owner, summary
* Click a done step → opens a snapshot view (replay)

---

## 4. Color Tokens

### 4.1 Role palette (semantic)

| Token              | Light     | Dark      | Use                              |
|--------------------|-----------|-----------|----------------------------------|
| `--color-chief`    | `#3B6FE0` | `#6E94F0` | Chief Agent accents              |
| `--color-critic-a` | `#D04A4A` | `#F06E6E` | Critic A (logic/bugs)           |
| `--color-critic-b` | `#7B4FBE` | `#A380D6` | Critic B (architecture/style)   |
| `--color-worker-1` | `#1B9E8C` | `#3BC9B3` | Worker A (rotate for more)      |
| `--color-worker-2` | `#E08A2E` | `#F0A852` | Worker B                        |
| `--color-worker-3` | `#4A8A3B` | `#76C25E` | Worker C                        |
| `--color-worker-4` | `#8E5BC1` | `#B580D6` | Worker D                        |

Workers cycle through `--color-worker-1..8` based on spawn order.

### 4.2 Status palette

| Token              | Light     | Dark      | Use              |
|--------------------|-----------|-----------|------------------|
| `--status-pending` | `#9CA3AF` | `#4B5563` | queued           |
| `--status-active`  | `#3B6FE0` | `#6E94F0` | running (pulse)  |
| `--status-done`    | `#16A34A` | `#4ADE80` | completed        |
| `--status-failed`  | `#DC2626` | `#F87171` | error            |
| `--status-warn`    | `#D97706` | `#FBBF24` | needs attention  |

### 4.3 Surface palette

| Token                | Light     | Dark      |
|----------------------|-----------|-----------|
| `--surface-1`        | `#FFFFFF` | `#0E1116` |
| `--surface-2`        | `#F4F5F7` | `#161B22` |
| `--surface-3`        | `#E5E7EB` | `#21262D` |
| `--text-primary`     | `#111827` | `#E6EDF3` |
| `--text-secondary`   | `#6B7280` | `#8B949E` |
| `--border`           | `#E5E7EB` | `#30363D` |

### 4.4 Theme

* **Default:** dark (mission-control aesthetic)
* **Alternative:** light (planned for v0.2)
* User preference persisted locally; never synced to cloud in v0.1

---

## 5. Typography

| Token         | Family                        | Size  | Weight | Use                       |
|---------------|-------------------------------|-------|--------|---------------------------|
| `--font-ui`   | Inter (system fallback)       | 14 px | 400    | Body, buttons             |
| `--font-h1`   | Inter                         | 24 px | 600    | Page titles (rare)        |
| `--font-h2`   | Inter                         | 18 px | 600    | Section heads             |
| `--font-mono` | JetBrains Mono (system mono)  | 13 px | 400    | Code, paths, IDs          |
| `--font-avatar` | Inter                       | 12 px | 500    | Agent name on avatar      |

* Line height: 1.5 for body, 1.25 for headings.
* No font-size below 12 px.

---

## 6. Components

### 6.1 `AgentCard`

```
┌─────────────────────────────────┐
│  ┌──┐  Chief Agent    [active]  │
│  │🦊│  Calm strategist          │
│  └──┘  ●● thinking...  3s       │
└─────────────────────────────────┘
```

* Avatar 40×40, role color border 2 px
* Status pill (right)
* One-line subtitle (role tagline)
* Optional "thinking" indicator with elapsed time
* Optional progress bar (when agent is mid-task)

### 6.2 `PhaseTimeline`

8-step horizontal stepper. See §3 T0.

### 6.3 `TaskItem`

```
[✓] Backend: implement /login          Worker 1  ·  3m 12s
    └─ src/auth/login.py:42–88
```

* Status icon, title, owner, duration
* Click → expand to show file list, code excerpt, or diff

### 6.4 `ReasoningBubble`

* Card with agent avatar + role-color top border 3 px
* Heading: agent name + step ("Planning — drafting API")
* Body: 1–3 sentence summary by default; "Show full transcript" reveals raw
* Timestamp in top-right (relative: "3s ago")

### 6.5 `ReviewVerdict`

* Big verdict badge: `PASS` (green) / `REPAIR` (amber) / `REWRITE` (red)
* Bulleted issue list (from Critic A and/or B)
* Confidence score (0–1)
* "Accept" / "Send to repair" buttons (Chief only — disabled for user)

### 6.6 `ConsoleLine`

```
[12:34:56]  Worker 1  ·  src/auth/login.py:42  created
[12:34:57]  Worker 1  ·  test 3 passed
[12:34:58]  Chief     ·  ✓ Phase 4 complete
```

* Monospace, 13 px
* Color-coded by source
* User can filter by source and free-text search

---

## 7. Agent Avatars

### 7.1 Visual

* **v0.1:** static PNG/SVG, 256×256 source, 40/80/120 display sizes
* **v0.5:** Live2D model (see [ROADMAP.md](./ROADMAP.md))
* Each agent has 4 default states: `idle`, `thinking`, `speaking`, `error`

### 7.2 Personality mapping

| Agent        | Theme | Archetype        | Suggested motif               |
|--------------|-------|------------------|-------------------------------|
| Chief Agent  | Blue  | Calm strategist  | Owl, lighthouse, captain       |
| Critic A     | Red   | Serious engineer | Hawk, magnifying glass, judge  |
| Critic B     | Purple| Architect        | Owl, compass, cathedral        |
| Workers      | Cycle | Cute developer   | Mascot per role (cat, fox, …)  |

### 7.3 Accessibility

* Every avatar has a `role="img"` with `aria-label="Chief Agent, thinking"`
* If avatar fails to load, fall back to colored circle with initials

---

## 8. Motion & Timing

* **Phase transitions:** 240 ms ease-in-out, timeline pulse
* **Status changes:** 160 ms fade + scale 0.96 → 1.0
* **Agent "thinking" indicator:** 1.2 s loop, opacity 0.4 → 1.0
* **Console new line:** 80 ms slide-in from top
* **No animation** longer than 400 ms (users get bored fast)
* **Respect `prefers-reduced-motion`** — disable non-essential motion

---

## 9. Accessibility (a11y)

* WCAG 2.1 AA color contrast on all text/background pairs
* Full keyboard nav:
  * `Tab` walks Z1 → Z2 → Z3 → Z4 → Z5
  * `Cmd/Ctrl+K` focuses command input
  * `Cmd/Ctrl+L` toggles bottom console
  * `Esc` closes any drawer
* Screen reader landmarks: `<header>`, `<nav>`, `<main>`, `<aside>`, `<footer>`
* Live regions: console + reasoning bubbles use `aria-live="polite"`
* All interactive elements ≥ 32 × 32 px hit target
* Focus ring: 2 px solid `--color-chief` outline, never `outline: none`

---

## 10. Empty / Loading / Error States

Every async surface must define all four:

| State    | Z3 Center                      | Z4 Right                | Z5 Console                |
|----------|--------------------------------|--------------------------|---------------------------|
| Empty    | "Awaiting your first request"  | "No tasks yet"           | "—"                        |
| Loading  | Pulsing Chief avatar + "Thinking..." | Skeleton rows (3)   | "Connecting to Claude Code…" |
| Error    | Red banner + retry button      | Red dot on task + tooltip | Red line + stack trace   |
| Success  | Milestone card ✓               | All tasks green          | "All workers idle"         |

---

## 11. Internationalization (i18n)

* All strings externalized to a single resource file
* v0.1 ships **English only**
* v0.2: Simplified Chinese
* Right-to-left (RTL) languages: **not in scope** for v1.0

---

## 12. Out of Scope for v0.1

The following are explicitly **not** part of v0.1:

* Custom theme builder
* Live2D avatars
* Plugin-contributed UI panels
* Mobile / responsive < 768 px
* Light theme
* Voice / speech input

These are tracked in [ROADMAP.md](./ROADMAP.md).

---

## 13. Open Questions

1. Should the user be able to **pause** an in-flight Chief decision? (proposed yes)
2. Should we expose a **diff viewer** in Z3 for code changes, or just a summary? (proposed summary-first, diff on demand)
3. Should the timeline be **fixed at the top** or **draggable to the bottom**? (proposed: bottom in v0.1, user-configurable in v0.2)

---

**RFC ends. Comments welcome on the project issue tracker.**
