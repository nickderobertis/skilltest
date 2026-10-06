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

* a pull request runs the affected tier (`just check` with an explicit NX_BASE
  from nx-set-shas) and reports the fixed required contexts, and triggers none
  of the live e2e suites, which no required job needs;
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
from dataclasses import dataclass, field
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS = ROOT / ".github" / "workflows"
REPO = "nickderobertis/skilltest"
REQUIRED_CONTEXTS = ("check (ubuntu-latest)", "check (macos-latest)", "pr-title", "llmlint")
LIVE = tuple(
    f"e2e-{n}.yml" for n in ("claude", "codex", "copilot", "crush", "cursor", "goose", "opencode", "qwen", "judge-api")
)
NO_RELEASE_COMMIT = ("ci.yml", *LIVE, "bundle-smoke.yml", "visual-docs.yml")


# --- the `${{ }}` expression subset these workflows use -----------------------

_TOKEN = re.compile(
    r"\s*(?:(?P<str>'(?:[^']|'')*')|(?P<op>==|!=|&&|\|\||!|\(|\)|,)|(?P<ident>[A-Za-z_][A-Za-z0-9_.\-]*))"
)


class ExprError(ValueError):
    pass


def _tokens(src: str) -> list[tuple[str, str]]:
    out, pos = [], 0
    src = src.strip()
    while pos < len(src):
        m = _TOKEN.match(src, pos)
        if not m or m.end() == pos:
            raise ExprError(f"cannot parse expression at {src[pos:]!r}")
        kind = m.lastgroup or ""
        out.append((kind, m.group(kind)))
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
    def __init__(self, src: str, ctx: dict, status: dict[str, bool]) -> None:
        self.toks, self.i, self.ctx, self.status = _tokens(src), 0, ctx, status

    def peek(self) -> str | None:
        return self.toks[self.i][1] if self.i < len(self.toks) else None

    def take(self) -> tuple[str, str]:
        tok = self.toks[self.i]
        self.i += 1
        return tok

    def parse(self) -> object:
        v = self.or_()
        if self.i != len(self.toks):
            raise ExprError(f"trailing tokens: {self.toks[self.i :]}")
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
            op = self.take()[1]
            rhs = self.unary()
            v = _eq(v, rhs) if op == "==" else not _eq(v, rhs)
        return v

    def unary(self) -> object:
        if self.peek() == "!":
            self.take()
            return not _truthy(self.unary())
        return self.atom()

    def atom(self) -> object:
        kind, text = self.take()
        if text == "(":
            v = self.or_()
            self.take()
            return v
        if kind == "str":
            return text[1:-1].replace("''", "'")
        if self.peek() == "(":
            self.take()
            args = []
            while self.peek() != ")":
                args.append(self.or_())
                if self.peek() == ",":
                    self.take()
            self.take()
            return self.call(text, args)
        if text in ("true", "false"):
            return text == "true"
        return self.lookup(text)

    def call(self, name: str, args: list[object]) -> object:
        if name == "startsWith":
            a, b = ("" if x is None else str(x) for x in args)
            return a.lower().startswith(b.lower())
        if name in self.status:
            return self.status[name]
        raise ExprError(f"unsupported function {name}()")

    def lookup(self, path: str) -> object:
        cur: object = self.ctx
        for part in path.split("."):
            if not isinstance(cur, dict):
                return None
            cur = cur.get(part)
        return cur


def evaluate(expr: object, ctx: dict, status: dict[str, bool] | None = None) -> object:
    if not isinstance(expr, str):
        return expr
    s = expr.strip()
    m = re.fullmatch(r"\$\{\{(.*)\}\}", s, re.S)
    return _Parser(m.group(1) if m else s, ctx, status or {}).parse()


def interpolate(text: str, ctx: dict) -> str:
    return re.sub(r"\$\{\{(.*?)\}\}", lambda m: str(evaluate(m.group(1), ctx)), text)


# --- events ----------------------------------------------------------------------


@dataclass
class Event:
    label: str
    name: str  # github.event_name
    ref: str
    payload: dict = field(default_factory=dict)
    dispatch: str | None = None  # the workflow file a workflow_dispatch targets

    def ctx(self, matrix: dict | None = None) -> dict:
        ref_name = (
            self.ref.rsplit("/", 1)[-1] if self.ref.startswith("refs/heads/") else self.ref.removeprefix("refs/tags/")
        )
        return {
            "github": {
                "event_name": self.name,
                "ref": self.ref,
                "ref_name": ref_name,
                "repository": REPO,
                "event": self.payload,
            },
            "matrix": matrix or {},
        }


def pull_request(action: str = "synchronize", fork: bool = False) -> Event:
    head = "someone/skilltest" if fork else REPO
    return Event(
        f"pull_request ({'fork' if fork else 'same-repo'}, {action})",
        "pull_request",
        "refs/pull/1/merge",
        {"action": action, "pull_request": {"head": {"repo": {"full_name": head}}}},
    )


def push_main(message: str) -> Event:
    return Event(
        f"push to main ({message.splitlines()[0]})", "push", "refs/heads/main", {"head_commit": {"message": message}}
    )


def push_tag(tag: str) -> Event:
    return Event(f"push tag {tag}", "push", f"refs/tags/{tag}", {"head_commit": {"message": "chore(release): 0.13.0"}})


# --- workflows -------------------------------------------------------------------


def load_workflows(directory: Path = WORKFLOWS) -> dict[str, dict]:
    out = {}
    for path in sorted(directory.glob("*.yml")):
        doc = yaml.safe_load(path.read_text())
        doc["on"] = doc.pop(True, doc.get("on"))  # YAML 1.1 reads a bare `on` key as True
        out[path.name] = doc
    return out


def _triggers(doc: dict) -> dict:
    on = doc.get("on")
    if isinstance(on, str):
        return {on: None}
    if isinstance(on, list):
        return {k: None for k in on}
    return on or {}


def triggered(doc: dict, event: Event, filename: str) -> bool:
    on = _triggers(doc)
    if event.name == "workflow_dispatch":
        return "workflow_dispatch" in on and event.dispatch == filename
    if event.name == "pull_request":
        kinds = [k for k in ("pull_request", "pull_request_target") if k in on]
        if not kinds:
            return False
        types = (on[kinds[0]] or {}).get("types") or ["opened", "synchronize", "reopened"]
        return event.payload["action"] in types
    if event.name == "push" and "push" in on:
        spec = on["push"] or {}
        if event.ref.startswith("refs/tags/"):
            pats = spec.get("tags")
            return pats is not None and any(fnmatch.fnmatch(event.ref[len("refs/tags/") :], p) for p in pats)
        pats = spec.get("branches")
        if pats is None:
            return "tags" not in spec
        return any(fnmatch.fnmatch(event.ref[len("refs/heads/") :], p) for p in pats)
    return False


def _contexts(job_id: str, job: dict, event: Event) -> list[str]:
    name = job.get("name", job_id)
    matrix = (job.get("strategy") or {}).get("matrix") or {}
    rows = (
        [{k: v} for k, vals in matrix.items() if k != "include" for v in vals]
        or list(matrix.get("include", []))
        or [{}]
    )
    out = []
    for row in rows:
        shown = interpolate(name, event.ctx(row))
        if "name" not in job and row:
            shown = f"{shown} ({', '.join(str(v) for v in row.values())})"
        out.append(shown)
    return out


@dataclass
class JobRun:
    workflow: str
    job: str
    result: str  # success | failure | skipped
    contexts: list[str]
    commands: list[str]
    called: dict[str, JobRun] = field(default_factory=dict)


def run_workflow(filename: str, docs: dict[str, dict], event: Event, fail: frozenset[str]) -> dict[str, JobRun]:
    """Every job's outcome for `event`, failing the `workflow:job` keys in `fail`."""
    jobs = docs[filename].get("jobs", {})
    results: dict[str, JobRun] = {}
    pending = list(jobs)
    while pending:
        for job_id in pending:
            job = jobs[job_id]
            needs = job.get("needs", [])
            needs = [needs] if isinstance(needs, str) else needs
            if all(n in results for n in needs):
                break
        else:
            raise ExprError(f"{filename}: needs cycle among {pending}")
        pending.remove(job_id)
        ok = all(results[n].result == "success" for n in needs)
        status = {
            "success": ok,
            "always": True,
            "failure": any(results[n].result == "failure" for n in needs),
            "cancelled": False,
        }
        cond = job.get("if")
        uses_status = isinstance(cond, str) and re.search(r"\b(success|always|failure|cancelled)\(", cond)
        runs = _truthy(evaluate(cond, event.ctx(), status)) if cond is not None else True
        if not uses_status:
            runs = runs and ok
        run = JobRun(filename, job_id, "skipped", _contexts(job_id, job, event), [])
        if runs:
            run.result = "success"
            uses = job.get("uses", "")
            if uses.startswith("./.github/workflows/"):
                called = uses.rsplit("/", 1)[-1]
                run.called = run_workflow(called, docs, event, fail)
                if any(r.result == "failure" for r in run.called.values()):
                    run.result = "failure"
            for step in job.get("steps", []):
                if "run" in step and _truthy(evaluate(step.get("if", True), event.ctx(), status)):
                    env = {k: interpolate(str(v), event.ctx()) for k, v in (step.get("env") or {}).items()}
                    # Substitute the step's own env into `"$VAR"`, as the shell would.
                    run.commands.append(
                        re.sub(r'"\$(\w+)"', lambda m: env.get(m.group(1), m.group(0)), step["run"]).strip()
                    )
                elif "uses" in step and _truthy(evaluate(step.get("if", True), event.ctx(), status)):
                    run.commands.append(f"uses {step['uses']}")
            if f"{filename}:{job_id}" in fail:
                run.result = "failure"
        results[job_id] = run
    return results


def simulate(docs: dict[str, dict], event: Event, fail: frozenset[str] = frozenset()) -> dict[str, dict[str, JobRun]]:
    return {f: run_workflow(f, docs, event, fail) for f in docs if triggered(docs[f], event, f)}


def _ran(runs: dict[str, dict[str, JobRun]], filename: str) -> list[str]:
    return [j for j, r in runs.get(filename, {}).items() if r.result != "skipped"]


def _needs_closure(doc: dict, job_id: str) -> set[str]:
    seen, stack = set(), [job_id]
    while stack:
        needs = doc["jobs"][stack.pop()].get("needs", [])
        for n in [needs] if isinstance(needs, str) else needs:
            if n not in seen:
                seen.add(n)
                stack.append(n)
    return seen


# --- the contract ------------------------------------------------------------------


def violations(docs: dict[str, dict]) -> list[str]:
    bad: list[str] = []
    ci = docs["ci.yml"]

    # Pull requests: the affected tier against an explicit base, every required
    # context reported, no live suite. Same-repo and fork heads alike.
    for event in (pull_request("opened"), pull_request("synchronize"), pull_request("synchronize", fork=True)):
        runs = simulate(docs, event)
        reported = {c for wf in runs.values() for r in wf.values() if r.result != "skipped" for c in r.contexts}
        for ctx in REQUIRED_CONTEXTS:
            if ctx not in reported:
                bad.append(f"{event.label}: required context `{ctx}` is not reported")
        for live in LIVE:
            if live in runs:
                bad.append(f"{event.label}: live suite {live} is triggered")
        for job_id, r in runs.get("ci.yml", {}).items():
            for sub in r.called.values():
                if sub.result != "skipped" and sub.workflow in LIVE:
                    bad.append(f"{event.label}: ci.yml:{job_id} runs live suite {sub.workflow}")
        check = runs.get("ci.yml", {}).get("check")
        cmds = check.commands if check else []
        if "just check affected" not in cmds:
            bad.append(
                f"{event.label}: ci.yml `check` does not run the affected tier (`just check affected`); ran {cmds}"
            )
        if "uses nrwl/nx-set-shas@v4" not in cmds:
            bad.append(f"{event.label}: ci.yml `check` derives no explicit NX_BASE (nx-set-shas) before `just check`")
    # No required-context job may wait on a live suite or on notignored.
    for job_id, job in ci["jobs"].items():
        if job_id in ("check", "llmlint"):
            closure = _needs_closure(ci, job_id)
            for n in closure:
                uses = ci["jobs"][n].get("uses", "")
                if any(uses.endswith(live) for live in LIVE):
                    bad.append(f"ci.yml:{job_id} (a required context) needs live suite job {n}")

    # An ordinary push to main: the sweep, every live suite, then the release.
    feat = push_main("feat: something (#99)")
    runs = simulate(docs, feat)
    check = runs.get("ci.yml", {}).get("check")
    if not check or check.result != "success" or "just check all" not in check.commands:
        bad.append(f"{feat.label}: ci.yml `check` does not run the full sweep (`just check all`)")
    live_jobs = {}
    for job_id, r in runs.get("ci.yml", {}).items():
        for sub in r.called.values():
            if sub.workflow in LIVE and sub.result == "success":
                live_jobs[sub.workflow] = f"{sub.workflow}:{sub.job}"
    for live in LIVE:
        if live not in live_jobs:
            bad.append(f"{feat.label}: live suite {live} does not run")
    release = runs.get("ci.yml", {}).get("release")
    if (
        not release
        or release.result != "success"
        or not any(s.workflow == "semantic-release.yml" and s.result == "success" for s in release.called.values())
    ):
        bad.append(f"{feat.label}: the release (semantic-release.yml) does not run after a green sweep")
    if "semantic-release.yml" in runs:
        bad.append(f"{feat.label}: semantic-release.yml is triggered directly, not after the sweep")
    for broken in ["ci.yml:check", *live_jobs.values()]:
        r = simulate(docs, feat, frozenset({broken})).get("ci.yml", {}).get("release")
        if r and r.result != "skipped":
            bad.append(f"{feat.label}: with {broken} failing, the release still runs")

    # The release commit: nothing gated again; the tag still fires the builds.
    rel = push_main("chore(release): 0.13.0 [bot]")
    runs = simulate(docs, rel)
    for wf in NO_RELEASE_COMMIT:
        if _ran(runs, wf):
            bad.append(f"{rel.label}: {wf} runs {_ran(runs, wf)}")
    for wf, rs in runs.items():
        for r in rs.values():
            if any(s.result != "skipped" for s in r.called.values()):
                bad.append(f"{rel.label}: {wf}:{r.job} calls a workflow")
    tag = push_tag("v0.13.0")
    runs = simulate(docs, tag)
    for wf in ("release.yml", "publish.yml"):
        if not _ran(runs, wf):
            bad.append(f"{tag.label}: {wf} does not run")
    for wf in NO_RELEASE_COMMIT:
        if _ran(runs, wf):
            bad.append(f"{tag.label}: {wf} runs on the tag")

    # Each live suite is still runnable by hand.
    for live in LIVE:
        ev = Event(f"workflow_dispatch {live}", "workflow_dispatch", "refs/heads/main", dispatch=live)
        if not _ran(simulate(docs, ev), live):
            bad.append(f"{ev.label}: does not run")
    return bad


def _mutations(docs: dict[str, dict]) -> list[tuple[str, dict[str, dict]]]:
    """Broken copies of the workflows, each of which the contract must reject."""
    out = []

    def m(label: str, fn) -> None:
        d = copy.deepcopy(docs)
        fn(d)
        out.append((label, d))

    m("a live suite triggered on pull_request", lambda d: d["e2e-claude.yml"]["on"].update({"pull_request": None}))
    m(
        "the release not waiting on one live suite",
        lambda d: d["ci.yml"]["jobs"]["release"]["needs"].remove("live-qwen"),
    )
    m("the release not waiting on the sweep", lambda d: d["ci.yml"]["jobs"]["release"]["needs"].remove("check"))
    m("bundle-smoke without the release-commit guard", lambda d: d["bundle-smoke.yml"]["jobs"]["smoke"].pop("if"))
    m("ci's check running on the release commit", lambda d: d["ci.yml"]["jobs"]["check"].update({"if": "true"}))
    m(
        "no explicit NX_BASE on a pull request",
        lambda d: d["ci.yml"]["jobs"]["check"]["steps"].__setitem__(
            slice(None), [s for s in d["ci.yml"]["jobs"]["check"]["steps"] if "nx-set-shas" not in s.get("uses", "")]
        ),
    )
    m(
        "the sweep tier dropped at merge-to-main",
        lambda d: [
            s["env"].update({"TIER": "affected"})
            for s in d["ci.yml"]["jobs"]["check"]["steps"]
            if "TIER" in (s.get("env") or {})
        ],
    )
    m(
        "llmlint skipped on a fork pull request",
        lambda d: d["ci.yml"]["jobs"]["llmlint"].update(
            {"if": "github.event.pull_request.head.repo.full_name == github.repository"}
        ),
    )
    m(
        "a required context needing a live suite",
        lambda d: d["ci.yml"]["jobs"]["llmlint"].update({"needs": ["live-claude"]}),
    )
    m(
        "semantic-release triggered directly on push",
        lambda d: d["semantic-release.yml"].update({"on": {"push": {"branches": ["main"]}}}),
    )
    return out


def report(docs: dict[str, dict]) -> None:
    events = [
        pull_request("synchronize"),
        pull_request("synchronize", fork=True),
        push_main("feat: something (#99)"),
        push_main("chore(release): 0.13.0 [bot]"),
        push_tag("v0.13.0"),
    ]
    for event in events:
        print(f"== {event.label}")
        for wf, rs in simulate(docs, event).items():
            for r in rs.values():
                called = "".join(f" -> {s.workflow}:{s.job} {s.result}" for s in r.called.values())
                cmd = next((c for c in r.commands if c.startswith("just check")), "")
                print(f"  {wf}:{r.job:<10} {r.result:<8} {cmd}{called}")
    feat = push_main("feat: something (#99)")
    for broken in ("ci.yml:check", "e2e-qwen.yml:live"):
        r = simulate(docs, feat, frozenset({broken}))["ci.yml"]["release"]
        print(f"== {feat.label}, {broken} induced to fail: ci.yml:release {r.result}")


def main(argv: list[str]) -> int:
    docs = load_workflows()
    if "--report" in argv:
        report(docs)
    problems = violations(docs)
    if problems:
        print(f"check-workflow-routing: {len(problems)} routing violation(s):", file=sys.stderr)
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        print('  fix the workflow named (see AGENTS.md "Commits, releases, and merging")', file=sys.stderr)
        return 1
    blind = [label for label, broken in _mutations(docs) if not violations(broken)]
    if blind:
        print(
            f"check-workflow-routing: the contract no longer catches: {', '.join(blind)}; "
            "a check in scripts/check-workflow-routing.py stopped seeing what it guards",
            file=sys.stderr,
        )
        return 1
    print(
        f"check-workflow-routing: {len(docs)} workflows route as the release model requires; "
        f"{len(_mutations(docs))} broken variants each rejected"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
