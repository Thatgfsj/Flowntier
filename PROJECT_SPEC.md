# Agent Company OS (ACO)

> A Visual AI Software Company Powered by Multi-Agent Workflow

**Version:** v0.1 RFC

**Status:** Draft

**Author:** Thatgfsj

**Created:** 2026-06-18

---

## 目录

1. [Vision](#1-vision)
2. [Core Philosophy](#2-core-philosophy)
3. [Agent Architecture](#3-agent-architecture)
4. [Workflow](#4-workflow)
5. [Model Routing](#5-model-routing)
6. [Provider Layer](#6-provider-layer)
7. [Claude Code Adapter](#7-claude-code-adapter)
8. [UI Design](#8-ui-design)
9. [Agent Avatars](#9-agent-avatars)
10. [User Visibility](#10-user-visibility)
11. [Plugin System](#11-plugin-system)
12. [Project Structure](#12-project-structure)
13. [Roadmap](#13-roadmap)
14. [Design Principles](#14-design-principles)

---

# 1. Vision

## What is Agent Company OS?

Agent Company OS (ACO) is **not another AI IDE**.

It is an **AI Software Company Operating System**.

Users interact with a beautiful visual workspace while multiple AI agents collaborate behind the scenes just like a real software company.

Instead of one AI completing everything, ACO organizes specialized AI roles into a structured workflow:

* Requirement Analysis
* Planning
* Architecture
* Parallel Development
* Review
* Repair
* Final Delivery

The IDE is only the visualization layer.

The real intelligence comes from the workflow.

---

# 2. Core Philosophy

## Single Source of Truth

Only ONE agent owns the entire project.

That agent is:

> Chief Agent

No Worker knows the whole project.

No Worker talks to another Worker.

Workers only execute assigned tasks.

This avoids:

* duplicated work
* token explosion
* context pollution
* inconsistent implementations

The Chief Agent is responsible for maintaining global consistency.

---

## Planning Before Coding

Every coding task begins with planning.

Planning is never skipped.

Workflow:

```
User
  ↓
Chief Agent
  ↓
Planning
  ↓
Critic Review
  ↓
Final Plan
  ↓
Workers
```

This is similar to how experienced software teams work.

---

## Review Before Delivery

Every generated result must pass AI review before reaching the user.

Chief Agent decides whether criticism affects the final quality.

Minor issues may be ignored.

Major issues trigger automatic repair.

---

# 3. Agent Architecture

## Chief Agent

**Responsibilities:**

* Requirement analysis
* Requirement clarification
* Project planning
* Architecture design
* API design
* Dependency analysis
* Worker scheduling
* Merge
* Final decision
* User communication

The Chief Agent is the "brain" of the system.

**Recommended models:**

* Claude Opus
* Kimi K2
* GPT-5
* Gemini 2.5 Pro

---

## Critic Agent A

**Focus:**

* Bugs
* Runtime crashes
* Edge cases
* Backend implementation
* Logic correctness

Never cares about UI aesthetics.

---

## Critic Agent B

**Focus:**

* Architecture
* Maintainability
* Readability
* Frontend style
* Code organization

Never checks runtime bugs.

---

## Worker Agents

Generated dynamically.

**Examples:**

* Backend Worker
* Frontend Worker
* Database Worker
* API Worker
* Testing Worker
* Documentation Worker

**Workers:**

* never communicate
* never know project structure
* only know assigned tasks
* return completed work

---

# 4. Workflow

## Phase 1 — Requirement Analysis

Chief Agent understands user request.

## Phase 2 — Planning

Chief Agent creates:

* architecture
* task graph
* dependencies
* APIs
* interfaces

**Output:** Planning Document

## Phase 3 — Planning Review

```
Planning document
       ↓
   Critic A  +  Critic B
       ↓
   Chief Agent
       ↓
Final Planning Document
```

Only after approval can coding begin.

## Phase 4 — Worker Dispatch

Chief Agent creates Workers.

**Example:**

| Worker | Task |
|--------|------|
| Worker 1 | Backend Login |
| Worker 2 | Frontend Login |
| Worker 3 | Database |
| Worker 4 | Docker |

Each Worker receives only:

* objective
* interfaces
* dependencies
* coding requirements

Nothing else.

## Phase 5 — Development

Workers execute independently.

* No communication
* No shared memory

Chief Agent collects outputs.

## Phase 6 — Review

```
   Chief Agent
       ↓
   Critic A  +  Critic B
       ↓
   Discussion
       ↓
   Decision
```

**Possible outcomes:** `PASS` / `REPAIR` / `REWRITE`

## Phase 7 — Repair

Chief Agent assigns repair tasks.

Workers repair only requested parts.

Repeat review until approved.

## Phase 8 — Delivery

Chief Agent summarizes:

* Completed tasks
* Known limitations
* Files modified
* Final explanation

Return to user.

---

# 5. Model Routing

Different agents should use different models.

**Example:**

* **Chief Agent** → Claude Opus / Kimi K2
* **Critic A** → Gemini
* **Critic B** → Claude Sonnet
* **Worker** → MiniMax M3
* **Reporter** → Qwen

ACO should never bind a role to one model.

Instead:

```
Role
  ↓
Model Router
  ↓
Provider
  ↓
API
```

---

# 6. Provider Layer

Inspired by ccswitch.

**Supported providers:**

* Anthropic
* OpenAI
* Google Gemini
* Kimi
* MiniMax
* DeepSeek
* SiliconFlow
* OpenRouter
* Ollama
* LM Studio
* Custom OpenAI Compatible

**Every provider implements:**

* Chat
* Streaming
* Vision
* Reasoning
* Tool Calling
* Context Length

---

# 7. Claude Code Adapter

Claude Code becomes the execution engine.

ACO should communicate with Claude Code through an adapter.

```
Chief Agent
     ↓
Claude Code Adapter
     ↓
Claude Code CLI
```

**Future adapters:**

* Codex CLI
* OpenHands
* Aider
* Gemini CLI
* Custom CLI

ACO should remain independent from any single execution engine.

---

# 8. UI Design

**Style:** Mission Control

**Avoid:** Infinite canvas / Complex node graph

**Layout:**

```
┌──────────────────────────────────────────────────────┐
│  Top: Command Input                                   │
├──────────┬─────────────────────────────┬─────────────┤
│  Left    │  Center                     │  Right      │
│          │                             │             │
│  Agent   │  Discussion                 │  Task List  │
│  Organi- │  - Current reasoning        │  - Current  │
│  zation  │  - Current review           │    file     │
│          │  - Current task             │  - Progress │
│  - Chief ├─────────────────────────────┤             │
│  - A     │  Bottom: Claude Code        │             │
│  - B     │  Console (Streaming logs)   │             │
│  - W's   │                             │             │
├──────────┴─────────────────────────────┴─────────────┤
│  Timeline: Requirement → Planning → Review →         │
│           Workers → Repair → Completed               │
└──────────────────────────────────────────────────────┘
```

---

# 9. Agent Avatars

Every Agent has an avatar.

**Examples:**

* **Chief Agent** — Calm strategist, Blue theme
* **Critic A** — Serious engineer, Red theme
* **Critic B** — Architect, Purple theme
* **Workers** — Cute developers, Different colors

**Future:**

* Live2D
* Expressions
* Voice
* Animation

---

# 10. User Visibility

Users should not see every token.

Instead: **Important milestones.**

**Example:**

* ✓ Requirement analyzed
* ✓ Plan generated
* ✓ Review completed
* ✓ Worker 3 repairing
* ✓ Final review passed

Discussion is **summarized**, not fully exposed.

---

# 11. Plugin System

**Future plugins:**

* Git
* GitHub
* Terminal
* Docker
* MCP
* Browser
* Figma
* Database
* Slack
* Discord
* Email

Everything should be pluggable.

---

# 12. Project Structure

```
app/
  agents/
    chief/
    critic/
    worker/
  workflow/
  providers/
  adapter/
  ui/
  plugins/
  models/
  prompts/
  assets/
  storage/
  config/
```

---

# 13. Roadmap

## v0.1

* Basic workflow
* Chief
* Critics
* Workers
* Claude Code
* Simple UI

## v0.2

* Model routing
* Provider management
* Task history
* Workspace

## v0.3

* Project memory
* Workflow replay
* Planning visualization

## v0.4

* Plugin system
* Git integration
* Docker integration
* MCP

## v0.5

* Live2D
* Animated agents
* Voice
* Streaming interactions

## v1.0

* Complete AI Software Company
* Visual collaboration
* Extensible execution engines
* Professional workflow management
* Enterprise-ready architecture

---

# 14. Design Principles

* Always plan before coding.
* Workers never communicate.
* Chief Agent owns all context.
* Critics review every important result.
* Different roles use different models.
* Execution engines are replaceable.
* The UI visualizes the workflow rather than hiding it.
* **The workflow is the product.**
* **The IDE is only the interface.**

> ACO is not an AI IDE.
> ACO is an AI Software Company Operating System.
