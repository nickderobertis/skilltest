"""Exceptions mirroring the CLI's exit-code contract.

The CLI distinguishes a *test failure* (exit 1 — surfaced as a [`Report`] with
``passed == False``, not an exception) from *bad input* (exit 2) and a *provider
failure* (exit 3). The latter two are environmental/usage problems the test
author must fix, so they are raised.

A provider failure additionally carries a **structured classification**
([`SkilltestProviderError.kind`]) parsed from the CLI's JSON error output, and is
raised as the **kind-specific subclass** (e.g. [`SkilltestTimeoutError`]) so a
handler can catch one category directly. Every subclass extends
[`SkilltestProviderError`], so ``except SkilltestProviderError`` still catches
them all.

The kind→subclass registry is checked against the generated ``ProviderErrorKind``
vocabulary by a test (``test_provider_error_subclasses_cover_every_kind``), so a
kind added to the Rust enum fails the suite until its subclass exists here.
"""

from __future__ import annotations

from typing import ClassVar, Literal

#: The provider-failure categories skilltest can classify, mirroring the Rust
#: ``ProviderErrorKind`` and the wire vocabulary of the CLI's JSON error output.
#: Kept in step with the generated ``_error.ReportError`` model by a drift-guard
#: test in the SDK suite.
ProviderErrorKind = Literal[
    "auth",
    "rate_limit",
    "model_not_found",
    "quota",
    "overloaded",
    "timeout",
    "spawn",
    "protocol",
    "other",
]


class SkilltestError(Exception):
    """Base class for skilltest SDK errors."""


class SkilltestUsageError(SkilltestError):
    """The CLI rejected the input (exit 2): bad config, malformed YAML, etc."""


class SkilltestProviderError(SkilltestError):
    """The provider command failed (exit 3): not found, crashed, bad output.

    ``kind`` is the structured failure category (a [`ProviderErrorKind`], e.g.
    ``"timeout"`` or ``"auth"``) when skilltest could classify it, else ``None``
    (an older CLI, or a failure it could not classify). ``context`` is the
    provider the failure came from (e.g. ``"oneharness:claude-code"``,
    ``"api-judge"``).

    Prefer catching the kind-specific subclass ([`SkilltestTimeoutError`], …) for
    targeted handling; catch this base for any provider failure. An unclassified
    failure (``kind is None``) and the ``"other"`` catch-all both surface as this
    base type — the eight specific kinds each get their own subclass.
    """

    #: The kind this subclass represents; ``None`` on the base (unclassified /
    #: the ``"other"`` catch-all). Subclasses set it, which registers them.
    provider_kind: ClassVar[ProviderErrorKind | None] = None
    #: kind → subclass, populated as subclasses are defined.
    _registry: ClassVar[dict[ProviderErrorKind, type[SkilltestProviderError]]] = {}

    def __init_subclass__(cls, **kwargs: object) -> None:
        super().__init_subclass__(**kwargs)
        if cls.provider_kind is not None:
            SkilltestProviderError._registry[cls.provider_kind] = cls

    def __init__(
        self,
        message: str,
        *,
        kind: ProviderErrorKind | None = None,
        context: str | None = None,
    ) -> None:
        super().__init__(message)
        self.kind: ProviderErrorKind | None = kind if kind is not None else self.provider_kind
        self.context: str | None = context

    @classmethod
    def for_kind(
        cls,
        message: str,
        *,
        kind: ProviderErrorKind | None = None,
        context: str | None = None,
    ) -> SkilltestProviderError:
        """Construct the kind-specific subclass for ``kind`` (falling back to this
        base for ``None``, ``"other"``, or an unregistered kind)."""
        target = cls._registry.get(kind, cls) if kind is not None else cls
        return target(message, kind=kind, context=context)


class SkilltestAuthError(SkilltestProviderError):
    """Authentication/authorization failed (missing or rejected credentials)."""

    provider_kind = "auth"


class SkilltestRateLimitError(SkilltestProviderError):
    """The provider rate-limited the call; a backoff-and-retry may succeed."""

    provider_kind = "rate_limit"


class SkilltestModelNotFoundError(SkilltestProviderError):
    """The harness/vendor does not recognize the requested model."""

    provider_kind = "model_not_found"


class SkilltestQuotaError(SkilltestProviderError):
    """The account's quota or billing limit is exhausted."""

    provider_kind = "quota"


class SkilltestOverloadedError(SkilltestProviderError):
    """A transient server-side overload; retried internally where possible."""

    provider_kind = "overloaded"


class SkilltestTimeoutError(SkilltestProviderError):
    """The call exceeded its deadline (harness ``--timeout``, curl ``--max-time``)."""

    provider_kind = "timeout"


class SkilltestSpawnError(SkilltestProviderError):
    """The provider process could not be started (binary missing, not runnable)."""

    provider_kind = "spawn"


class SkilltestProtocolError(SkilltestProviderError):
    """The provider ran but produced output that violated the protocol."""

    provider_kind = "protocol"
