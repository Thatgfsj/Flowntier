//! RPC method dispatcher.
//!
//! Maps `(method, path)` strings to an async handler. Handlers
//! receive the request body and return a JSON body or a tuple
//! `(status, body)`.
//!
//! The set of registered handlers mirrors what the Python
//! runtime used to serve under FastAPI. Only a minimal subset is
//! implemented here — enough to unblock the Tauri client; new
//! methods land as they're ported.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::protocol::{codes, RpcRequest, RpcResponse};

/// A handler: takes the request body, returns `(status, body)`.
pub type Handler = Arc<dyn Fn(Value) -> HandlerFuture + Send + Sync>;
pub type HandlerFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<(u16, Value), String>> + Send>>;

/// A registry of RPC handlers keyed by `(method, path)`.
#[derive(Default, Clone)]
pub struct Dispatcher {
    /// (HTTP method, path) -> handler. The pair is what HTTP itself
    /// uses to identify a route; using it as the key here means a
    /// GET and a PUT on the same path can coexist (e.g. `GET
    /// /api/router/roles` reads the role list, `PUT /api/router/roles`
    /// overwrites it).
    handlers: HashMap<(String, String), Handler>,
}

impl std::fmt::Debug for Dispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dispatcher")
            .field(
                "routes",
                &self
                    .handlers
                    .keys()
                    .map(|(m, p)| format!("{m} {p}"))
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Dispatcher {
    /// New empty dispatcher.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a handler for `(method, path)`.
    ///
    /// `method` is the HTTP verb (`GET`, `POST`, `PUT`, `PATCH`,
    /// `DELETE`, ...). The caller is responsible for keeping
    /// `(method, path)` unique; registering the same pair twice
    /// overwrites the previous handler, which is usually a bug
    /// in the caller — see `register_all` for the canonical
    /// endpoint list.
    ///
    /// Path patterns: a path containing `{name}` (one per segment)
    /// is a placeholder. During dispatch, the placeholder matches
    /// any non-empty segment of the incoming request path and the
    /// extracted value is written into the request body under the
    /// same key (e.g. `{name}` -> body["name"]). This lets
    /// handlers be registered once for an entire collection of
    /// concrete paths (PUT /api/settings/secrets/{name} matches
    /// /api/settings/secrets/OPENAI_API_KEY, etc.).
    pub fn register<F>(&mut self, method: impl Into<String>, path: impl Into<String>, f: F)
    where
        F: Fn(Value) -> HandlerFuture + Send + Sync + 'static,
    {
        self.handlers
            .insert((method.into().to_uppercase(), path.into()), Arc::new(f));
    }

    /// List registered routes as `METHOD path` pairs, sorted
    /// deterministically. Useful for diagnostics.
    pub fn methods(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .handlers
            .keys()
            .map(|(m, p)| format!("{m} {p}"))
            .collect();
        v.sort();
        v
    }

    /// Dispatch an RPC request. Looks up the handler by
    /// `(req.method, req.params.path)`.
    ///
    /// Lookup algorithm:
    ///   1. Exact match — fastest path.
    ///   2. Pattern match — scan registered paths with the
    ///      same method, find the first where the placeholder
    ///      pattern matches the incoming path. Extracted
    ///      placeholders are injected into the request body
    ///      under the same key.
    pub async fn dispatch(&self, req_id: u64, req: RpcRequest) -> RpcResponse {
        let method = req.method.to_uppercase();
        let path = req.params.path;
        let mut body = req.params.body.unwrap_or(Value::Null);

        // 1. Exact match.
        if let Some(handler) = self.handlers.get(&(method.clone(), path.clone())) {
            return match handler(body).await {
                Ok((status, b)) => RpcResponse::status(req_id, status, b),
                Err(e) => RpcResponse::err(req_id, codes::INTERNAL, e),
            };
        }

        // 2. Pattern match.
        let incoming_segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        for ((registered_method, registered_path), handler) in &self.handlers {
            if *registered_method != method {
                continue;
            }
            let pattern_segments: Vec<&str> = registered_path
                .split('/')
                .filter(|s| !s.is_empty())
                .collect();
            if pattern_segments.len() != incoming_segments.len() {
                continue;
            }
            let mut placeholder_values: Vec<(&str, String)> = Vec::new();
            let mut matched = true;
            for (p, i) in pattern_segments.iter().zip(incoming_segments.iter()) {
                if let Some(name) = p.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                    if i.is_empty() {
                        matched = false;
                        break;
                    }
                    placeholder_values.push((name, (*i).to_string()));
                } else if p != i {
                    matched = false;
                    break;
                }
            }
            if matched {
                // Inject placeholders into the body so handlers
                // can access them via `body.get("name")` etc.
                if let Value::Object(ref mut map) = body {
                    for (name, value) in &placeholder_values {
                        map.insert((*name).to_string(), Value::String(value.clone()));
                    }
                } else {
                    let mut map = serde_json::Map::new();
                    for (name, value) in &placeholder_values {
                        map.insert((*name).to_string(), Value::String(value.clone()));
                    }
                    body = Value::Object(map);
                }
                return match handler(body).await {
                    Ok((status, b)) => RpcResponse::status(req_id, status, b),
                    Err(e) => RpcResponse::err(req_id, codes::INTERNAL, e),
                };
            }
        }

        RpcResponse::err(
            req_id,
            codes::NOT_FOUND,
            format!("no handler registered for {method} {path}"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::RpcParams;

    fn req(method: &str, path: &str) -> RpcRequest {
        RpcRequest {
            jsonrpc: "2.0".into(),
            id: 1,
            method: method.into(),
            params: RpcParams { path: path.into(), body: None },
        }
    }

    #[tokio::test]
    async fn dispatches_known_method() {
        let mut d = Dispatcher::new();
        d.register("GET", "/api/ping", |_body| {
            Box::pin(async { Ok((200, serde_json::json!({"pong": true}))) })
        });
        let resp = d.dispatch(1, req("GET", "/api/ping")).await;
        let r = resp.result.unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body["pong"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn unknown_method_is_not_found() {
        let d = Dispatcher::new();
        let resp = d.dispatch(2, req("GET", "/nope")).await;
        let e = resp.error.unwrap();
        assert_eq!(e.code, codes::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_and_put_on_same_path_coexist() {
        // The v0.3 fix: previously the dispatcher only keyed on
        // path, so a second register on the same path silently
        // overwrote the first. With (method, path) as the key,
        // GET and PUT handlers can both be registered.
        let mut d = Dispatcher::new();
        d.register("GET", "/api/router/roles", |_body| {
            Box::pin(async {
                Ok((
                    200,
                    serde_json::json!({"op": "list", "roles": []}),
                ))
            })
        });
        d.register("PUT", "/api/router/roles", |_body| {
            Box::pin(async {
                Ok((
                    200,
                    serde_json::json!({"op": "update", "ok": true}),
                ))
            })
        });
        let list = d.dispatch(1, req("GET", "/api/router/roles")).await;
        let upd = d.dispatch(2, req("PUT", "/api/router/roles")).await;
        assert_eq!(list.result.unwrap().body["op"], "list");
        assert_eq!(upd.result.unwrap().body["op"], "update");
    }

    #[tokio::test]
    async fn method_is_case_insensitive() {
        let mut d = Dispatcher::new();
        d.register("get", "/api/ping", |_body| {
            Box::pin(async { Ok((200, serde_json::json!({"ok": true}))) })
        });
        // Lowercase 'get' is normalized to GET on register;
        // dispatch with uppercase GET should still find it.
        let resp = d.dispatch(1, req("GET", "/api/ping")).await;
        assert_eq!(resp.result.unwrap().status, 200);
    }
}