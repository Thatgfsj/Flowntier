# agent-core

The in-process agent execution engine for Flowntier.

## Overview

`agent-core` provides the core runtime loop that powers all Flowntier agents (Chief, BugHunter, Reviewer, and Worker). It is written in pure Rust and replaces the previous two-sidecar architecture.

## Architecture

```
┌────────────────────────────────────────────────────────┐
│                      Agent Loop                        │
│   (LLM Stream → Tool Call Detection → Execution)       │
├───────────────┬────────────────────────┬───────────────┤
│   Providers   │         Tools          │   Workspace   │
│  - OpenAI     │  - bash (Git/PS/cmd)   │  - UNC-safe   │
│  - Anthropic  │  - read / write        │  - Case-norm  │
│  - Compat     │  - patch / grep / glob │  - Safe path  │
└───────────────┴────────────────────────┴───────────────┘
```

## Key Modules

- `loop_`: Main agent execution loop (`Agent::run`). Handles iteration limits, repeat-failure detection with warnings, token accounting, and streaming tool events.
- `workspace`: Workspace root isolation and path normalization (`Workspace::contains`, `Workspace::resolve`, `Workspace::relativize`). Fully normalized for Windows UNC paths (`\\?\`) and cross-platform case-insensitivity.
- `provider`: LLM streaming clients:
  - `openai`: OpenAI and OpenAI-compatible endpoints (supports DeepSeek, vLLM, Ollama, LiteLLM with compliant `content: null` and fallback `index`).
  - `anthropic`: Anthropic Messages API (`/v1/messages`) with typed `tool_result` blocks and role alternating.
- `tool`: Built-in tools:
  - `bash`: Cross-platform shell executor (auto-probes Git Bash, PowerShell, and cmd).
  - `read`, `write`, `patch`: Workspace-scoped file manipulation tools.
  - `grep`, `glob`: Code search tools.
- `prompt`: System prompts and role configurations.
- `event`: Typed `AgentEvent` stream dispatched to UI via event bus.
