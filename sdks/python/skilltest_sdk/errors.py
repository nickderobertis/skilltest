"""Exceptions mirroring the CLI's exit-code contract.

The CLI distinguishes a *test failure* (exit 1 — surfaced as a [`Report`] with
``passed == False``, not an exception) from *bad input* (exit 2) and a *provider
failure* (exit 3). The latter two are environmental/usage problems the test
author must fix, so they are raised.

A provider failure additionally carries a **structured classification**
([`SkilltestProviderError.kind`]) parsed from the CLI's JSON error output, so
consumers can branch on the category (retry a ``"timeout"``, fail fast on
``"auth"``) instead of matching substrings in the message.
"""

from __future__ import annotations

from typing import Literal

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
    ``"api-judge"``). Branch on ``kind`` for targeted handling rather than
    parsing the message string.
    """

    def __init__(
        self,
        message: str,
        *,
        kind: ProviderErrorKind | None = None,
        context: str | None = None,
    ) -> None:
        super().__init__(message)
        self.kind: ProviderErrorKind | None = kind
        self.context: str | None = context
