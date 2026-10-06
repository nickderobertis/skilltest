#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyyaml==6.0.2"]
# ///
# llmlint: ignore-file[new_code_lands_in_a_project] repo-level gate over .github/workflows, which belongs to no Nx project; run workspace-wide from `just check` like scripts/check-release-targets.sh (AGENTS.md: scripts/* are orchestrator-independent glue).
"""Workflow-routing contract: which jobs each GitHub event runs.

The release model (AGENTS.md "Commits, releases, and merging") lives in the
`on:` triggers, job `if:` guards and `needs:` edges spread over every workflow,
where no single file shows it and a hosted run proves it only after merge. This
reads the committed workflows, evaluates each event the way Actions does —
trigger filters, `if:` expressions (implicit `success()` over `needs`), called
workflows — and asserts the routing:

* a pull request runs the affected tier (`just check affected` after
  nx-set-shas exports an explicit NX_BASE) and reports the fixed required
  contexts, and triggers none of the live e2e suites, which no required job needs;
* an ordinary push to main runs the full sweep (`just check all`) and every live
  suite, and reaches the release only through their success — a failure induced
  in the sweep or in any one live suite leaves the release skipped;
* the `chore(release):` commit runs no ci, live e2e, bundle-smoke or visual-docs
  job, while the `v*` tag still fires release.yml and publish.yml.

Then it re-runs those assertions over deliberately broken copies of the
workflows, each of which must fail — a gate nobody has watched fail is not known
to work. Quiet on success, one line; `--report` prints the routing table.
"""

from __future__ import annotations

import copy
import fnmatch
import re
import sys
from collections.abc import Callable
from dataclasses import dataclass, field
from enum import StrEnum
from pathlib import Path
from typing import Literal

import yaml

ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS = ROOT / ".github" / "workflows"
REPO = "nickderobertis/skilltest"
REQUIRED_CONTEXTS = ("check (ubuntu-latest)", "check (macos-latest)", "pr-title", "llmlint")
LIVE = tuple(
    f"e2e-{n}.yml" for n in ("claude", "codex", "copilot", "crush", "cursor", "goose", "opencode", "qwen", "judge-api")
)
NO_RELEASE_COMMIT = ("ci.yml", *LIVE, "bundle-smoke.yml", "visual-docs.yml")
FIX = 'fix the workflow named (AGENTS.md "Commits, releases, and merging" states the intended routing)'


class WorkflowError(ValueError):
    """A workflow file, or an expression in one, this model cannot read."""


# --- the `${{ }}` expression subset these workflows use -----------------------

TokenKind = Literal["str", "op", "ident"]
_KINDS: dict[str, TokenKind] = {"str": "str", "op": "op", "ident": "ident"}


@dataclass(frozen=True)
class Token:
    kind: TokenKind
    text: str


_TOKEN = re.compile(r"\s*(?:(?P<str>'(?:[^']|'')*')|(?P<op>==|!=|&&|\|\||!|\(|\)|,)|(?P<ident>[A-Za-z_][\w.\-]*))")


def _tokens(src: str) -> list[Token]:
    out: list[Token] = []
    pos, src = 0, src.strip()
    while pos < len(src):
        m = _TOKEN.match(src, pos)
        if not m or m.end() == pos or m.lastgroup not in _KINDS:
            raise WorkflowError(f"cannot parse expression at {src[pos:]!r}")
        kind = _KINDS[m.lastgroup]
        out.append(Token(kind, m.group(kind)))
        pos = m.end()
        while pos < len(src) and src[pos].isspace():
            pos += 1
    return out


def _truthy(v: object) -> bool:
    return v not in (None, False, "", 0)


def _eq(a: object, b: object) -> bool:
    if isinstance(a, str) and isinstance(b, str):
        return a.lower() == b.lower()  # Actions compares strings case-insensitively
    return a == b


class _Parser:
    def __init__(self, src: str, ctx: dict[str, object], status: dict[str, bool]) -> None:
        self.toks, self.i, self.ctx, self.status = _tokens(src), 0, ctx, status

    def peek(self) -> str | None:
        return self.toks[self.i].text if self.i < len(self.toks) else None

    def take(self, expect: str | None = None) -> Token:
        if self.i >= len(self.toks):
            raise WorkflowError(f"expression ends early{f', expected {expect!r}' if expect else ''}")
        tok = self.toks[self.i]
        if expect is not None and tok.text != expect:
            raise WorkflowError(f"expected {expect!r}, found {tok.text!r}")
        self.i += 1
        return tok

    def parse(self) -> object:
        v = self.or_()
        if self.i != len(self.toks):
            raise WorkflowError(f"trailing tokens: {[t.text for t in self.toks[self.i :]]}")
        return v

    def or_(self) -> object:
        v = self.and_()
        while self.peek() == "||":
            self.take()
            rhs = self.and_()
            v = v if _truthy(v) else rhs
        return v

    def and_(self) -> object:
        v = self.cmp()
        while self.peek() == "&&":
            self.take()
            rhs = self.cmp()
            v = rhs if _truthy(v) else v
        return v

    def cmp(self) -> object:
        v = self.unary()
        while self.peek() in ("==", "!="):
            op = self.take().text
            rhs = self.unary()
            v = _eq(v, rhs) if op == "==" else not _eq(v, rhs)
        return v

    def unary(self) -> object:
        if self.peek() == "!":
            self.take()
            return not _truthy(self.unary())
        return self.atom()

    def atom(self) -> object:
        tok = self.take()
        match tok:
            case Token(kind="op", text="("):
                v = self.or_()
                self.take(")")
                return v
            case Token(kind="str", text=text):
                return text[1:-1].replace("''", "'")
            case Token(kind="ident", text=("true" | "false") as text):
                return text == "true"
            case Token(kind="ident", text=name) if self.peek() == "(":
                self.take("(")
                args: list[object] = []
                while self.peek() != ")":
                    args.append(self.or_())
                    if self.peek() == ",":
                        self.take(",")
                self.take(")")
                return self.call(name, args)
            case Token(kind="ident", text=path):
                return self.lookup(path)
            case _:
                raise WorkflowError(f"unexpected {tok.text!r}")

    def call(self, name: str, args: list[object]) -> object:
        match name, args:
            case "startsWith", [a, b]:
                return ("" if a is None else str(a)).lower().startswith(("" if b is None else str(b)).lower())
            case _ if name in self.status and not args:
                return self.status[name]
            case _:
                raise WorkflowError(f"unsupported function {name}() with {len(args)} argument(s)")

    def lookup(self, path: str) -> object:
        cur: object = self.ctx
        for part in path.split("."):
            if not isinstance(cur, dict):
                return None
            cur = cur.get(part)
        return cur


def evaluate(expr: object, ctx: dict[str, object], status: dict[str, bool] | None = None) -> object:
    """An `if:` value: a bare expression or one `${{ }}`, else the literal itself."""
    if not isinstance(expr, str):
        return expr
    m = re.fullmatch(r"\s*\$\{\{(.*)\}\}\s*", expr, re.S)
    return _Parser(m.group(1) if m else expr, ctx, status or {}).parse()


def interpolate(text: str, ctx: dict[str, object]) -> str:
    """A string value: literal text with each `${{ }}` replaced by its value."""
    return re.sub(r"\$\{\{(.*?)\}\}", lambda m: str(evaluate(m.group(1), ctx)), text)


# --- the workflows, validated into a model ------------------------------------------


@dataclass(frozen=True)
class Step:
    run: str | None
    uses: str | None
    cond: object
    env: dict[str, str]


@dataclass(frozen=True)
class Job:
    id: str
    name: str | None
    needs: tuple[str, ...]
    cond: object
    uses: str | None
    matrix: dict[str, object]
    steps: tuple[Step, ...]


@dataclass(frozen=True)
class Workflow:
    filename: str
    triggers: dict[str, dict[str, object]]
    jobs: dict[str, Job]


def _mapping(value: object, where: str) -> dict:
    if value is None:
        return {}
    if not isinstance(value, dict):
        raise WorkflowError(f"{where} must be a mapping")
    return value


def _optional_str(value: object, where: str) -> str | None:
    if value is not None and not isinstance(value, str):
        raise WorkflowError(f"{where} must be a string")
    return value


def parse_workflow(filename: str, raw: object) -> Workflow:
    doc = _mapping(raw, filename)
    on = doc.get(True, doc.get("on"))  # YAML 1.1 reads a bare `on` key as True
    match on:
        case str():
            triggers: dict[str, dict[str, object]] = {on: {}}
        case list():
            triggers = {str(k): {} for k in on}
        case dict():
            # A trigger's filters are a mapping; `schedule` alone is a list of crons,
            # which no event modelled here reads.
            triggers = {
                str(k): {} if isinstance(v, list) else _mapping(v, f"{filename}: on.{k}") for k, v in on.items()
            }
        case _:
            raise WorkflowError(f"{filename}: `on` must be a string, list or mapping")
    jobs: dict[str, Job] = {}
    for job_id, raw_job in _mapping(doc.get("jobs"), f"{filename}: jobs").items():
        where = f"{filename}: jobs.{job_id}"
        job = _mapping(raw_job, where)
        needs = job.get("needs", [])
        needs = [needs] if isinstance(needs, str) else needs
        if not isinstance(needs, list) or not all(isinstance(n, str) for n in needs):
            raise WorkflowError(f"{where}.needs must be a job id or a list of them")
        steps = []
        for i, raw_step in enumerate(job.get("steps") or []):
            step = _mapping(raw_step, f"{where}.steps[{i}]")
            env = {str(k): str(v) for k, v in _mapping(step.get("env"), f"{where}.steps[{i}].env").items()}
            run = _optional_str(step.get("run"), f"{where}.steps[{i}].run")
            uses = _optional_str(step.get("uses"), f"{where}.steps[{i}].uses")
            steps.append(Step(run, uses, step.get("if", True), env))
        strategy = _mapping(job.get("strategy"), f"{where}.strategy")
        jobs[job_id] = Job(
            id=job_id,
            name=_optional_str(job.get("name"), f"{where}.name"),
            needs=tuple(needs),
            cond=job.get("if"),
            uses=_optional_str(job.get("uses"), f"{where}.uses"),
            matrix=_mapping(strategy.get("matrix"), f"{where}.strategy.matrix"),
            steps=tuple(steps),
        )
    return Workflow(filename, triggers, jobs)


def load_raw(directory: Path = WORKFLOWS) -> dict[str, object]:
    return {path.name: yaml.safe_load(path.read_text()) for path in sorted(directory.glob("*.yml"))}


def parse_all(raw: dict[str, object]) -> dict[str, Workflow]:
    return {name: parse_workflow(name, doc) for name, doc in raw.items()}


# --- events and their simulation -----------------------------------------------------


@dataclass(frozen=True)
class Event:
    label: str
    name: str  # github.event_name
    ref: str
    payload: dict[str, object] = field(default_factory=dict)
    dispatch: str | None = None  # the workflow file a workflow_dispatch targets

    def ctx(self, matrix: dict[str, object] | None = None) -> dict[str, object]:
        ref_name = self.ref.removeprefix("refs/heads/").removeprefix("refs/tags/")
        github = {"event_name": self.name, "ref": self.ref, "ref_name": ref_name, "repository": REPO}
        return {"github": {**github, "event": self.payload}, "matrix": matrix or {}}


def pull_request(action: str = "synchronize", fork: bool = False) -> Event:
    head = "someone/skilltest" if fork else REPO
    return Event(
        f"pull_request ({'fork' if fork else 'same-repo'}, {action})",
        "pull_request",
        "refs/pull/1/merge",
        {"action": action, "pull_request": {"head": {"repo": {"full_name": head}}}},
    )


def push_main(message: str) -> Event:
    return Event(f"push to main ({message})", "push", "refs/heads/main", {"head_commit": {"message": message}})


def push_tag(tag: str) -> Event:
    return Event(f"push tag {tag}", "push", f"refs/tags/{tag}", {"head_commit": {"message": "chore(release): 0.13.0"}})


def dispatch(filename: str) -> Event:
    return Event(f"workflow_dispatch {filename}", "workflow_dispatch", "refs/heads/main", dispatch=filename)


def _matches(patterns: object, name: str) -> bool:
    return isinstance(patterns, list) and any(fnmatch.fnmatch(name, str(p)) for p in patterns)


def triggered(wf: Workflow, event: Event) -> bool:
    on = wf.triggers
    match event.name:
        case "workflow_dispatch":
            return "workflow_dispatch" in on and event.dispatch == wf.filename
        case "pull_request":
            kind = next((k for k in ("pull_request", "pull_request_target") if k in on), None)
            if kind is None:
                return False
            types = on[kind].get("types") or ["opened", "synchronize", "reopened"]
            return event.payload["action"] in types
        case "push" if "push" in on:
            spec = on["push"]
            if event.ref.startswith("refs/tags/"):
                return _matches(spec.get("tags"), event.ref.removeprefix("refs/tags/"))
            if spec.get("branches") is None:
                return "tags" not in spec
            return _matches(spec.get("branches"), event.ref.removeprefix("refs/heads/"))
        case _:
            return False


class Result(StrEnum):
    SUCCESS = "success"
    FAILURE = "failure"
    SKIPPED = "skipped"


@dataclass
class JobRun:
    workflow: str
    job: str
    result: Result
    contexts: list[str]
    commands: list[str] = field(default_factory=list)
    called: dict[str, JobRun] = field(default_factory=dict)


def _contexts(job: Job, event: Event) -> list[str]:
    rows: list[dict[str, object]] = [
        {k: v} for k, vals in job.matrix.items() if k != "include" and isinstance(vals, list) for v in vals
    ]
    include = job.matrix.get("include")
    rows = rows or (list(include) if isinstance(include, list) else []) or [{}]
    out = []
    for row in rows:
        shown = interpolate(job.name or job.id, event.ctx(row))
        if job.name is None and row:
            shown = f"{shown} ({', '.join(str(v) for v in row.values())})"
        out.append(shown)
    return out


def run_workflow(filename: str, wfs: dict[str, Workflow], event: Event, fail: frozenset[str]) -> dict[str, JobRun]:
    """Every job's outcome for `event`, failing the `workflow:job` keys in `fail`."""
    jobs = wfs[filename].jobs
    results: dict[str, JobRun] = {}
    pending = list(jobs)
    while pending:
        ready = next((j for j in pending if all(n in results for n in jobs[j].needs)), None)
        if ready is None:
            raise WorkflowError(f"{filename}: needs cycle or unknown job among {pending}")
        pending.remove(ready)
        job = jobs[ready]
        ok = all(results[n].result == Result.SUCCESS for n in job.needs)
        status = {
            "success": ok,
            "always": True,
            "failure": any(results[n].result == Result.FAILURE for n in job.needs),
            "cancelled": False,
        }
        uses_status = isinstance(job.cond, str) and re.search(r"\b(success|always|failure|cancelled)\(", job.cond)
        runs = _truthy(evaluate(job.cond, event.ctx(), status)) if job.cond is not None else True
        if not uses_status:
            runs = runs and ok  # Actions' implicit `success() &&`
        run = JobRun(filename, ready, Result.SKIPPED, _contexts(job, event))
        if runs:
            run.result = Result.SUCCESS
            if job.uses and job.uses.startswith("./.github/workflows/"):
                run.called = run_workflow(job.uses.rsplit("/", 1)[-1], wfs, event, fail)
                if any(r.result == Result.FAILURE for r in run.called.values()):
                    run.result = Result.FAILURE
            for step in job.steps:
                if not _truthy(evaluate(step.cond, event.ctx(), status)):
                    continue
                if step.run is not None:
                    env = {k: interpolate(v, event.ctx()) for k, v in step.env.items()}
                    # Substitute the step's own env into `"$VAR"`, as the shell would.
                    run.commands.append(
                        re.sub(r'"\$(\w+)"', lambda m: env.get(m.group(1), m.group(0)), step.run).strip()
                    )
                elif step.uses is not None:
                    run.commands.append(f"uses {step.uses}")
            if f"{filename}:{ready}" in fail:
                run.result = Result.FAILURE
        results[ready] = run
    return results


def simulate(
    wfs: dict[str, Workflow], event: Event, fail: frozenset[str] = frozenset()
) -> dict[str, dict[str, JobRun]]:
    return {f: run_workflow(f, wfs, event, fail) for f, wf in wfs.items() if triggered(wf, event)}


def _ran(runs: dict[str, dict[str, JobRun]], filename: str) -> list[str]:
    return [j for j, r in runs.get(filename, {}).items() if r.result != Result.SKIPPED]


def _needs_closure(wf: Workflow, job_id: str) -> set[str]:
    seen: set[str] = set()
    stack = [job_id]
    while stack:
        for n in wf.jobs[stack.pop()].needs:
            if n not in seen:
                seen.add(n)
                stack.append(n)
    return seen


# --- the contract ---------------------------------------------------------------------


def violations(wfs: dict[str, Workflow]) -> list[str]:
    bad: list[str] = []
    ci = wfs["ci.yml"]

    # Pull requests: the affected tier against an explicit base, every required
    # context reported, no live suite. Same-repo and fork heads alike.
    for event in (pull_request("opened"), pull_request("synchronize"), pull_request("synchronize", fork=True)):
        runs = simulate(wfs, event)
        reported = {c for wf in runs.values() for r in wf.values() if r.result != Result.SKIPPED for c in r.contexts}
        bad += [
            f"{event.label}: required context `{c}` is not reported" for c in REQUIRED_CONTEXTS if c not in reported
        ]
        bad += [f"{event.label}: live suite {live} is triggered" for live in LIVE if live in runs]
        for job_id, r in runs.get("ci.yml", {}).items():
            bad += [
                f"{event.label}: ci.yml:{job_id} runs live suite {sub.workflow}"
                for sub in r.called.values()
                if sub.result != Result.SKIPPED and sub.workflow in LIVE
            ]
        check = runs.get("ci.yml", {}).get("check")
        cmds = check.commands if check else []
        if "just check affected" not in cmds:
            bad.append(
                f"{event.label}: ci.yml `check` does not run the affected tier (`just check affected`); ran {cmds}"
            )
        if "uses nrwl/nx-set-shas@v4" not in cmds:
            bad.append(f"{event.label}: ci.yml `check` derives no explicit NX_BASE (nx-set-shas) before `just check`")
    # No required-context job may wait on a live suite.
    for job_id in ("check", "llmlint"):
        for n in _needs_closure(ci, job_id):
            if any((ci.jobs[n].uses or "").endswith(live) for live in LIVE):
                bad.append(f"ci.yml:{job_id} (a required context) needs live suite job {n}")

    # An ordinary push to main: the sweep, every live suite, then the release.
    feat = push_main("feat: something (#99)")
    runs = simulate(wfs, feat)
    check = runs.get("ci.yml", {}).get("check")
    if not check or check.result != Result.SUCCESS or "just check all" not in check.commands:
        bad.append(f"{feat.label}: ci.yml `check` does not run the full sweep (`just check all`)")
    live_jobs = {
        sub.workflow: f"{sub.workflow}:{sub.job}"
        for r in runs.get("ci.yml", {}).values()
        for sub in r.called.values()
        if sub.workflow in LIVE and sub.result == Result.SUCCESS
    }
    bad += [f"{feat.label}: live suite {live} does not run" for live in LIVE if live not in live_jobs]
    release = runs.get("ci.yml", {}).get("release")
    released = release is not None and any(
        s.workflow == "semantic-release.yml" and s.result == Result.SUCCESS for s in release.called.values()
    )
    if not released:
        bad.append(f"{feat.label}: the release (semantic-release.yml) does not run after a green sweep")
    if "semantic-release.yml" in runs:
        bad.append(f"{feat.label}: semantic-release.yml is triggered directly, not after the sweep")
    for broken in ["ci.yml:check", *live_jobs.values()]:
        r = simulate(wfs, feat, frozenset({broken})).get("ci.yml", {}).get("release")
        if r is not None and r.result != Result.SKIPPED:
            bad.append(f"{feat.label}: with {broken} failing, the release still runs")

    # The release commit: nothing gated again; the tag still fires the builds.
    rel = push_main("chore(release): 0.13.0 [bot]")
    runs = simulate(wfs, rel)
    bad += [f"{rel.label}: {wf} runs {_ran(runs, wf)}" for wf in NO_RELEASE_COMMIT if _ran(runs, wf)]
    for wf, rs in runs.items():
        bad += [
            f"{rel.label}: {wf}:{r.job} calls a workflow"
            for r in rs.values()
            if any(s.result != Result.SKIPPED for s in r.called.values())
        ]
    tag = push_tag("v0.13.0")
    runs = simulate(wfs, tag)
    bad += [f"{tag.label}: {wf} does not run" for wf in ("release.yml", "publish.yml") if not _ran(runs, wf)]
    bad += [f"{tag.label}: {wf} runs on the tag" for wf in NO_RELEASE_COMMIT if _ran(runs, wf)]

    # Each live suite is still runnable by hand.
    bad += [f"{dispatch(live).label}: does not run" for live in LIVE if not _ran(simulate(wfs, dispatch(live)), live)]
    return bad


# Deliberately broken edits of the raw workflow documents (YAML 1.1 parses the
# bare `on` key as True), each of which the contract must reject.
Mutation = Callable[[dict], None]


def _drop_step(job: dict, needle: str) -> None:
    job["steps"] = [s for s in job["steps"] if needle not in s.get("uses", "")]


def _set_tier(job: dict, tier: str) -> None:
    for step in job["steps"]:
        if "TIER" in (step.get("env") or {}):
            step["env"]["TIER"] = tier


def _jobs(d: dict, workflow: str) -> dict:
    return d[workflow]["jobs"]


FORK_GUARD = "github.event.pull_request.head.repo.full_name == github.repository"
MUTATIONS: tuple[tuple[str, Mutation], ...] = (
    ("a live suite triggered on pull_request", lambda d: d["e2e-claude.yml"][True].update({"pull_request": None})),
    ("the release not waiting on one live suite", lambda d: _jobs(d, "ci.yml")["release"]["needs"].remove("live-qwen")),
    ("the release not waiting on the sweep", lambda d: _jobs(d, "ci.yml")["release"]["needs"].remove("check")),
    ("bundle-smoke without the release-commit guard", lambda d: _jobs(d, "bundle-smoke.yml")["smoke"].pop("if")),
    ("ci's check running on the release commit", lambda d: _jobs(d, "ci.yml")["check"].update({"if": "true"})),
    ("no explicit NX_BASE on a pull request", lambda d: _drop_step(_jobs(d, "ci.yml")["check"], "nx-set-shas")),
    ("the sweep tier dropped at merge-to-main", lambda d: _set_tier(_jobs(d, "ci.yml")["check"], "affected")),
    ("llmlint skipped on a fork pull request", lambda d: _jobs(d, "ci.yml")["llmlint"].update({"if": FORK_GUARD})),
    (
        "a required context needing a live suite",
        lambda d: _jobs(d, "ci.yml")["llmlint"].update({"needs": ["live-claude"]}),
    ),
    ("semantic-release triggered on push", lambda d: d["semantic-release.yml"].update({True: {"push": None}})),
)


def blind_spots(raw: dict[str, object]) -> list[str]:
    """The mutations the contract fails to reject."""
    blind = []
    for label, mutate in MUTATIONS:
        broken = copy.deepcopy(raw)
        mutate(broken)
        if not violations(parse_all(broken)):
            blind.append(label)
    return blind


def report(wfs: dict[str, Workflow]) -> None:
    events = [
        pull_request("synchronize"),
        pull_request("synchronize", fork=True),
        push_main("feat: something (#99)"),
        push_main("chore(release): 0.13.0 [bot]"),
        push_tag("v0.13.0"),
    ]
    for event in events:
        print(f"== {event.label}")
        for wf, rs in simulate(wfs, event).items():
            for r in rs.values():
                called = "".join(f" -> {s.workflow}:{s.job} {s.result}" for s in r.called.values())
                cmd = next((c for c in r.commands if c.startswith("just check")), "")
                print(f"  {wf + ':' + r.job:<34} {r.result:<8} {cmd}{called}")
    feat = push_main("feat: something (#99)")
    for broken in ("ci.yml:check", "e2e-qwen.yml:live"):
        r = simulate(wfs, feat, frozenset({broken}))["ci.yml"]["release"]
        print(f"== {feat.label}, {broken} induced to fail: ci.yml:release {r.result}")


def main(argv: list[str]) -> int:
    unknown = [a for a in argv if a != "--report"]
    if unknown:
        print(f"check-workflow-routing: unknown argument(s) {unknown}; the only option is --report", file=sys.stderr)
        return 2
    try:
        raw = load_raw()
        wfs = parse_all(raw)
        if "--report" in argv:
            report(wfs)
        problems = violations(wfs)
        blind = [] if problems else blind_spots(raw)
    except (OSError, yaml.YAMLError, WorkflowError) as exc:
        print(f"check-workflow-routing: cannot model .github/workflows: {exc}; {FIX}", file=sys.stderr)
        return 2
    if problems:
        print(f"check-workflow-routing: {len(problems)} routing violation(s):", file=sys.stderr)
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        print(f"  {FIX}", file=sys.stderr)
        return 1
    if blind:
        print(
            f"check-workflow-routing: the contract no longer catches: {', '.join(blind)}; restore the assertion in "
            "violations() that guards it (or update MUTATIONS if the routing changed on purpose)",
            file=sys.stderr,
        )
        return 1
    print(
        f"check-workflow-routing: {len(wfs)} workflows route as the release model requires; "
        f"{len(MUTATIONS)} broken variants each rejected"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
