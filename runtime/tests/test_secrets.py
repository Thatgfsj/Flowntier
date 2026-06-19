"""Tests for `aco_runtime_lib.secrets`.

Uses a stub keyring backend so the tests don't touch the real
Windows Credential Manager / macOS Keychain / Linux Secret Service.
"""
from __future__ import annotations

import os
from typing import Any

import pytest
from keyring.errors import PasswordDeleteError


class _StubKeyring:
    """In-memory replacement for `keyring` module -- mimics the
    `set_password / get_password / delete_password` API used by
    SecretStore."""

    def __init__(self) -> None:
        self.store: dict[tuple[str, str], str] = {}

    def set_password(self, service: str, account: str, value: str) -> None:
        self.store[(service, account)] = value

    def get_password(self, service: str, account: str) -> str | None:
        return self.store.get((service, account))

    def delete_password(self, service: str, account: str) -> None:
        if (service, account) not in self.store:
            raise PasswordDeleteError(f"no such entry: ({service!r}, {account!r})")
        del self.store[(service, account)]


@pytest.fixture(autouse=True)
def _patched_keyring(monkeypatch: pytest.MonkeyPatch) -> _StubKeyring:
    stub = _StubKeyring()
    import keyring
    monkeypatch.setattr(keyring, "set_password", stub.set_password)
    monkeypatch.setattr(keyring, "get_password", stub.get_password)
    monkeypatch.setattr(keyring, "delete_password", stub.delete_password)

    for var in [
        "MINIMAX_API_KEY",
        "OPENAI_API_KEY",
        "GITHUB_TOKEN",
    ]:
        monkeypatch.delenv(var, raising=False)
    return stub


def _new_store(service: str = "test-service") -> Any:
    from aco_runtime_lib.secrets import SecretStore
    return SecretStore(service=service)


def test_set_and_get_roundtrip(_patched_keyring: _StubKeyring) -> None:
    s = _new_store()
    assert s.get("FOO") is None
    s.set("FOO", "bar")
    assert s.get("FOO") == "bar"


def test_set_overwrites(_patched_keyring: _StubKeyring) -> None:
    s = _new_store()
    s.set("FOO", "first")
    s.set("FOO", "second")
    assert s.get("FOO") == "second"


def test_delete_returns_true_when_present(_patched_keyring: _StubKeyring) -> None:
    s = _new_store()
    s.set("FOO", "bar")
    assert s.delete("FOO") is True
    assert s.get("FOO") is None


def test_delete_returns_false_when_absent(_patched_keyring: _StubKeyring) -> None:
    s = _new_store()
    assert s.delete("NEVER_SET") is False


def test_list_accounts_returns_only_present(
    _patched_keyring: _StubKeyring,
) -> None:
    s = _new_store()
    s.set("MINIMAX_API_KEY", "x")
    s.set("OPENAI_API_KEY", "y")
    accounts = s.list_accounts()
    assert "MINIMAX_API_KEY" in accounts
    assert "OPENAI_API_KEY" in accounts
    assert "DEEPSEEK_API_KEY" not in accounts


def test_seed_os_environ_sets_unset_vars(
    _patched_keyring: _StubKeyring,
) -> None:
    s = _new_store()
    s.set("MINIMAX_API_KEY", "sk-xyz")
    s.set("GITHUB_TOKEN", "ghp-abc")
    seeded = s.seed_os_environ()
    assert "MINIMAX_API_KEY" in seeded
    assert "GITHUB_TOKEN" in seeded
    assert os.environ["MINIMAX_API_KEY"] == "sk-xyz"
    assert os.environ["GITHUB_TOKEN"] == "ghp-abc"


def test_seed_os_environ_skips_already_set(
    monkeypatch: pytest.MonkeyPatch,
    _patched_keyring: _StubKeyring,
) -> None:
    monkeypatch.setenv("MINIMAX_API_KEY", "from-setx")
    s = _new_store()
    s.set("MINIMAX_API_KEY", "from-keychain")
    s.seed_os_environ()
    assert os.environ["MINIMAX_API_KEY"] == "from-setx"


def test_seed_os_environ_overwrite_true(
    monkeypatch: pytest.MonkeyPatch,
    _patched_keyring: _StubKeyring,
) -> None:
    monkeypatch.setenv("MINIMAX_API_KEY", "from-setx")
    s = _new_store()
    s.set("MINIMAX_API_KEY", "from-keychain")
    s.seed_os_environ(overwrite=True)
    assert os.environ["MINIMAX_API_KEY"] == "from-keychain"


def test_probe_accounts_known_names() -> None:
    s = _new_store()
    probed = s.probe_accounts()
    for required in [
        "MINIMAX_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "GOOGLE_API_KEY",
        "DEEPSEEK_API_KEY",
        "GITHUB_TOKEN",
        "PYPI_TOKEN",
    ]:
        assert required in probed, f"{required} missing from probe list"
