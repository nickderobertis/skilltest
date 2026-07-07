"""Define a full test case in Python — everything a `*.skilltest.yaml` file
carries — and hand it straight to [`run_skill`][skilltest_sdk.runner.run_skill].

```python
from skilltest_sdk import TestCase, run_skill, boolean, numeric, user

case = TestCase(
    skill="skills/greeter",
    input="Greet Dr. Smith, who has an appointment today.",
    evals=[
        boolean("the reply greets Dr. Smith by name"),
        numeric("how warm is the tone", min=0, max=10, threshold=7),
    ],
)

report = run_skill(case)
assert report.passed
```

A `TestCase` is the code twin of the YAML schema (`docs/schema.md`): `skill`
resolves relative to the working directory, `input` is the first user message,
`user` turns the case multi-turn, `mocks` are the same
[`spy`][skilltest_sdk.mock.spy] / [`stub`][skilltest_sdk.mock.stub] /
[`deny`][skilltest_sdk.mock.deny] / [`rewrite`][skilltest_sdk.mock.rewrite]
objects you would pass to ``run_skill(mocks=...)`` (bound for assertions after
the run, and referenceable by name from a `called`/`not_called` eval), and
`evals` decide pass/fail.

The builders construct models **generated from the CLI's own input schema**
(`schemas/case.schema.json` → ``_case.py``, via ``just gen-contract``), so the
payload shape cannot drift from the Rust parse: a renamed or removed field is a
type error here, not a silently-ignored key at runtime. The CLI then validates
the compiled case semantically, so a malformed one is a loud
[`SkilltestUsageError`][skilltest_sdk.errors.SkilltestUsageError], never a
vacuous pass.

Writing cases in code is the recommended approach — it keeps the case, its
mocks, and any deterministic transcript checks in one typed place. YAML files
remain fully supported (`run_skill("cases/greet.yaml")`) and are what the
plugins auto-discover; see the package README.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING, Any, Literal

from ._case import (
    BooleanEval,
    CalledEval,
    FieldPredicateSpec,
    MockDecl,
    NotCalledEval,
    NumericEval,
    SimulatedUser,
)
from ._case import (
    TestCase as _CaseModel,
)
from .errors import SkilltestUsageError
from .mock import Criterion, Matcher, ToolMock, ToolSpy

if TYPE_CHECKING:
    from collections.abc import Sequence

__all__ = [
    "Eval",
    "SimulatedUser",
    "TestCase",
    "boolean",
    "called",
    "not_called",
    "numeric",
    "user",
]

#: One eval, as accepted by a [`TestCase`][skilltest_sdk.case.TestCase] —
#: build with [`boolean`][skilltest_sdk.case.boolean] /
#: [`numeric`][skilltest_sdk.case.numeric] / [`called`][skilltest_sdk.case.called]
#: / [`not_called`][skilltest_sdk.case.not_called]. The union arms are models
#: generated from the input contract.
type Eval = BooleanEval | NumericEval | CalledEval | NotCalledEval


# ---------------------------------------------------------------------------
# Evals
# ---------------------------------------------------------------------------


def boolean(criterion: str, *, expected: bool = True, name: str | None = None) -> BooleanEval:
    """Assert a plain-English ``criterion`` holds (or, with ``expected=False``,
    that it does not). Scored by the judge against the transcript."""
    return BooleanEval(type="boolean", criterion=criterion, expected=expected, name=name)


#: The comparator sugar accepted by [`numeric`][skilltest_sdk.case.numeric]:
#: the symbol or its canonical wire name.
_COMPARATORS: dict[str, Literal["gte", "gt", "lte", "lt"]] = {
    ">=": "gte",
    ">": "gt",
    "<=": "lte",
    "<": "lt",
    "gte": "gte",
    "gt": "gt",
    "lte": "lte",
    "lt": "lt",
}


def numeric(
    criterion: str,
    *,
    min: float,
    max: float,
    threshold: float,
    comparator: str = ">=",
    name: str | None = None,
) -> NumericEval:
    """Score ``criterion`` on the ``[min, max]`` scale and pass when the score
    satisfies ``comparator`` (``>=`` ``>`` ``<=`` ``<``) against ``threshold``."""
    canonical = _COMPARATORS.get(comparator)
    if canonical is None:
        raise SkilltestUsageError(
            f"unknown comparator {comparator!r}; use one of >= > <= < (or gte/gt/lte/lt)"
        )
    return NumericEval(
        type="numeric",
        criterion=criterion,
        min=min,
        max=max,
        threshold=threshold,
        comparator=canonical,
        name=name,
    )


def _compile_where(
    where: dict[str, Criterion] | None,
) -> dict[str, str | FieldPredicateSpec] | None:
    """Lower ``where`` criteria to the generated predicate models: exact
    strings pass through; [`contains`][skilltest_sdk.mock.contains] /
    [`matching`][skilltest_sdk.mock.matching] lower to their hook-side form.
    Arbitrary predicates cannot run hook-side — loud error."""
    if not where:
        return None
    compiled: dict[str, str | FieldPredicateSpec] = {}
    for key, criterion in where.items():
        if isinstance(criterion, str):
            compiled[key] = criterion
        elif isinstance(criterion, Matcher) and criterion.compiled is not None:
            compiled[key] = FieldPredicateSpec(**criterion.compiled)
        else:
            raise SkilltestUsageError(
                f"eval `where` field `{key}` must be an exact string, contains(), or "
                "matching() — arbitrary predicates cannot run hook-side"
            )
    return compiled


def called(
    mock: str,
    *,
    times: int | None = None,
    where: dict[str, Criterion] | None = None,
    name: str | None = None,
) -> CalledEval:
    """Deterministic (no judge): assert the named ``mock``/spy observed at least
    one matching call, or exactly ``times``. Reference a
    [`spy`][skilltest_sdk.mock.spy]/[`stub`][skilltest_sdk.mock.stub]/… by the
    ``name=`` you gave it in the case's ``mocks``. ``where`` narrows by input
    field (exact string or [`contains`][skilltest_sdk.mock.contains]/
    [`matching`][skilltest_sdk.mock.matching])."""
    return CalledEval(type="called", mock=mock, times=times, where=_compile_where(where), name=name)


def not_called(
    mock: str,
    *,
    where: dict[str, Criterion] | None = None,
    name: str | None = None,
) -> NotCalledEval:
    """Deterministic: assert the named ``mock``/spy observed **no** matching
    call (optionally narrowed by ``where``)."""
    return NotCalledEval(type="not_called", mock=mock, where=_compile_where(where), name=name)


# ---------------------------------------------------------------------------
# Simulated user (multi-turn)
# ---------------------------------------------------------------------------


def user(
    persona: str, *, done_when: str | None = None, max_turns: int | None = None
) -> SimulatedUser:
    """A simulated user for a multi-turn case: after each assistant turn the
    judge plays ``persona`` until ``done_when`` holds, the skill reports done,
    or ``max_turns`` is reached."""
    return SimulatedUser(persona=persona, done_when=done_when, max_turns=max_turns)


# ---------------------------------------------------------------------------
# The case
# ---------------------------------------------------------------------------


@dataclass
class TestCase:
    """A full test case defined in code — the twin of a `*.skilltest.yaml` file.

    Pass one to [`run_skill`][skilltest_sdk.runner.run_skill] /
    [`stream_skill`][skilltest_sdk.stream.stream_skill] in place of a path.
    """

    # Its name starts with `Test`, but this is not a pytest test class — tell
    # pytest not to try to collect it (avoids a PytestCollectionWarning wherever
    # a user imports it into a test module). Unannotated, so `@dataclass` ignores
    # it as a field.
    __test__ = False

    #: Skill directory under test (a dir containing SKILL.md), resolved relative
    #: to the working directory.
    skill: str | Path
    #: The initial data/prompt handed to the skill as the first user message.
    input: str
    #: The evals that decide pass/fail (must be non-empty).
    evals: Sequence[Eval]
    #: Optional report label (defaults to ``case`` when omitted).
    name: str | None = None
    #: Present => multi-turn (see [`user`][skilltest_sdk.case.user]).
    user: SimulatedUser | None = None
    #: Mock/spy objects for this case — the same builders ``run_skill(mocks=)``
    #: takes. Bound for assertions after the run; a named one is referenceable
    #: from a `called`/`not_called` eval.
    mocks: Sequence[ToolSpy] = field(default_factory=tuple)
    #: Force the observation channel even without mocks (implied when ``mocks``
    #: is non-empty), so the report carries ``mock_calls``.
    spy: bool = False

    def _compile(self) -> dict[str, Any]:
        """The JSON the CLI ingests via ``--case-json``, built through the
        generated [`_case.TestCase`] model so the payload shape is pinned to
        the input contract. Assigns names to the case's mocks (so binding and
        `called`/`not_called` references resolve) and turns the spy channel on
        when any mock/spy is present."""
        decls = [MockDecl.model_validate(d) for d in _compile_case_mocks(self.mocks)]
        model = _CaseModel(
            name=self.name,
            skill=str(self.skill),
            input=self.input,
            user=self.user,
            mocks=decls or None,
            spy=True if (self.spy or self.mocks) else None,
            evals=list(self.evals),
        )
        # exclude_none mirrors the Rust types' skip-absent serialization, so
        # the emitted JSON is the canonical minimal form the kitchen-sink
        # golden pins.
        return model.model_dump(exclude_none=True)


def _compile_case_mocks(mocks: Sequence[ToolSpy]) -> list[dict[str, Any]]:
    """Compile a case's ``mocks`` into declarations: every intercepting mock
    (stub/deny/rewrite) becomes an action declaration; a **named** spy becomes a
    no-action declaration so a `called`/`not_called` eval can reference it.
    Unnamed spies contribute nothing here — they filter locally after the run —
    but still turn the channel on via the case's ``spy`` flag."""
    decls: list[dict[str, Any]] = []
    for index, mock in enumerate(mocks):
        if isinstance(mock, ToolMock):
            decls.append(mock._decl(mock._user_name or f"__case_mock_{index}"))
        elif mock._user_name is not None:
            decls.append(mock._case_decl(mock._user_name))
    return decls
