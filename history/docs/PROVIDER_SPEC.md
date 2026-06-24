# Provider Spec

> Multi-provider model layer for Agent Company OS

**Version:** v0.1 RFC
**Status:** Draft
**Author:** Thatgfsj
**Supersedes:** PROJECT_SPEC.md §5, §6
**Last updated:** 2026-06-18

---

## 1. Goals

ACO must work with **any** LLM provider that exposes a chat-completions-style
API, including local ones. The provider layer:

1. Hides provider-specific quirks behind a single `Provider` trait
2. Lets the **Model Router** choose a provider per role
3. Supports **failover** when a provider errors or rate-limits
4. Tracks **cost** and **token usage** centrally
5. Keeps **API keys in env vars only** — never on disk in v0.1
6. Is **pluggable** — adding a new provider is a single file

---

## 2. Supported Providers (v0.1)

| Provider        | API style           | Native streaming | Native tools | Native vision | Notes                       |
|-----------------|---------------------|------------------|--------------|---------------|-----------------------------|
| Anthropic       | Anthropic Messages  | ✅               | ✅           | ✅            | First-class                 |
| OpenAI          | Chat Completions    | ✅               | ✅           | ✅            |                             |
| Google Gemini   | Gemini generateContent | ✅            | ✅           | ✅            |                             |
| Kimi (Moonshot) | OpenAI-compatible   | ✅               | ✅           | ❌            |                             |
| MiniMax         | OpenAI-compatible   | ✅               | partial      | ❌            |                             |
| DeepSeek        | OpenAI-compatible   | ✅               | ✅           | ❌            |                             |
| SiliconFlow     | OpenAI-compatible   | ✅               | partial      | partial       |                             |
| OpenRouter      | OpenAI-compatible   | ✅               | ✅           | ✅            | Aggregator                  |
| Ollama (local)  | OpenAI-compatible   | ✅               | partial      | model-dep.    | Local                       |
| LM Studio       | OpenAI-compatible   | ✅               | partial      | model-dep.    | Local                       |
| Custom          | OpenAI-compatible   | n/a              | n/a          | n/a           | User-provided base URL + key |

> "OpenAI-compatible" means we reuse the OpenAI client with a custom
> `base_url`. This covers ~80% of providers with no extra code.

---

## 3. The `Provider` Trait

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &ProviderId;
    fn models(&self) -> &[ModelSpec];

    async fn chat(
        &self,
        req: ChatRequest,
        opts: RequestOpts,
    ) -> Result<ChatResponse, ProviderError>;

    async fn stream(
        &self,
        req: ChatRequest,
        opts: RequestOpts,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, ProviderError>;

    fn capabilities(&self) -> Capabilities;
    fn context_window(&self, model: &ModelId) -> usize;
}
```

### 3.1 `ChatRequest` (provider-agnostic)

```rust
pub struct ChatRequest {
    pub model: ModelId,
    pub messages: Vec<Message>,           // system/user/assistant/tool
    pub tools: Vec<ToolSpec>,             // JSON-Schema functions
    pub tool_choice: ToolChoice,          // auto | any | named | none
    pub temperature: Option<f32>,         // 0.0 - 2.0
    pub max_tokens: Option<u32>,
    pub stop: Option<Vec<String>>,
    pub response_format: Option<ResponseFormat>, // text | json_object
    pub metadata: HashMap<String, String>,
}
```

### 3.2 `Message`

```rust
pub enum Message {
    System { content: String },
    User   { content: Vec<ContentPart> },  // text + images
    Assistant { content: String, tool_calls: Vec<ToolCall> },
    Tool      { tool_call_id: String, content: String },
}

pub enum ContentPart {
    Text(String),
    Image { url: String, detail: ImageDetail },
}
```

### 3.3 `ChatResponse`

```rust
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: FinishReason,
    pub usage: Usage,
    pub raw: serde_json::Value,           // provider's raw payload
}

pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cached_input_tokens: u32,         // where supported
    pub cost_usd: Option<f64>,            // computed by provider
}
```

### 3.4 `Capabilities`

```rust
pub struct Capabilities {
    pub chat: bool,
    pub stream: bool,
    pub vision: bool,
    pub tool_call: bool,                  // native, not prompt-engineered
    pub parallel_tool_calls: bool,
    pub json_mode: bool,
    pub prompt_caching: bool,
    pub reasoning_effort: bool,           // supports low/med/high
    pub max_context_window: usize,
}
```

---

## 4. Model Spec

```rust
pub struct ModelSpec {
    pub id: ModelId,                      // e.g. "claude-opus-4-8"
    pub display_name: String,             // "Claude Opus 4.8"
    pub context_window: usize,
    pub max_output_tokens: u32,
    pub input_cost_per_mtok: f64,         // USD
    pub output_cost_per_mtok: f64,
    pub capabilities: Capabilities,
    pub deprecated: bool,
    pub sunset_date: Option<NaiveDate>,
}
```

Cost data is **advisory** in v0.1 — it's used for the cost dashboard, not
for billing. Update it manually in `models.toml` when prices change.

---

## 5. Configuration

### 5.1 Storage

| Item              | Where                            |
|-------------------|----------------------------------|
| API keys          | Environment variables only       |
| Base URLs         | `config/providers.toml`          |
| Model list        | `config/providers.toml`          |
| Default model per role | `config/router.toml`         |
| Cost rates        | `config/providers.toml`          |
| Per-request overrides | `TASK_ASSIGN.model_hint`     |

**Never** write API keys to disk. The runtime must reject any key found
in a config file.

### 5.2 `config/providers.toml`

```toml
[providers.anthropic]
type        = "anthropic"
base_url    = "https://api.anthropic.com"
api_key_env = "ANTHROPIC_API_KEY"
enabled     = true

[providers.anthropic.models.claude-opus-4-8]
display_name      = "Claude Opus 4.8"
context_window    = 200000
max_output_tokens = 32000
input_cost_mtok   = 15.0
output_cost_mtok  = 75.0
capabilities      = ["chat", "stream", "vision", "tool_call", "json_mode", "prompt_caching", "reasoning_effort"]

[providers.openai]
type        = "openai"
base_url    = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
enabled     = true

# Custom OpenAI-compatible
[providers.siliconflow]
type        = "openai_compat"
base_url    = "https://api.siliconflow.cn/v1"
api_key_env = "SILICONFLOW_API_KEY"
enabled     = false   # opt-in

# Local
[providers.ollama]
type        = "openai_compat"
base_url    = "http://localhost:11434/v1"
api_key_env = "OLLAMA_NO_KEY"   # any non-empty string
enabled     = true
```

### 5.3 `config/router.toml`

```toml
[defaults]
chief      = "anthropic:claude-opus-4-8"
critic_a   = "google:gemini-2-5-pro"
critic_b   = "anthropic:claude-sonnet-4-6"
worker     = "minimax:minimax-m3"
reporter   = "deepseek:deepseek-reasoner"

[fallback.chief]
chain = ["anthropic:claude-opus-4-8", "kimi:kimi-k2", "openai:gpt-5", "google:gemini-2-5-pro"]

[fallback.worker]
chain = ["minimax:minimax-m3", "deepseek:deepseek-chat", "openai:gpt-5-mini"]
```

---

## 6. Model Router

The router maps a **role** to a **provider chain** at request time.

```rust
pub struct Router {
    defaults: HashMap<Role, Vec<ProviderModel>>,  // role -> ordered chain
    overrides: HashMap<TaskId, ProviderModel>,    // per-task pin
}

impl Router {
    pub async fn pick(
        &self,
        role: Role,
        task: Option<TaskId>,
    ) -> Result<ProviderModel, RouterError>;
}
```

### 6.1 Selection rules

1. If `task` has a pinned `model_hint` and that model is enabled, use it.
2. Else use the first enabled model in the role's `chain`.
3. Skip models whose `Capabilities` don't satisfy the request
   (e.g., vision request → must have `vision: true`).
4. Skip models that are `deprecated` and past `sunset_date`.

### 6.2 Failover

If a request fails with a **retryable** error
(timeout / 5xx / 429 with `Retry-After`), the router:

1. Waits `Retry-After` seconds (or 1s default).
2. Tries the next model in the chain.
3. After 3 attempts on the same model, moves to the next chain entry.
4. After exhausting the chain, returns `RouterError::Exhausted`.

**Non-retryable** errors (400, 401, 403) are **not** retried — they
bubble up immediately (the user has misconfigured something).

---

## 7. Cost Tracking

Every `ChatResponse.usage` is logged to `storage/usage.sqlite` with:

```sql
CREATE TABLE usage (
  id              TEXT PRIMARY KEY,         -- ULID
  ts              INTEGER NOT NULL,         -- unix seconds
  task_id         TEXT,
  agent_id        TEXT,                     -- 'agent:worker:...'
  provider        TEXT NOT NULL,
  model           TEXT NOT NULL,
  input_tokens    INTEGER NOT NULL,
  output_tokens   INTEGER NOT NULL,
  cached_tokens   INTEGER NOT NULL DEFAULT 0,
  cost_usd        REAL,
  finish_reason   TEXT
);

CREATE INDEX idx_usage_ts ON usage(ts);
CREATE INDEX idx_usage_task ON usage(task_id);
```

The UI surfaces this in v0.2 (cost dashboard). v0.1 logs only.

---

## 8. Health & Warmup

* On startup, the runtime pings each enabled provider's `/models` endpoint.
* Provider that fails 3 times consecutively is marked `unhealthy` and
  excluded from the chain until the next ping cycle (60 s).
* Workers must never see an unhealthy provider.

---

## 9. Error Model

```rust
pub enum ProviderError {
    Auth,                       // 401/403 — not retryable
    BadRequest(String),         // 400     — not retryable
    RateLimited { retry_after: Duration },  // 429 — retryable
    Server { status: u16, body: String },   // 5xx   — retryable
    Network(io::Error),                    // any    — retryable
    Timeout(Duration),                      // any    — retryable
    ContextLengthExceeded { requested: u32, max: u32 },  // not retryable
    ContentFiltered,                        // not retryable
    UnsupportedCapability(String),         // not retryable
    Other(String),
}
```

`Retryable` errors go through the failover chain. `Not retryable` errors
bubble up to the Chief, which may repair, re-plan, or escalate to the user.

---

## 10. Adding a New Provider

For an **OpenAI-compatible** provider:

1. Add an entry to `config/providers.toml` with `type = "openai_compat"`.
2. Set the env var for the API key.
3. Add models with their specs.
4. Restart. (Hot-reload is a v0.3 feature.)

For a **non-OpenAI** provider (e.g., a future Anthropic-rivaling API):

1. Implement the `Provider` trait in `providers/<name>.rs`.
2. Register it in `providers/registry.rs`.
3. Add config + model specs in `providers.toml`.

Estimated effort: **1 day** for the trait, **1 day** for tests, **0.5 day**
for docs.

---

## 11. Open Questions

1. Should we cache responses by `(model, messages-hash)` to save cost on
   re-runs? (proposed yes, opt-in, v0.2)
2. Should the router learn from past failures (i.e., downrank unreliable
   providers automatically)? (proposed yes, v0.3)
3. Should per-task token budgets be enforced **before** the request
   (estimate) or **after** (reject if exceeded)? (proposed: estimate first,
   hard cap with a configurable buffer)

---

**RFC ends.**
