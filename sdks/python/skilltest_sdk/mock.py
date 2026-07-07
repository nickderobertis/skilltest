"""Tool mocking and spying: hold a mock/spy as a variable, pass it to
[`run_skill`][skilltest_sdk.runner.run_skill], and assert on it directly —
`vi.fn()`/`unittest.mock` semantics over real harness tool calls.

```python
from skilltest_sdk import run_skill, spy, stub, deny, contains, matching

git = spy(tool="bash", pattern=r"\\bgit\\b")                 # observe only
push = stub(contains="git push", output="Everything up-to-date")

report = run_skill("cases/deploy.skilltest.yaml", mocks=[push, git])

push.assert_called_once()
git.assert_called_with(command=contains("git status"))
git.where(command=matching(r"\\bsudo\\b")).assert_not_called()
```

Vocabulary (sinon's): a **spy** observes without intercepting; a **stub** fakes
a shell call's result; **deny** blocks with a model-visible message; and
**rewrite** substitutes raw input fields. [`ToolMock`][skilltest_sdk.mock.ToolMock]
extends [`ToolSpy`][skilltest_sdk.mock.ToolSpy], so every mock carries the full
spy assertion surface.

Two matching engines, on purpose: a **mock's** criteria compile into the
hook-side ruleset that runs inside the harness (`pattern` is Rust-regex — no
lookarounds — and predicates must be expressible as `equals`/`contains`/
`pattern`), while a **spy** filters the returned records locally, so its
`pattern` uses Python's own `re` and `where()` accepts arbitrary callables.

These are deliberately *not* `unittest.mock.Mock` objects: tool calls are
structured records (tool, input, platform/model, verdict), the useful
assertions are partial/fuzzy rather than exact-args, and `Mock`'s
attribute-swallowing would let a typo'd assertion pass vacuously. A misspelled
method here is an `AttributeError`, and reading an unbound spy raises instead
of counting as zero calls.
"""

from __future__ import annotations

import json
import re
from typing import TYPE_CHECKING, Any

from pydantic import BaseModel

from .errors import SkilltestUsageError

if TYPE_CHECKING:
    from collections.abc import Callable, Sequence

    from ._report import MockCall

__all__ = [
    "Matcher",
    "ToolCall",
    "ToolMock",
    "ToolSpy",
    "anything",
    "contains",
    "deny",
    "matching",
    "rewrite",
    "spy",
    "stub",
]


# ---------------------------------------------------------------------------
# Field matchers
# ---------------------------------------------------------------------------


class Matcher:
    """A predicate on one input-field value. Use the factory functions
    ([`contains`][skilltest_sdk.mock.contains],
    [`matching`][skilltest_sdk.mock.matching],
    [`anything`][skilltest_sdk.mock.anything]) rather than instantiating.

    ``compiled`` is the hook-side predicate this matcher can be lowered to for
    a *mock's* criteria (`None` when it cannot cross that boundary)."""

    def __init__(
        self,
        check: Callable[[str], bool],
        description: str,
        compiled: dict[str, str] | None = None,
    ) -> None:
        self._check = check
        self.description = description
        self.compiled = compiled

    def matches(self, value: str) -> bool:
        return self._check(value)

    def __repr__(self) -> str:  # pragma: no cover - repr sugar
        return self.description


def contains(needle: str) -> Matcher:
    """Match a field whose value contains ``needle``."""
    if not needle:
        raise SkilltestUsageError("contains() needs a non-empty needle (empty matches everything)")
    return Matcher(lambda value: needle in value, f"contains({needle!r})", {"contains": needle})


def matching(pattern: str | re.Pattern[str]) -> Matcher:
    """Match a field whose value matches ``pattern`` (unanchored). In a spy
    (local matching) this is Python's `re`; compiled into a mock it runs as
    Rust-regex inside the harness — stick to the shared subset (no
    lookarounds) for criteria used both ways."""
    compiled = re.compile(pattern)
    if not compiled.pattern:
        raise SkilltestUsageError("matching() needs a non-empty pattern (empty matches everything)")
    return Matcher(
        lambda value: compiled.search(value) is not None,
        f"matching({compiled.pattern!r})",
        {"pattern": compiled.pattern},
    )


def anything() -> Matcher:
    """Match any value — asserts the field merely *exists* on the call. Spy
    (local) matching only; the hook-side ruleset has no exists-predicate."""
    return Matcher(lambda _value: True, "anything()")


#: What a `where()` / factory criterion value may be: an exact scalar, a
#: Matcher, or an arbitrary predicate on the field's string form.
Criterion = Any


def _coerce_field(value: Any) -> str:
    """A field's string form: the raw string, or compact JSON for anything
    else — the same coercion the hook-side rules apply."""
    if isinstance(value, str):
        return value
    return json.dumps(value, separators=(",", ":"))


def _criterion_holds(criterion: Criterion, value: Any) -> bool:
    text = _coerce_field(value)
    if isinstance(criterion, Matcher):
        return criterion.matches(text)
    if callable(criterion):
        return bool(criterion(text))
    if isinstance(criterion, str):
        return text == criterion
    # A non-string scalar (int/float/bool/None) compares by its JSON form,
    # mirroring the coercion applied to the field itself.
    return text == json.dumps(criterion, separators=(",", ":"))


def _describe_criteria(criteria: dict[str, Criterion]) -> str:
    parts = [f"{key}={value!r}" for key, value in criteria.items()]
    return ", ".join(parts)


# ---------------------------------------------------------------------------
# Calls and the spy/mock objects
# ---------------------------------------------------------------------------


class ToolCall(BaseModel):
    """One observed tool call, with its **original** (pre-rewrite) input and
    the run it belongs to. The transcript's `events` show post-rewrite reality
    (the stub that actually ran); this shows what the skill *attempted*."""

    tool: str | None = None
    #: The tool's structured input arguments (harness-specific shape).
    input: Any = None
    #: The verdict applied: `allow`, `deny`, `rewrite`, or `stub`.
    action: str
    #: Name of the intercepting mock, when one did.
    mock: str | None = None
    platform: str
    model: str

    @property
    def mocked(self) -> bool:
        """True iff a mock's verdict was applied to this call."""
        return self.action != "allow"

    @property
    def command(self) -> str | None:
        """Sugar for the shell 90%-case: ``input["command"]`` when present."""
        if isinstance(self.input, dict):
            command = self.input.get("command")
            if isinstance(command, str):
                return command
        return None

    def _summary(self) -> str:
        detail = self.command if self.command is not None else _coerce_field(self.input)
        base = f"{self.tool or '?'}({detail})"
        return f"{base} [{self.action}]" if self.mocked else base


class ToolSpy:
    """A spy on tool calls: observes every matching call without intercepting.
    Construct with [`spy`][skilltest_sdk.mock.spy], pass it in ``mocks=`` to
    [`run_skill`][skilltest_sdk.runner.run_skill], then assert. Unbound access
    raises — "no run yet" must never read as "zero calls"."""

    def __init__(
        self,
        *,
        tool: str | None = None,
        contains: str | None = None,
        pattern: str | re.Pattern[str] | None = None,
        where: dict[str, Criterion] | None = None,
        name: str | None = None,
    ) -> None:
        if tool is None and contains is None and pattern is None and not where:
            raise SkilltestUsageError(
                "a spy/mock needs at least one criterion (tool=, contains=, pattern=, where=)"
            )
        for field, value in (("tool", tool), ("contains", contains)):
            if value == "":
                raise SkilltestUsageError(f"empty `{field}` would match everything")
        self._tool = tool
        self._contains = contains
        self._pattern: re.Pattern[str] | None = re.compile(pattern) if pattern is not None else None
        if self._pattern is not None and not self._pattern.pattern:
            raise SkilltestUsageError("empty `pattern` would match everything")
        self._where: dict[str, Criterion] = dict(where or {})
        #: A caller-visible name, so a case's `called`/`not_called` eval can
        #: reference this mock/spy (see `TestCase`). `None` = auto-named.
        self._user_name = name
        self._calls: list[ToolCall] | None = None
        #: For a `where()` view: the parent's calls, shown in failure messages
        #: when the filtered result is empty (so a miss is diagnosable).
        self._pool: list[ToolCall] | None = None

    # -- binding ------------------------------------------------------------

    def _bind(self, calls: Sequence[ToolCall]) -> None:
        """(Internal) Attach a run's matching calls; called by the runner."""
        self._calls = [c for c in calls if self._matches(c)]

    def _matches(self, call: ToolCall) -> bool:
        if self._tool is not None and (
            call.tool is None or call.tool.lower() != self._tool.lower()
        ):
            return False
        if self._contains is not None or self._pattern is not None:
            # The same haystack the hook-side rules match: the compact event
            # JSON carrying the tool name and its input.
            event: dict[str, Any] = {}
            if call.tool is not None:
                event["tool_name"] = call.tool
            if call.input is not None:
                event["tool_input"] = call.input
            haystack = json.dumps(event, separators=(",", ":"))
            if self._contains is not None and self._contains not in haystack:
                return False
            if self._pattern is not None and self._pattern.search(haystack) is None:
                return False
        return _where_holds(self._where, call)

    # -- observations ---------------------------------------------------------

    @property
    def calls(self) -> list[ToolCall]:
        """The matching observed calls, in order. Raises when unbound."""
        if self._calls is None:
            raise SkilltestUsageError(
                "this spy/mock is not bound to a run yet — pass it to run_skill(mocks=[...]) "
                "and read it after the run completes"
            )
        return list(self._calls)

    @property
    def called(self) -> bool:
        """True iff at least one matching call was observed."""
        return bool(self.calls)

    @property
    def call_count(self) -> int:
        return len(self.calls)

    def where(self, **criteria: Criterion) -> ToolSpy:
        """A filtered view with the identical assertion surface. Keyword names
        target input fields (values: exact scalar, a matcher like
        [`contains`][skilltest_sdk.mock.contains]/[`matching`][skilltest_sdk.mock.matching],
        or any predicate); the reserved ``tool=`` targets the tool name. An
        empty result is a full citizen — `.assert_not_called()` composes."""
        view = ToolSpy.__new__(ToolSpy)
        view._tool = None
        view._contains = None
        view._pattern = None
        view._where = {}
        view._user_name = None
        view._calls = [c for c in self.calls if _view_criteria_hold(criteria, c)]
        view._pool = self.calls
        return view

    # -- case compilation -----------------------------------------------------

    def _match_spec(self) -> dict[str, Any]:
        """(Internal) The hook-side `match` object for this spy/mock's criteria,
        shared by a mock's declaration and a named spy's case declaration."""
        match: dict[str, Any] = {}
        if self._tool is not None:
            match["tool"] = self._tool
        if self._contains is not None:
            match["contains"] = self._contains
        if self._pattern is not None:
            match["pattern"] = self._pattern.pattern
        if self._where:
            match["input"] = {
                key: _compile_criterion(key, value) for key, value in self._where.items()
            }
        return match

    def _case_decl(self, name: str) -> dict[str, Any]:
        """(Internal) A named spy's no-action declaration for a case's `mocks:`
        block, so a `called`/`not_called` eval can reference it by `name`."""
        return {"name": name, "match": self._match_spec()}

    # -- assertions -----------------------------------------------------------

    def assert_called(self) -> None:
        """Assert at least one matching call was observed."""
        if not self.called:
            raise AssertionError(f"expected at least one call, got none{self._observed()}")

    def assert_called_once(self) -> None:
        """Assert exactly one matching call was observed."""
        self.assert_called_times(1)

    def assert_called_times(self, times: int) -> None:
        """Assert exactly ``times`` matching calls were observed."""
        if self.call_count != times:
            raise AssertionError(
                f"expected exactly {times} call(s), got {self.call_count}{self._observed()}"
            )

    def assert_not_called(self) -> None:
        """Assert no matching call was observed."""
        if self.called:
            raise AssertionError(f"expected no calls, got {self.call_count}{self._observed()}")

    def assert_called_with(self, **criteria: Criterion) -> None:
        """Assert **some** matching call satisfies ``criteria`` (any-call
        semantics — deliberately not `unittest.mock`'s check-the-last-call,
        which is the wrong default against harness transcripts)."""
        if not self.where(**criteria).called:
            raise AssertionError(
                f"no call matched {_describe_criteria(criteria)}{self._observed()}"
            )

    def _observed(self) -> str:
        """The actual calls, for failure messages — a miss is diagnosable from
        the assertion output alone. A filtered view that matched nothing falls
        back to its parent's calls (what *was* observed)."""
        records = self._calls or self._pool or []
        if not records:
            return ""
        summaries = ", ".join(c._summary() for c in records[:5])
        more = f", … {len(records) - 5} more" if len(records) > 5 else ""
        return f"; observed: {summaries}{more}"


def _where_holds(criteria: dict[str, Criterion], call: ToolCall) -> bool:
    for key, criterion in criteria.items():
        if not isinstance(call.input, dict) or key not in call.input:
            return False
        if not _criterion_holds(criterion, call.input[key]):
            return False
    return True


def _view_criteria_hold(criteria: dict[str, Criterion], call: ToolCall) -> bool:
    """`where()` criteria: input fields, plus the reserved `tool` name."""
    plain = dict(criteria)
    tool = plain.pop("tool", None)
    if tool is not None and (call.tool is None or not _criterion_holds(tool, call.tool)):
        return False
    return _where_holds(plain, call)


class ToolMock(ToolSpy):
    """A mock: a spy that also **intercepts** — its criteria compile into the
    hook-side ruleset, so matching happens inside the harness. Construct with
    [`stub`][skilltest_sdk.mock.stub] / [`deny`][skilltest_sdk.mock.deny] /
    [`rewrite`][skilltest_sdk.mock.rewrite]."""

    def __init__(
        self,
        action: dict[str, Any],
        *,
        tool: str | None = None,
        contains: str | None = None,
        pattern: str | re.Pattern[str] | None = None,
        where: dict[str, Criterion] | None = None,
        name: str | None = None,
    ) -> None:
        super().__init__(tool=tool, contains=contains, pattern=pattern, where=where, name=name)
        self._action = action
        #: Synthetic declaration name, assigned when compiled into a run.
        self._name: str | None = None

    def _decl(self, name: str) -> dict[str, Any]:
        """(Internal) The declaration this compiles to in the `--mocks` file."""
        self._name = name
        return {"name": name, "match": self._match_spec(), **self._action}

    def _bind(self, calls: Sequence[ToolCall]) -> None:
        # A mock binds the calls its own rule intercepted (by resolved name),
        # not a local re-match — the hook's decision is the truth.
        self._calls = [c for c in calls if c.mock is not None and c.mock == self._name]


def _compile_criterion(key: str, criterion: Criterion) -> dict[str, Any] | str:
    """Compile a factory `where=` criterion into the hook-side predicate shape.
    Only exact strings and the shipped matchers can cross the boundary —
    arbitrary predicates cannot run inside the harness, so they are a loud
    construction-time error (use a spy for those)."""
    if isinstance(criterion, str):
        return criterion
    if isinstance(criterion, Matcher) and criterion.compiled is not None:
        return criterion.compiled
    raise SkilltestUsageError(
        f"mock criterion `{key}` cannot be compiled into the hook-side ruleset "
        f"(got {criterion!r}); mocks accept exact strings, contains(), or matching() — "
        "use a spy for arbitrary predicates"
    )


# ---------------------------------------------------------------------------
# Factories
# ---------------------------------------------------------------------------


def spy(
    *,
    tool: str | None = None,
    contains: str | None = None,
    pattern: str | re.Pattern[str] | None = None,
    where: dict[str, Criterion] | None = None,
    name: str | None = None,
) -> ToolSpy:
    """A spy: observe every matching tool call, intercept nothing.

    ``tool`` matches the (per-harness) tool name case-insensitively;
    ``contains``/``pattern`` match over the call's event JSON; ``where`` gives
    per-input-field criteria. Spies filter locally, so ``pattern`` is native
    Python `re` and ``where`` values may be arbitrary predicates.

    Give a ``name`` to reference this spy from a case's
    [`called`][skilltest_sdk.case.called] / [`not_called`][skilltest_sdk.case.not_called]
    eval; a named spy's criteria must be hook-expressible (no
    [`anything`][skilltest_sdk.mock.anything] or Python predicates).
    """
    return ToolSpy(tool=tool, contains=contains, pattern=pattern, where=where, name=name)


def stub(
    contains: str | None = None,
    *,
    output: str,
    exit_code: int = 0,
    tool: str | None = None,
    pattern: str | re.Pattern[str] | None = None,
    where: dict[str, Criterion] | None = None,
    name: str | None = None,
) -> ToolMock:
    """Fake a matching SHELL call's result: the real command never runs and
    the model receives ``output`` as the tool's genuine result. The positional
    argument is the ``contains`` matcher (the common case). ``pattern`` is
    Rust-regex (linear-time; no lookarounds) — it runs inside the harness.
    A ``name`` lets a case's `called`/`not_called` eval reference this mock."""
    return ToolMock(
        {"stub": {"output": output, "exit_code": exit_code}},
        tool=tool,
        contains=contains,
        pattern=pattern,
        where=where,
        name=name,
    )


def deny(
    contains: str | None = None,
    *,
    message: str,
    tool: str | None = None,
    pattern: str | re.Pattern[str] | None = None,
    where: dict[str, Criterion] | None = None,
    name: str | None = None,
) -> ToolMock:
    """Block a matching call; the model reads ``message`` as the tool's
    feedback. Works on every hook-capable harness (the most portable verb).
    A ``name`` lets a case's `called`/`not_called` eval reference this mock."""
    return ToolMock(
        {"deny": message},
        tool=tool,
        contains=contains,
        pattern=pattern,
        where=where,
        name=name,
    )


def rewrite(
    contains: str | None = None,
    *,
    input: dict[str, Any],
    tool: str | None = None,
    pattern: str | re.Pattern[str] | None = None,
    where: dict[str, Criterion] | None = None,
    name: str | None = None,
) -> ToolMock:
    """Substitute a matching call's raw input fields — the low-level escape
    hatch, and the way to mock file reads (rewrite ``file_path`` to a
    fixture). ``input`` is the substituted arguments object.
    A ``name`` lets a case's `called`/`not_called` eval reference this mock."""
    return ToolMock(
        {"rewrite": input},
        tool=tool,
        contains=contains,
        pattern=pattern,
        where=where,
        name=name,
    )


# ---------------------------------------------------------------------------
# Run integration (used by runner.run_skill / stream.stream_skill)
# ---------------------------------------------------------------------------


def compile_decls(mocks: Sequence[ToolSpy]) -> list[dict[str, Any]]:
    """(Internal) The `--mocks` declaration list for the run: one entry per
    [`ToolMock`][skilltest_sdk.mock.ToolMock] with a synthetic name; spies
    contribute nothing (they filter locally) but still require the channel."""
    decls = []
    for index, mock in enumerate(mocks):
        if isinstance(mock, ToolMock):
            decls.append(mock._decl(f"__mock_{index}"))
    return decls


def bind_mocks(mocks: Sequence[ToolSpy], runs: Sequence[Any]) -> None:
    """(Internal) Bind every spy/mock to the report's records. Loud when a run
    carries no channel — a spy must never silently read as zero calls."""
    calls: list[ToolCall] = []
    for run in runs:
        records: list[MockCall] | None = run.mock_calls
        if records is None:
            raise SkilltestUsageError(
                f"run `{run.case}` [{run.platform}/{run.model}] reported no mock/spy "
                "observations; the provider/platform does not support the mock channel"
            )
        calls.extend(
            ToolCall(
                tool=record.tool,
                input=record.input,
                action=record.action,
                mock=record.mock,
                platform=run.platform,
                model=run.model,
            )
            for record in records
        )
    for mock in mocks:
        mock._bind(calls)
