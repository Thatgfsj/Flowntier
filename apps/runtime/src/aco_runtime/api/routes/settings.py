"""Settings REST API.

Endpoints (all under /api/settings):

* GET    /secrets              -- list account names (no values)
* GET    /secrets/{name}       -- get one masked value (last 4 chars only)
* PUT    /secrets/{name}       -- set (body: {"value": "..."})
* DELETE /secrets/{name}       -- remove

The runtime never returns the full plaintext secret over HTTP.
The list endpoint shows only account names; the get endpoint
shows a 4-char suffix. The full plaintext only lives in the
OS keychain and the runtime's process memory (after seed).

The keys endpoint:
* POST   /secrets/{name}/reveal -- one-shot full value (UI use only;
  not persisted in browser history)
"""
from __future__ import annotations

from typing import Any

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

# Import lazily so the runtime can boot even if keyring fails
# (e.g. Linux without a keyring service).
def _store() -> Any:
    from aco_runtime_lib.secrets import secrets_store
    return secrets_store


router = APIRouter()


class SecretIn(BaseModel):
    value: str = Field(min_length=1, max_length=4096)


class SecretListItem(BaseModel):
    name: str
    present: bool
    # Only the last 4 chars of the value are sent to the UI, to
    # give the user a hint without exposing the full key.
    masked: str | None = None


class SecretOut(BaseModel):
    name: str
    masked: str | None = None


@router.get("/secrets", response_model=list[SecretListItem])
def list_secrets() -> list[SecretListItem]:
    """List every known account with a hint of its value.

    Iterates over the SecretStore's `probe_accounts()` and reports
    presence + a 4-char suffix. Never returns the full plaintext.
    """
    s = _store()
    out: list[SecretListItem] = []
    for name in s.probe_accounts():
        v = s.get(name)
        if v is None:
            out.append(SecretListItem(name=name, present=False))
        else:
            suffix = v[-4:] if len(v) >= 4 else "*" * len(v)
            out.append(SecretListItem(name=name, present=True, masked=f"…{suffix}"))
    return out


@router.get("/secrets/{name}", response_model=SecretOut)
def get_secret(name: str) -> SecretOut:
    """Return a masked view of one secret."""
    s = _store()
    v = s.get(name)
    if v is None:
        raise HTTPException(status_code=404, detail="secret not set")
    suffix = v[-4:] if len(v) >= 4 else "*" * len(v)
    return SecretOut(name=name, masked=f"…{suffix}")


@router.put("/secrets/{name}", status_code=204)
def put_secret(name: str, body: SecretIn) -> None:
    """Set or replace a secret in the OS keychain."""
    s = _store()
    try:
        s.set(name, body.value)
    except Exception as exc:  # noqa: BLE001
        raise HTTPException(status_code=500, detail=f"keychain set failed: {exc}") from exc


@router.delete("/secrets/{name}", status_code=204)
def delete_secret(name: str) -> None:
    """Remove a secret from the OS keychain."""
    s = _store()
    if not s.delete(name):
        raise HTTPException(status_code=404, detail="secret not set")


@router.post("/secrets/{name}/reveal", response_model=dict[str, str])
def reveal_secret(name: str) -> dict[str, str]:
    """One-shot full plaintext reveal. The UI calls this only when
    the user explicitly clicks 'Show'. Returns the value in the
    response body; not cached.

    Security note: the plaintext value briefly traverses the HTTP
    channel on localhost. Same trust boundary as the Settings
    UI itself (which already runs locally). Documented for
    transparency; consider moving to Tauri IPC only in v0.3.
    """
    s = _store()
    v = s.get(name)
    if v is None:
        raise HTTPException(status_code=404, detail="secret not set")
    return {"name": name, "value": v}


@router.post("/secrets/seed", response_model=dict[str, list[str]])
def seed_now(overwrite: bool = False) -> dict[str, list[str]]:
    """Re-seed os.environ from the keychain. Returns the names
    that were set. Useful after the user adds a new key via the
    Settings UI and wants the runtime to pick it up without a
    restart."""
    s = _store()
    return {"seeded": s.seed_os_environ(overwrite=overwrite)}
