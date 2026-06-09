# Provider protocol

`skilltest` never talks to a model directly. It shells out to a **provider**
command (default `oneharness`) that speaks a small JSON-lines protocol. For each
operation skilltest spawns the command, writes **one** JSON request object to
stdin (followed by a newline), and reads **one** JSON response object from
stdout. The command exits 0 on success; any non-zero exit (with a message on
stderr) is surfaced as a provider error.

A reference implementation lives in
[`crates/skilltest-cli/src/bin/fake_provider.rs`](../crates/skilltest-cli/src/bin/fake_provider.rs)
(`skilltest-fake-provider`); it is deterministic and used by the e2e suites.

The provider command is configured as an argv vector (`provider:` in the config,
or `--provider` on the CLI). skilltest appends no arguments of its own — all
context travels in the request body — so any wrapper that reads stdin and writes
stdout works.

## Common request fields

Every request has an `op` field and a `messages` array (the conversation so
far). A message is `{ "role": "user" | "assistant" | "system", "content": string }`.

## `respond` — an assistant/skill turn

Run the skill (on the given platform/model) to produce the next assistant turn.

Request:

```json
{
  "op": "respond",
  "platform": "claude-code",
  "model": "claude-opus-4-8",
  "skill": { "name": "greeter", "path": "/abs/skills/greeter", "instructions": "<SKILL.md body>" },
  "messages": [{ "role": "user", "content": "Greet Dr. Smith." }]
}
```

Response:

```json
{ "message": "Hello, Dr. Smith!", "done": false }
```

`done` (optional, default `false`) lets the skill signal it considers the task
complete, ending a multi-turn conversation early.

## `user` — a simulated-user turn

Only used for multi-turn cases. Produce the next user turn, playing the persona.

Request:

```json
{
  "op": "user",
  "model": "claude-opus-4-8",
  "persona": "You are a terse patient confirming an appointment.",
  "messages": [ "...the conversation so far..." ]
}
```

Response:

```json
{ "message": "Yes, please go ahead.", "stop": false }
```

`stop` (optional, default `false`) lets the simulated user end the conversation.

## `judge` — a natural-language eval or done-check

Score a plain-English criterion against the transcript. Used for every eval and
for the multi-turn `done_when` check.

Boolean request/response:

```json
{ "op": "judge", "model": "claude-opus-4-8", "kind": "boolean", "criterion": "the reply greets Dr. Smith by name", "messages": [ ... ] }
```
```json
{ "value": true, "reason": "the reply addresses her by title and surname" }
```

Numeric request/response (the scale travels as `min`/`max`):

```json
{ "op": "judge", "model": "claude-opus-4-8", "kind": "numeric", "criterion": "how completely was the appointment confirmed", "min": 0, "max": 10, "messages": [ ... ] }
```
```json
{ "value": 8, "reason": "confirmed with date but not time" }
```

`value` must be a boolean for `kind: "boolean"` and a number for
`kind: "numeric"`; a mismatch is a provider error. `reason` is optional but
strongly encouraged — it is what appears in a failing report.
