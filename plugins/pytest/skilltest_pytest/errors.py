"""Exceptions mirroring the CLI's exit-code contract.

The CLI distinguishes a *test failure* (exit 1 — surfaced as a [`Report`] with
``passed == False``, not an exception) from *bad input* (exit 2) and a *provider
failure* (exit 3). The latter two are environmental/usage problems the test
author must fix, so they are raised.
"""

from __future__ import annotations


class SkilltestError(Exception):
    """Base class for skilltest plugin errors."""


class SkilltestUsageError(SkilltestError):
    """The CLI rejected the input (exit 2): bad config, malformed YAML, etc."""


class SkilltestProviderError(SkilltestError):
    """The provider command failed (exit 3): not found, crashed, bad output."""
