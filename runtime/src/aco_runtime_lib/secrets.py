"""Encrypted secret store, backed by the OS keychain.

Wraps the `keyring` library to provide a uniform get/set/delete
interface for the ACO Settings UI and the runtime's
ProviderManager. The actual storage:

* Windows  -> Windows Credential Manager (DPAPI)
* macOS    -> Keychain
* Linux    -> Secret Service (gnome-keyring, KWallet, ...)

Secrets are addressed by (service, account). For ACO we use
service="AgentCompanyOS" and account=<env_var_name>, e.g.
`("AgentCompanyOS", "MINIMAX_API_KEY")`.
"""
from __future__ import annotations

import os

import keyring
from keyring.errors import KeyringError, PasswordDeleteError

SERVICE = "AgentCompanyOS"


class SecretStore:
    """Thin wrapper around `keyring` with a known service prefix."""

    def __init__(self, service: str = SERVICE) -> None:
        self._service = service

    @property
    def service(self) -> str:
        return self._service

    def set(self, account: str, value: str) -> None:
        keyring.set_password(self._service, account, value)

    def get(self, account: str) -> str | None:
        try:
            return keyring.get_password(self._service, account)
        except KeyringError:
            return None

    def delete(self, account: str) -> bool:
        try:
            keyring.delete_password(self._service, account)
            return True
        except PasswordDeleteError:
            return False
        except KeyringError:
            return False

    def list_accounts(self) -> list[str]:
        return [a for a in self.probe_accounts() if self.get(a) is not None]

    def probe_accounts(self) -> list[str]:
        return [
            "MINIMAX_API_KEY",
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "GOOGLE_API_KEY",
            "DEEPSEEK_API_KEY",
            "DASHSCOPE_API_KEY",
            "OPENROUTER_API_KEY",
            "REPLICATE_API_TOKEN",
            "JIMENG_ACCESS_KEY_ID",
            "JIMENG_SECRET_ACCESS_KEY",
            "ARK_API_KEY",
            "GITHUB_TOKEN",
            "GITHUB_PAT",
            "PYPI_TOKEN",
        ]

    def seed_os_environ(self, overwrite: bool = False) -> list[str]:
        set_names: list[str] = []
        for account in self.list_accounts():
            value = self.get(account)
            if value is None:
                continue
            if not overwrite and account in os.environ:
                continue
            os.environ[account] = value
            set_names.append(account)
        return set_names


secrets_store = SecretStore()


__all__ = ["SERVICE", "SecretStore", "secrets_store"]
