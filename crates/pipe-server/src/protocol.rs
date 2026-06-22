//! Wire protocol for the ACO pipe server.
//!
//! Two channels (newline-delimited JSON, message-mode framing):
//!
//! * **RPC** — request/response. One connection = one round-trip.
//!   - request  : `{"jsonrpc":"2.0","id":N,"method":M,"params":{"path":...,"body":...}}`
//!   - response : `{"jsonrpc":"2.0","id":N,"result":{"status":S,"body":B}}`
//!     or `{"jsonrpc":"2.0","id":N,"error":{"code":-N,"message":"..."}}`
//!
//! * **Events** — long-lived connection; server pushes one
//!   JSON object per event: `{"kind":K,"agent_id":A,"...":...}`
//!
//! This mirrors `apps/runtime/src/aco_runtime/pipe_server.py`
//! byte-for-byte so the Tauri Rust client does not need to change.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Maximum bytes per message.
pub const MAX_LINE: usize = 1_048_576; // 1 MiB

// ── RPC ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String, // always "2.0"
    pub id: u64,
    pub method: String,
    pub params: RpcParams,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RpcParams {
    /// URL-style path (e.g. `/api/providers`). Preserved so handlers
    /// can keep their existing FastAPI-style signatures during the
    /// port.
    #[serde(default)]
    pub path: String,
    /// Request body. JSON object.
    #[serde(default)]
    pub body: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String, // always "2.0"
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<RpcResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResult {
    /// HTTP-style status code (200..299 = success).
    pub status: u16,
    /// Body (JSON).
    pub body: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcResponse {
    pub fn ok(id: u64, body: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(RpcResult { status: 200, body }),
            error: None,
        }
    }
    pub fn status(id: u64, status: u16, body: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(RpcResult { status, body }),
            error: None,
        }
    }
    pub fn err(id: u64, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(RpcError { code, message: message.into(), data: None }),
        }
    }
}

// ── Events ───────────────────────────────────────────────────────

/// JSON-RPC error codes (subset).
pub mod codes {
    pub const PARSE: i32 = -32700;
    pub const INVALID: i32 = -32600;
    pub const NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL: i32 = -32603;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_ok_roundtrip() {
        let r = RpcResponse::ok(7, serde_json::json!({"hello": "world"}));
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"status\":200"));
        let back: RpcResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, 7);
        let result = back.result.unwrap();
        assert_eq!(result.status, 200);
    }

    #[test]
    fn response_error_roundtrip() {
        let r = RpcResponse::err(7, codes::NOT_FOUND, "nope");
        let s = serde_json::to_string(&r).unwrap();
        let back: RpcResponse = serde_json::from_str(&s).unwrap();
        let err = back.error.unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "nope");
    }
}