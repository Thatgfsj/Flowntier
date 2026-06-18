"""Model router: role → provider:model.

Reads a static config (TOML or in-memory dict) and picks a model
per role. Phase 1: no failover. Phase 2 adds retry + failover
chains per docs/PROVIDER_SPEC §6.

See `docs/PROVIDER_SPEC.md` §5.3 and §6.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from aco_runtime_lib.providers.base import Provider, ProviderError
from aco_runtime_lib.providers.mock import MockProvider


@dataclass(slots=True, frozen=True)
class ModelRef:
    """A reference to a specific provider:model."""

    provider_id: str
    model_id: str

    def __str__(self) -> str:
        return f"{self.provider_id}:{self.model_id}"

    @classmethod
    def parse(cls, ref: str) -> "ModelRef":
        if ":" not in ref:
            raise ValueError(f"invalid model ref: {ref!r} (expected 'provider:model')")
        provider_id, model_id = ref.split(":", 1)
        if not provider_id or not model_id:
            raise ValueError(f"invalid model ref: {ref!r}")
        return cls(provider_id=provider_id, model_id=model_id)


@dataclass(slots=True)
class RouterConfig:
    """Per-role default model. Mirrors `config/router.toml`."""

    defaults: dict[str, ModelRef]

    @classmethod
    def from_dict(cls, d: dict[str, str]) -> "RouterConfig":
        return cls(defaults={k: ModelRef.parse(v) for k, v in d.items()})


class ModelRouter:
    """Picks a provider+model for a given role.

    Phase 1: no failover. If the chosen provider raises a non-
    retryable error, the error propagates. Phase 2 will add the
    chain + retry policy from PROVIDER_SPEC §6.2.
    """

    def __init__(
        self,
        providers: dict[str, Provider],
        config: RouterConfig,
    ) -> None:
        self._providers = providers
        self._config = config

    def pick(self, role: str) -> tuple[Provider, ModelRef]:
        ref = self._config.defaults.get(role)
        if ref is None:
            raise ProviderError(f"no default model for role {role!r}", retryable=False)
        provider = self._providers.get(ref.provider_id)
        if provider is None:
            raise ProviderError(
                f"provider {ref.provider_id!r} is not enabled",
                retryable=False,
            )
        return provider, ref

    def register(self, provider_id: str, provider: Provider) -> None:
        """Add or replace a provider at runtime (Phase 2: hot-reload)."""
        self._providers[provider_id] = provider

    @property
    def available(self) -> list[str]:
        return sorted(self._providers.keys())


def default_router(mock_first: bool = True) -> ModelRouter:
    """Build a router for tests / local dev. All roles → MockProvider.

    Production routers are built from `config/router.toml` and the
    real provider implementations (see `apps/runtime/main.py` in
    Phase 1 wiring).
    """
    mock = MockProvider()
    providers: dict[str, Provider] = {"mock": mock}
    config = RouterConfig.from_dict(
        {
            "chief": "mock:mock-m3",
            "critic_a": "mock:mock-m3",
            "critic_b": "mock:mock-m3",
            "worker": "mock:mock-m3",
            "reporter": "mock:mock-m3",
        }
    )
    return ModelRouter(providers=providers, config=config)
