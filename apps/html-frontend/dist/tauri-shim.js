// Tauri 2.x IPC shim for the Flowntier HTML frontend
// (v0.4.21, event 000058).
//
// In the Tauri shell, every @tauri-apps/api call lands in
// window.__TAURI_INTERNALS__.invoke(cmd, args, options). Tauri's
// Rust main process dispatches that into the matching
// `#[tauri::command]` registered via generate_handler!.
//
// In the browser host, there's no Rust main process. Instead,
// we translate each invoke into a POST /rpc against the
// pipe-server's HTTP bridge (127.0.0.1:8765) and translate each
// listen() into a Server-Sent Events subscription. The body
// shape on the bridge is the same FastAPI-style {path, body}
// JSON-RPC envelope the named-pipe transport uses, so handlers
// in pipe-server/src/handlers.rs are unchanged.
//
// Wire shape:
//
//   desktop (Tauri 2.x):
//     invoke(cmd, args, opts) -> IPC msg -> Rust main thread
//     listen(event, handler)  -> plugin:event|listen + transformCallback
//
//   browser (this shim):
//     invoke(cmd, args, opts) -> POST /rpc  {method,path:"/__cmd/<cmd>",body:args}
//     listen(event, handler)  -> EventSource('/events') + dispatch by event.kind
//
// "cmd → real RPC path" mapping table is below. Each entry is
// `{method, pathTemplate}` where pathTemplate can use {argName}
// placeholders that get substituted from `args`.
//
// When the chairman adds a new tauri command, only the mapping
// table needs updating — no pipe-server rebuild.

(function () {
  'use strict';

  // ── Config ──────────────────────────────────────────────────
  // The runtime binds to 127.0.0.1:8765 by default. If your
  // machine already has a service on 8765 (a Python MCP server,
  // an old dev build, etc.), launch the runtime with
  // FLOWNTIER_HTTP_BRIDGE=127.0.0.1:18765 and update the
  // `bridge` localStorage entry to match — the runtime's bridge
  // will pick the same port automatically.
  const BRIDGE = (() => {
    try {
      const override = localStorage.getItem('flowntier.bridge');
      return override || 'http://127.0.0.1:18765';
    } catch { return 'http://127.0.0.1:18765'; }
  })();

  // ── Cmd → {method, pathTemplate, body?} ──────────────────────
  // Each entry maps a Tauri command name to a FastAPI-style RPC
  // request against the pipe-server. pathTemplate may contain
  // {argName} placeholders substituted from the invoke `args`.
  //
  // body:
  //   "args"         — forward the whole `args` object as body
  //   undefined      — no body (GET-style)
  //   a function     — body = fn(args)
  //
  // Special case: cmd names starting with "plugin:event|" are
  // dispatched to the local SSE subscriber map (no HTTP).
  const CMD_MAP = {
    // ── v0.4.19+ runtime handshake & health ───────────────
    health_check:              { method: 'GET',  path: '/health' },
    rpc_version:               { method: 'GET',  path: '/api/rpc/version' },

    // ── Secrets ─────────────────────────────────────────────
    list_secrets:               { method: 'GET',  path: '/api/settings/secrets' },
    save_secret:               { method: 'PUT',  path: '/api/settings/secrets/{name}', body: ({name, value}) => ({ name, value }) },
    delete_secret:             { method: 'DELETE', path: '/api/settings/secrets/{name}' },
    reveal_secret:             { method: 'GET',  path: '/api/settings/secrets/{name}/reveal' },
    seed_secrets:              { method: 'POST', path: '/api/settings/secrets/_seed' },

    // ── Providers (v0.4.10+) ───────────────────────────────
    list_providers:            { method: 'GET',  path: '/api/providers' },
    toggle_provider:           { method: 'PATCH', path: '/api/providers/{id}', body: ({id, enabled}) => ({ enabled }) },
    fetch_provider_models:     { method: 'GET',  path: '/api/providers/{id}/models' },
    add_custom_provider:       { method: 'POST', path: '/api/providers/custom', body: 'args' },
    remove_custom_provider:    { method: 'DELETE', path: '/api/providers/custom/{id}' },

    // ── Router roles (v0.4.18+) ────────────────────────────
    list_router_roles:         { method: 'GET',  path: '/api/router/roles' },
    list_router_models:        { method: 'GET',  path: '/api/router/models' },
    update_router_roles:       { method: 'PUT',  path: '/api/router/roles', body: 'args' },
    get_role_resolve_status:   { method: 'GET',  path: '/api/router/roles/{role}/resolve', body: ({role}) => ({ role }) },

    // ── Workdir / nwt (v0.4.16+) ───────────────────────────
    get_workdir:               { method: 'GET',  path: '/api/workdir' },
    set_workdir:               { method: 'POST', path: '/api/workdir', body: ({path}) => ({ path }) },
    set_workdir_with_nwt:      { method: 'POST', path: '/api/workdir/with-nwt', body: ({path}) => ({ path }) },
    clear_workdir:             { method: 'POST', path: '/api/workdir/_clear' },

    // ── Diagnostics / data wipe ────────────────────────────
    get_diagnostics:           { method: 'GET',  path: '/api/diagnostics' },
    wipe_all_data:             { method: 'POST', path: '/api/_wipe' },

    // ── Logging ─────────────────────────────────────────────
    log_frontend_error:        { method: 'POST', path: '/api/_log_error', body: 'args' },

    // ── Chat / agent (v0.4.19+) ────────────────────────────
    run_agent_task:            { method: 'POST', path: '/api/run_task', body: 'args' },
    start_workflow_cmd:        { method: 'POST', path: '/api/workflow/start', body: 'args' },
    get_workflow:              { method: 'GET',  path: '/api/workflow/{id}' },
    cancel_workflow:           { method: 'POST', path: '/api/workflow/{id}/cancel' },

    // ── KV / sample / first-run ────────────────────────────
    kv_get:                    { method: 'GET',  path: '/api/kv/{key}' },
    kv_set:                    { method: 'POST', path: '/api/kv/{key}', body: ({key, value}) => ({ value }) },
    load_sample_workflow:      { method: 'GET',  path: '/api/sample/auth_login' },
    first_run_complete:        { method: 'POST', path: '/api/kv/first_run/complete' },

    // ── i_ching (decorative; v0.4 uses it as onboarding) ───
    draw_i_ching:              { method: 'POST', path: '/api/i_ching/draw' },

    // ── Plugins ─────────────────────────────────────────────
    list_plugins:              { method: 'GET',  path: '/api/plugins' },
    invoke_plugin:             { method: 'POST', path: '/api/plugins/{name}/invoke', body: 'args' },

    // ── v0.4.20 quota tracker ──────────────────────────────
    get_quota_status:          { method: 'GET',  path: '/api/quota/status' },
    reset_quota:               { method: 'POST', path: '/api/quota/reset', body: 'args' },
    get_role_quota_status:     { method: 'GET',  path: '/api/router/roles/{role}/resolve', body: ({role}) => ({ role }) },

    // ── Internal: log search (v0.4.12+) ────────────────────
    search_log:                { method: 'GET',  path: '/api/_log_search' },

    // ── Internal: events_bridge (Rust-side; no-op on browser)
    events_bridge:             { method: 'GET',  path: '/health' },
  };

  // ── RPC transport ─────────────────────────────────────────
  let nextRpcId = 1;
  async function rpc(method, path, body) {
    const id = nextRpcId++;
    const resp = await fetch(`${BRIDGE}/rpc`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        jsonrpc: '2.0', id, method,
        params: { path, body: body || {} },
      }),
    });
    if (!resp.ok) {
      throw new Error(`bridge /rpc returned HTTP ${resp.status}`);
    }
    const data = await resp.json();
    if (data.error) {
      throw new Error(`${data.error.code}: ${data.error.message}`);
    }
    return data.result;
  }

  // ── Build path + body for a given (cmd, args) ─────────────
  function buildRequest(cmd, args) {
    const spec = CMD_MAP[cmd];
    if (!spec) {
      throw new Error(`tauri-shim: unknown cmd "${cmd}" (add to CMD_MAP in dist/tauri-shim.js)`);
    }
    // Substitute {argName} in pathTemplate.
    const path = spec.path.replace(/\{(\w+)\}/g, (_, name) => {
      if (args && args[name] != null) return encodeURIComponent(args[name]);
      throw new Error(`tauri-shim: cmd "${cmd}" missing arg "${name}"`);
    });
    let body;
    if (spec.body === 'args') body = args || {};
    else if (typeof spec.body === 'function') body = spec.body(args || {});
    return { method: spec.method, path, body };
  }

  // ── Tauri event system shim (plugin:event|listen / |unlisten)
  // In a Tauri shell, listen() registers an IPC callback. Here
  // we keep a local subscriber map and fan out each AgentEvent
  // received from the SSE bridge to every matching subscriber.
  // Each subscriber gets a unique id returned by
  // transformCallback so it can unlisten cleanly.
  const eventSubscribers = new Map(); // eventName -> Map<subId, handler>

  function fanoutEvent(agentEvent) {
    // Each SSE message is a serialised AgentEvent (or WfEvent
    // from the workflow log; both pass through the same pipe).
    // Tauri 2.x's `listen(eventName, handler)` matches on the
    // eventName string that the desktop app.emit() chose.
    //
    // The Tauri shell uses `wf:event` as the channel name and
    // emits BOTH WfEvent and AgentEvent under that single name
    // (see apps/desktop/src-tauri/src/lib.rs `events_bridge`).
    // ChatZone listens to `wf:event` and filters by `kind` field
    // inside the payload. So in the browser shim, every SSE
    // message fans out to the `wf:event` subscribers — exactly
    // one fan-out target, regardless of the message's `kind`.
    //
    // For the QUOTA_NUDGE banner we DO want to filter by kind;
    // that's a separate listener the chair side subscribes to
    // directly under the `quota_nudge` event name. So we
    // register TWO listener targets per SSE message:
    //   - `wf:event`  (Tauri shell's channel; ChatZone uses this)
    //   - `quota_nudge` (if the payload's kind starts with QUOTA_NUDGE)
    if (!agentEvent || typeof agentEvent !== 'object') return;

    // Always fan-out to the wf:event channel (Tauri shell
    // channel name). ChatZone's useAgentStream listens here and
    // filters by kind.
    const wfSubs = eventSubscribers.get('wf:event');
    if (wfSubs) {
      for (const fn of wfSubs.values()) {
        try { fn({ event: 'wf:event', payload: agentEvent, id: -1 }); }
        catch (e) { console.error('event handler threw', e); }
      }
    }

    // Filtered channel for QUOTA_NUDGE banner: only match
    // events whose status starts with the marker.
    const kind = agentEvent.kind;
    if (kind === 'done' && typeof agentEvent.status === 'string'
        && agentEvent.status.startsWith('QUOTA_NUDGE:')) {
      const nudgeSubs = eventSubscribers.get('quota_nudge');
      if (nudgeSubs) {
        for (const fn of nudgeSubs.values()) {
          try { fn({ event: 'quota_nudge', payload: agentEvent, id: -1 }); }
          catch (e) { console.error('quota handler threw', e); }
        }
      }
    }
  }

  // SSE bridge: one EventSource for all events.
  let sse = null;
  function startSSE() {
    if (sse) return;
    sse = new EventSource(`${BRIDGE}/events`);
    sse.onmessage = (msg) => {
      try { fanoutEvent(JSON.parse(msg.data)); }
      catch (e) { console.warn('tauri-shim SSE parse failed', e); }
    };
    sse.onerror = () => {
      // EventSource auto-reconnects. Surface a one-time warning.
      console.warn('tauri-shim: SSE dropped; browser will retry');
    };
  }

  // ── Install __TAURI_INTERNALS__ shim ──────────────────────
  // Tauri 2.x's @tauri-apps/api calls these three hooks.
  let nextCallbackId = 1;
  const callbackMap = new Map(); // id -> fn (for transformCallback result)

  const internals = {
    // invoke(cmd, args, options) → Promise<result>
    // The 3rd `options` argument is the Tauri options object (e.g.
    // { headers: ... }). We ignore it on the bridge — the bridge
    // has no concept of per-call headers.
    invoke: async function (cmd, args, _options) {
      // Special case: plugin:event|* are handled in-process.
      if (cmd === 'plugin:event|listen') {
        const { event, handler } = args || {};
        const handlerId = handler; // transformCallback already ran
        if (!eventSubscribers.has(event)) eventSubscribers.set(event, new Map());
        // Store the actual JS callback. `handler` is an integer id
        // assigned by transformCallback — we look it up there.
        const fn = callbackMap.get(handlerId);
        eventSubscribers.get(event).set(handlerId, fn || (() => {}));
        // Return an unlisten function (Tauri listens return a Promise
        // resolving to an integer event id; we return an integer too).
        return nextCallbackId++;
      }
      if (cmd === 'plugin:event|unlisten') {
        const { event, eventId } = args || {};
        const subs = eventSubscribers.get(event);
        if (subs) subs.delete(eventId);
        return undefined;
      }
      if (cmd === 'plugin:event|emit') {
        // Browser-side emit: forward to SSE so other tabs (or this
        // tab) see it. We just resolve — most chatzone code doesn't
        // emit.
        return undefined;
      }
      // Generic cmd → RPC.
      const req = buildRequest(cmd, args);
      const r = await rpc(req.method, req.path, req.body);
      // Tauri invoke returns the result.body content (or the
      // entire result depending on the cmd). The desktop pipe
      // wrapper unwraps r.result.body for some cmds and returns
      // r.result for others. We default to returning r.body (the
      // pipe handler's response body), which matches what
      // pipe_request returns for almost every cmd in lib.rs.
      return r.body;
    },

    // transformCallback(cb, once=false) → integer id
    // Tauri core wraps a JS callback into an id that can be
    // passed across IPC. We just allocate an id and remember the
    // callback in a map; the actual callback runs in-process.
    transformCallback: function (cb, _once) {
      const id = nextCallbackId++;
      callbackMap.set(id, cb);
      return id;
    },

    // unregisterCallback(id) → void
    unregisterCallback: function (id) {
      callbackMap.delete(id);
    },
  };

  // Tauri v2 looks for `window.__TAURI_INTERNALS__` OR the
  // `__TAURI_INTERNALS__` global. Set both.
  window.__TAURI_INTERNALS__ = internals;

  // ── Health probe: warn loudly if bridge is unreachable ──
  fetch(`${BRIDGE}/health`).then((r) => {
    if (!r.ok) console.warn(`tauri-shim: /health returned ${r.status}`);
  }).catch((e) => {
    console.error(
      `tauri-shim: cannot reach bridge at ${BRIDGE} (${e.message}).\n` +
      `Start the runtime: cargo run -p pipe-server --bin flowntier-runtime`,
    );
  });

  startSSE();
})();