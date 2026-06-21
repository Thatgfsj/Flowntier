"""Windows named-pipe transport for the ACO runtime.

The Tauri desktop shell talks to the Python sidecar over two local
named pipes:

* ``\\\\.\\pipe\\aco_runtime``        — JSON-RPC 2.0 over newline-delimited
                                       JSON. One connection = one
                                       request-response. The server
                                       dispatches each call into the
                                       existing FastAPI route table
                                       (in-process), so all business
                                       logic, validation, and error
                                       shapes are unchanged.
* ``\\\\.\\pipe\\aco_runtime_events`` — long-lived connections; the
                                       server pushes every event from
                                       the shared ``EventBus`` as one
                                       JSON line per event.

This module is Windows-only at runtime. The import is gated on
``sys.platform == "win32"`` so the rest of the codebase stays
importable on macOS/Linux dev machines.
"""
from __future__ import annotations

import asyncio
import json
import sys
from typing import Any

from loguru import logger

if sys.platform != "win32":
    raise ImportError(
        "aco_runtime.pipe_server is Windows-only "
        "(uses win32pipe CreateNamedPipe)."
    )

import win32file  # type: ignore[import-not-found]
import win32pipe  # type: ignore[import-not-found]
import pywintypes  # type: ignore[import-not-found]

RPC_PIPE_NAME = r"\\.\pipe\aco_runtime"
EVENTS_PIPE_NAME = r"\\pipe\aco_runtime_events"  # prepended below
EVENTS_PIPE_NAME = r"\\.\pipe\aco_runtime_events"
MAX_LINE = 1_048_576  # 1 MiB hard cap per pipe message
PIPE_BUFFER = 64 * 1024


def _open_rpc_server_instance() -> Any:
    """Create one blocking win32pipe handle preconfigured for messages."""
    handle = win32pipe.CreateNamedPipe(
        RPC_PIPE_NAME,
        win32pipe.PIPE_ACCESS_DUPLEX
        | win32file.FILE_FLAG_OVERLAPPED,
        win32pipe.PIPE_TYPE_MESSAGE
        | win32pipe.PIPE_READMODE_MESSAGE
        | win32pipe.PIPE_WAIT,
        win32pipe.PIPE_UNLIMITED_INSTANCES,
        PIPE_BUFFER,
        PIPE_BUFFER,
        0,
        None,
    )
    return handle


def _open_events_server_instance() -> Any:
    return win32pipe.CreateNamedPipe(
        EVENTS_PIPE_NAME,
        win32pipe.PIPE_ACCESS_DUPLEX
        | win32file.FILE_FLAG_OVERLAPPED,
        win32pipe.PIPE_TYPE_MESSAGE
        | win32pipe.PIPE_READMODE_MESSAGE
        | win32pipe.PIPE_WAIT,
        win32pipe.PIPE_UNLIMITED_INSTANCES,
        PIPE_BUFFER,
        PIPE_BUFFER,
        0,
        None,
    )


# ── RPC dispatch (one connection = one request) ──────────────────


def _read_one_message(handle: Any) -> bytes:
    """Read exactly one message-mode pipe frame (returns the raw bytes)."""
    # Peek-message loop: pipe MESSAGE-mode preserves message boundaries,
    # so each ReadFile returns one whole message.
    _, data = win32file.ReadFile(handle, MAX_LINE)
    return bytes(data)


def _write_message(handle: Any, payload: bytes) -> None:
    win32file.WriteFile(handle, payload)


def _handle_rpc_sync(app, handle) -> None:
    """Serve exactly one RPC request on a freshly-accepted connection."""
    try:
        try:
            raw = _read_one_message(handle)
        except pywintypes.error as e:
            # ERROR_BROKEN_PIPE / ERROR_INVALID_HANDLE etc. on disconnect.
            logger.debug("rpc read err: {}", e)
            return

        try:
            req = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as e:
            err = {
                "jsonrpc": "2.0",
                "id": None,
                "error": {"code": -32700, "message": f"bad json: {e}"},
            }
            _write_message(handle, (json.dumps(err) + "\n").encode("utf-8"))
            return

        if not isinstance(req, dict) or "method" not in req:
            err = {
                "jsonrpc": "2.0",
                "id": req.get("id") if isinstance(req, dict) else None,
                "error": {"code": -32600, "message": "invalid request"},
            }
            _write_message(handle, (json.dumps(err) + "\n").encode("utf-8"))
            return

        id_ = req.get("id")
        method = req["method"]
        params = req.get("params") or {}
        path = params.get("path", "")
        body = params.get("body")

        try:
            status, resp_body = _dispatch_through_app(app, method, path, body)
        except Exception as e:  # noqa: BLE001
            err = {
                "jsonrpc": "2.0",
                "id": id_,
                "error": {"code": -32603, "message": f"dispatch: {e!r}"},
            }
            _write_message(handle, (json.dumps(err) + "\n").encode("utf-8"))
            return

        out = {
            "jsonrpc": "2.0",
            "id": id_,
            "result": {"status": status, "body": resp_body},
        }
        _write_message(handle, (json.dumps(out) + "\n").encode("utf-8"))
    finally:
        try:
            win32file.CloseHandle(handle)
        except pywintypes.error:
            pass


def _dispatch_through_app(app, method: str, path: str, body: Any):
    """Reuse the FastAPI route table as a regular function via the ASGI
    protocol (no uvicorn / no socket)."""
    path_only, _, query = path.partition("?")
    scope = {
        "type": "http",
        "asgi": {"version": "3.0", "spec_version": "2.0"},
        "method": method.upper(),
        "path": path_only,
        "raw_path": path_only.encode("utf-8"),
        "query_string": query.encode("utf-8"),
        "headers": [(b"host", b"pipe")],
        "server": ("pipe", 0),
        "client": ("pipe", 0),
        "scheme": "http",
    }
    if body is None:
        body_bytes = b""
    else:
        body_bytes = json.dumps(body).encode("utf-8")
    sent = {"status": 200, "body": bytearray()}

    async def receive():
        return {
            "type": "http.request",
            "body": body_bytes,
            "more_body": False,
        }

    async def send(msg):
        if msg["type"] == "http.response.start":
            sent["status"] = int(msg["status"])
        elif msg["type"] == "http.response.body":
            sent["body"].extend(msg.get("body", b""))

    # Run the ASGI app synchronously. We do not have a running event loop
    # on this thread (we're inside asyncio.to_thread), so we drive it
    # ourselves with asyncio.run_coroutine_threadsafe is overkill — just
    # create a fresh loop for the duration of the dispatch.
    loop = asyncio.new_event_loop()
    try:
        loop.run_until_complete(app(scope, receive, send))
    finally:
        loop.close()

    raw = bytes(sent["body"])
    if not raw:
        return sent["status"], None
    try:
        return sent["status"], json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        return sent["status"], raw.decode("utf-8", errors="replace")


async def _serve_rpc(app) -> None:
    """Pre-spawn N pipe-server instances that each accept in parallel.

    Why not a single accept loop? Because Windows named pipes are
    per-instance: a new client cannot connect to a pipe instance that
    is still busy with the previous client. A single accept loop
    processes requests serially and rejects any concurrent client
    (`ERROR_FILE_NOT_FOUND`). We spawn one thread per instance so N
    clients can hit us in parallel.
    """
    import threading

    n_workers = 16

    def worker() -> None:
        while True:
            handle = _open_rpc_server_instance()
            try:
                win32pipe.ConnectNamedPipe(handle, None)
            except pywintypes.error as e:
                # Client disconnected before we accepted; loop and try again.
                logger.debug("rpc connect err: {}; reopening", e)
                try:
                    win32file.CloseHandle(handle)
                except pywintypes.error:
                    pass
                continue
            try:
                _handle_rpc_sync(app, handle)
            except Exception as e:  # noqa: BLE001
                logger.exception("rpc handler crashed: {}", e)
                try:
                    win32file.CloseHandle(handle)
                except pywintypes.error:
                    pass

    logger.info(
        "rpc pipe listening: {} ({} accept workers)", RPC_PIPE_NAME, n_workers
    )
    threads = [threading.Thread(target=worker, daemon=True, name="rpc-accept")
               for _ in range(n_workers)]
    for t in threads:
        t.start()
    # Yield forever; workers handle everything.
    import asyncio as _asyncio

    await _asyncio.Event().wait()


# ── Events (long-lived connections) ──────────────────────────────


def _events_loop_for_client(handle, bus, loop: asyncio.AbstractEventLoop) -> None:
    """Drain the bus and write one JSON line per event to the client.
    Detects client disconnect when ReadFile returns empty/broken-pipe.
    """
    queue: asyncio.Queue = asyncio.run_coroutine_threadsafe(
        bus.subscribe(), loop
    ).result()

    closed = False

    def pump_to_pipe():
        nonlocal closed
        while not closed:
            ev = asyncio.run_coroutine_threadsafe(queue.get(), loop).result()
            line = (json.dumps({"event": ev}) + "\n").encode("utf-8")
            try:
                win32file.WriteFile(handle, line)
            except pywintypes.error:
                closed = True
                return

    pump_thread = None
    try:
        pump_thread = _start_daemon(target=pump_to_pipe)
        # Block on a read so we detect EOF when the client disconnects.
        while True:
            try:
                _, _ = win32file.ReadFile(handle, 1)
            except pywintypes.error:
                break
    finally:
        closed = True
        if pump_thread is not None:
            pump_thread.join(timeout=0.5)
        try:
            asyncio.run_coroutine_threadsafe(bus.unsubscribe(queue), loop).result(
                timeout=1.0
            )
        except Exception:
            pass
        try:
            win32file.CloseHandle(handle)
        except pywintypes.error:
            pass


def _start_daemon(target):
    import threading

    t = threading.Thread(target=target, daemon=True)
    t.start()
    return t


async def _serve_events(bus) -> None:
    """Same N-worker model as _serve_rpc. Each accepted events client
    gets its own long-lived pipe instance and its own bus subscription."""
    import threading
    import asyncio as _asyncio

    loop = _asyncio.get_running_loop()  # capture NOW, before thread starts
    n_workers = 4

    def worker() -> None:
        while True:
            handle = _open_events_server_instance()
            try:
                win32pipe.ConnectNamedPipe(handle, None)
            except pywintypes.error as e:
                logger.debug("events connect err: {}; reopening", e)
                try:
                    win32file.CloseHandle(handle)
                except pywintypes.error:
                    pass
                continue
            try:
                _events_loop_for_client(handle, bus, loop)
            except Exception as e:  # noqa: BLE001
                logger.exception("events handler crashed: {}", e)
                try:
                    win32file.CloseHandle(handle)
                except pywintypes.error:
                    pass

    logger.info(
        "events pipe listening: {} ({} accept workers)",
        EVENTS_PIPE_NAME,
        n_workers,
    )
    threads = [threading.Thread(target=worker, daemon=True, name="events-accept")
               for _ in range(n_workers)]
    for t in threads:
        t.start()
    await _asyncio.Event().wait()


# ── Entry point ──────────────────────────────────────────────────


async def serve(app, bus) -> None:
    """Start the RPC + events pipe servers. Await forever."""
    await asyncio.gather(_serve_rpc(app), _serve_events(bus))
