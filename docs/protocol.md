# Providers

skilltest never talks to a model directly. A **provider** does three things for
the runner: produce an assistant/skill turn (`respond`), play the simulated user
(`user`), and score the transcript against a criterion (`judge`). There are two
provider backends.

## 1. The oneharness provider (default)

[`oneharness`](https://github.com/nickderobertis/oneharness) (v0.3.7+) is a
prompt→text runner over many agentic harnesses (Claude Code, Codex, OpenCode,
Cursor, …). skilltest's `OneharnessProvider` wires five real oneharness features
into the runner:

- **`--system <text>`** — the skill's instructions are passed as a real system
  prompt (e.g. `--append-system-prompt` for claude-code), not inlined into the
  user turn.
- **`--resume <session>`** — for harnesses that support session continuation
  (`claude-code`, `opencode`, `cursor` today; see `oneharness list` and
  `supports_resume`), the runner threads the `session_id` returned on each turn
  into the next `respond` call, so the harness sees a real continuing
  conversation and keeps its tool state. For harnesses without resume support,
  skilltest falls back to inlining the full transcript on every turn.
- **`--events`** — normalized tool events (`{kind, name, input, output, index}`)
  lifted from each harness's transcript. skilltest attaches them to the assistant
  turn (`Message.events` in the [report](schema.md)) so consumers can inspect
  *what the skill did* — shell commands, file edits, tool uses — not just its
  final text. Empty for harnesses that expose no machine-readable transcript.
- **Normalized `usage`** — `{input_tokens, output_tokens, cost_usd}` is parsed
  off each result and aggregated into the [report](schema.md) so cross-model
  cost reporting is portable instead of harness-specific.
- **Normalized `failure_kind`** — when a run fails with a classified reason
  (`auth`, `rate_limit`, `model_not_found`, `quota`), the CLI maps it to a
  pointed hint instead of a generic provider error.
- **`--mock-rules` / `--spy-file`** — when a case declares `mocks`/`spy`,
  skilltest compiles the declarations into oneharness's mock ruleset, writes it
  to a temp file, and passes both flags on each skill turn (never on judge or
  simulated-user calls). oneharness installs its `mock` hook ephemerally for
  that one invocation (zero permanent config mutation), enforces the rules
  inside the harness, and appends every observed call to the spy JSONL, which
  skilltest parses into the report's `mock_calls`. A harness that cannot
  express a requested verb (e.g. rewrite on goose) is a loud oneharness usage
  error surfaced as a provider error. Requires oneharness v0.3.7+ (the release
  carrying the `mock` responder and `run --mock-rules`/`--spy-file`).

For each operation skilltest invokes:

```
oneharness run --harness <H> --model <M> --compact --events \
  --timeout <secs> --prompt-file - [--system <skill>] [--resume <session_id>]
```

with a constructed prompt on stdin, then reads `results[0]`: it requires
`status == "ok"` and uses `text`, `session_id`, `usage`, and `events`. A
non-`ok` status becomes a provider error (classified by `failure_kind` when set).

**Approval mode.** skilltest passes no `--mode`, so oneharness applies its own
default approval mode (v0.3.0+ normalized `--mode` across harnesses — a breaking
change from the pre-0.3 allow-everything behavior). To let the skill under test
take every action without prompting, set `bypass` through oneharness's own config
(`ONEHARNESS_MODE=bypass` or its config file), keeping approval policy in one
place rather than split between the two tools.

**Output format.** skilltest deliberately omits `--output-format`: oneharness
already requests each harness's default format and extracts the reply (and
events) accordingly; forcing `json` everywhere once broke the text-native
harnesses.

| op | harness / model | what skilltest passes |
| --- | --- | --- |
| `respond` | the platform + model under test | `--system <skill instructions>` + either the latest user message (when resuming) or the whole transcript (no-resume harnesses) |
| `user` | `judge_harness` + `judge_model` | the persona, the conversation, and "write only the user's next message" |
| `judge` | `judge_harness` + `judge_model` | the criterion + transcript, then "respond with ONLY `{\"value\": …, \"reason\": …}`" |

Two deliberate choices:

- **Evaluator independence.** `user` and `judge` run on a fixed `judge_harness`
  (default `claude-code`) and `judge_model`, *not* the harness/model under test,
  so the evaluator doesn't vary across the matrix.
- **Tolerant verdict parsing.** Real models don't always emit bare JSON, so the
  judge response is extracted from the first `{…}` in the text (code fences and
  surrounding prose are tolerated) and then type-checked against the eval kind.

Configure it (see `docs/schema.md`):

```yaml
provider:
  kind: oneharness
  bin: oneharness          # resolved on PATH
  judge_harness: claude-code
  timeout_secs: 120
```

`platforms` are oneharness harness ids; `models` must be valid for the chosen
harness (e.g. `sonnet`, `haiku`, or a full model id for `claude-code`).

## 2. The custom command protocol (JSON-lines)

The second backend speaks a small JSON-lines protocol and backs both the bundled
deterministic `skilltest-fake-provider` (which is how the gate runs without a
model) and any provider you write. skilltest spawns the command once per op,
writes **one** JSON request object to stdin (newline-terminated), and reads
**one** JSON response object from stdout; a non-zero exit (message on stderr) is a
provider error.

```yaml
provider:
  kind: command
  command: ["skilltest-fake-provider"]   # or ["python3", "my_provider.py"]
```

Every request has an `op` and a `messages` array (`{role, content}`).

**`respond`** — request carries `platform`, `model`, `skill` (`{name, path,
instructions}`), `messages`, and an optional `session` (a handle the runner
captured from a prior `respond` so a stateful provider can continue);
response: `{"message": "...", "done": false}`, plus optional `usage`
(`{input_tokens, output_tokens, cost_usd}`, all individually optional),
`session_id` (which the runner will pass back as `session` next turn), and
`events` — an optional array of normalized tool events (`{kind, name, input,
output, index}`, parallel to oneharness's) the runner attaches to the assistant
turn so consumers can inspect what the provider did.

When the run declares tool mocks/spies, the `respond` request additionally
carries a **`mocks`** block: `{"mocks": {"rules": <ruleset|null>}}`, where
`rules` is the compiled oneharness mock ruleset (`{"rules": [{"match": …,
"action": {"deny"|"rewrite"|"stub": …}}]}`, first match wins) or `null` for a
spy-only run. A provider that receives the block MUST apply the rules to its
tool calls and respond with **`mock_calls`**: an array (possibly empty) of
`{tool, input, action, rule}` records — `input` is the call's *original*
arguments, `action` the verdict applied (`allow`/`deny`/`rewrite`/`stub`), and
`rule` the intercepting ruleset index (`null` for `allow`). A response without
`mock_calls` despite a `mocks` block is a **provider error** — the runner
treats silent mock-ignoring as a failure, never a vacuous pass. Providers that
don't implement mocking simply keep omitting the field; skilltest only sends
the block when a case asks for it, and errs loudly if it goes unanswered. The
bundled fake provider implements the full flow (using the same decision engine
skilltest ships, `skilltest_core::mock::decide`) and is the reference.

**`user`** — request carries `model`, `persona`, `messages`; response:
`{"message": "...", "stop": false}`, plus optional `usage`.

**`judge`** — request carries `model`, `kind` (`"boolean"`/`"numeric"`),
`criterion`, `messages`, plus `min`/`max` for numeric; response:
`{"value": <bool|number>, "reason": "..."}`, plus optional `usage`. `value`
must be a boolean for boolean evals and a number for numeric; a mismatch is a
provider error.

`usage` and `session_id` are entirely optional — a stateless provider (like
the bundled fake) simply omits them and the report's usage totals stay empty.

A reference implementation is
[`crates/skilltest-cli/src/bin/fake_provider.rs`](../crates/skilltest-cli/src/bin/fake_provider.rs).

## 3. Streaming output (`run --format json-stream`)

Alongside the buffered `--format json` (one JSON `Report` at the end), the CLI
offers an opt-in **NDJSON stream** the SDKs consume to watch a run live and
**short-circuit** it. Each line is one JSON object:

```
{"type":"event","case":"…","platform":"…","model":"…","turn":1,"event":{…ToolEvent…}}
…one per tool event, the instant it is observed…
{"type":"result","report":{…Report…}}
```

The terminal `result` line carries the same `Report` the buffered format
returns. Under the hood the runner drives each turn through the provider's
streaming path (`OneharnessProvider` uses `oneharness run --stream`, which emits
its own NDJSON events); the buffered format is unchanged.

**Short-circuit.** A consumer that stops reading (closing the pipe) makes the
CLI's next event write fail; the CLI tears the run down — for `OneharnessProvider`
it kills the oneharness child, closing *its* stream so the harness is torn down,
and a bad turn is cut off instead of paid for in full. The SDKs expose this as an
async iterator (`stream_skill` / `streamSkill`): `break` out of the loop to abort;
read `.report` for the final report when the stream ran to completion.

## 4. The direct-API judge (`judge:` override)

By default the provider that runs the skill also judges (the oneharness
`judge_harness`). Every `judge`/`user` call then pays an agent-loop cold start
just to produce one short verdict. The optional `judge:` config swaps the judge
(and the simulated user) onto a **direct model API call** — one HTTP round trip,
no harness — while the base provider keeps running the skill under test. It is a
composition (`SplitProvider`): `respond` stays on the configured provider;
`judge` and `simulate_user` go to the API.

```yaml
provider:
  kind: oneharness          # the skill still runs on a real harness
judge:
  kind: api
  vendor: anthropic         # anthropic | openai
  # api_key_env: ANTHROPIC_API_KEY   # default per vendor (or OPENAI_API_KEY)
  # base_url: https://...            # optional (proxy / OpenAI-compatible gateway)
  # timeout_secs: 60
  # curl_bin: curl
  # strict_json: true                # structured-outputs verdict (default on)
judge_model: claude-opus-4-8   # MUST be a valid API model id for the vendor
```

Details and rationale:

- **Model.** The judge uses the run's `judge_model`; with `vendor: api` it must
  be a real API model id (`claude-opus-4-8`, `gpt-4o`, …), not an oneharness
  model alias. `platforms`/`models` (the skill under test) are unaffected.
- **Same prompts.** The judge and simulated-user prompts are identical to the
  oneharness path, so the two backends are directly comparable — only the
  transport and (for the judge) the output constraint differ.
- **Strict JSON (default on).** With `strict_json: true`, the judge verdict is
  constrained to the `{value, reason}` schema via the vendor's structured-outputs
  feature (Anthropic `output_config.format`, OpenAI `response_format:
  json_schema` with `strict: true`), so the reply is guaranteed parseable instead
  of scraped. The tolerant `{…}` extraction still runs as a backstop; set
  `strict_json: false` for a model/endpoint that doesn't support structured
  outputs. The simulated user is never schema-constrained.
- **Retries.** Transient API failures (rate limit, overload) are retried with
  exponential backoff before surfacing; auth/quota/not-found errors fail fast.
- **Transport.** The request is sent with `curl` (Rust has no official vendor
  SDK): Anthropic `POST /v1/messages`, OpenAI `POST /v1/chat/completions`. The
  API key is read from `api_key_env` at run time and passed through a private
  (`0600`) `curl` config file, so it never lands in `argv`/`ps` or on disk in
  config. Failures are classified (`auth`/`rate_limit`/`quota`/`model_not_found`)
  so the CLI gives the same pointed hints as a harness failure.
- **Usage.** Token usage (`input_tokens`/`output_tokens`) is parsed from the API
  response into the report's totals, so judge cost is visible. (Cost in USD is
  not reported by either API and stays empty.)

**Benchmarking the two paths.** Run the same case with and without the `judge:`
override and compare the JSON report's `usage` totals (and wall-clock, e.g.
`time skilltest run …`). The skill-running half is byte-for-byte identical, so
the delta is the judge backend.

> Why a default of harness judging at all: the harness judge works on
> *subscription* auth (e.g. claude-code with no API key) and can judge with any
> oneharness model. The API judge needs an API key, but on API-key auth it is
> faster and cheaper per verdict. Keep harness judging as the portable default;
> opt into the API judge where the keys exist and judge throughput matters.
