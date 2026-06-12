//! The provider boundary. `skilltest` never talks to a model directly; a
//! [`Provider`] runs the skill, plays the simulated user, and judges the
//! transcript.
//!
//! There are two real implementations. [`OneharnessProvider`] (the default) runs
//! each prompt on a harness through the
//! [`oneharness`](https://github.com/nickderobertis/oneharness) CLI and parses
//! its JSON. [`CommandProvider`] speaks a small JSON-lines protocol (see
//! `docs/protocol.md`) and backs both the deterministic `skilltest-fake-provider`
//! used by the gate and any custom provider you write. The [`Provider`] trait
//! also lets the runner be unit-tested against an in-memory fake.

use std::io::Write as _;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::config::{ApiJudgeConfig, ApiVendor, OneharnessConfig};
use crate::conversation::{Message, Role};
use crate::error::{Error, Result};
use crate::eval::JudgeValue;

/// A borrowed view of the skill under test, as sent to the provider.
pub struct SkillRef<'a> {
    pub name: &'a str,
    pub dir: &'a str,
    pub instructions: &'a str,
}

/// The kind of judgement requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JudgeKind {
    Boolean,
    Numeric,
}

impl JudgeKind {
    fn as_str(self) -> &'static str {
        match self {
            JudgeKind::Boolean => "boolean",
            JudgeKind::Numeric => "numeric",
        }
    }
}

/// A judge query: the criterion, its kind, and (for numeric) the scale.
pub struct JudgeQuery<'a> {
    pub kind: JudgeKind,
    pub criterion: &'a str,
    pub scale: Option<(f64, f64)>,
}

/// Token / cost usage for one provider call.
///
/// Each field is independently optional because not every harness reports every
/// signal (cost is commonly absent on subscription auth; some harnesses report
/// no usage at all). The whole struct is `Option<Usage>` on a turn — `None`
/// means "no signal," not "zero."
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

impl Usage {
    /// True iff every field is `None`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.input_tokens.is_none() && self.output_tokens.is_none() && self.cost_usd.is_none()
    }

    /// Add another sample into this total. `None` values stay `None` until
    /// something reports a real number, at which point they accumulate.
    pub fn add(&mut self, other: &Usage) {
        if let Some(v) = other.input_tokens {
            self.input_tokens = Some(self.input_tokens.unwrap_or(0) + v);
        }
        if let Some(v) = other.output_tokens {
            self.output_tokens = Some(self.output_tokens.unwrap_or(0) + v);
        }
        if let Some(v) = other.cost_usd {
            self.cost_usd = Some(self.cost_usd.unwrap_or(0.0) + v);
        }
    }
}

/// An assistant/skill turn produced by the provider.
#[derive(Debug, Clone, Default)]
pub struct AssistantTurn {
    pub message: String,
    /// The skill signalled it considers the task complete.
    pub done: bool,
    /// Cost/token usage for this call, if the provider reported it.
    pub usage: Option<Usage>,
    /// A session handle the runner can pass back on the next `respond` call to
    /// continue the same conversation against the real harness (only some
    /// harnesses expose this — see `OneharnessProvider::supports_resume`).
    pub session_id: Option<String>,
}

/// A simulated-user turn produced by the provider.
#[derive(Debug, Clone, Default)]
pub struct UserTurn {
    pub message: String,
    /// The simulated user chose to end the conversation.
    pub stop: bool,
    pub usage: Option<Usage>,
}

/// A judge verdict: the raw value (bool or number) plus the stated reason.
#[derive(Debug, Clone)]
pub struct JudgeVerdict {
    pub value: JudgeValue,
    pub reason: String,
    pub usage: Option<Usage>,
}

/// The provider boundary.
pub trait Provider {
    /// Run one assistant/skill turn given the conversation so far. `session`,
    /// when `Some`, is a handle returned by a previous `respond` call on this
    /// run that the provider may use to continue the same harness session
    /// (e.g. via `oneharness run --resume`); providers that don't support
    /// continuation should ignore it.
    ///
    /// # Errors
    /// [`Error::Provider`] if the command fails or returns malformed output.
    fn respond(
        &self,
        platform: &str,
        model: &str,
        skill: &SkillRef<'_>,
        messages: &[Message],
        session: Option<&str>,
    ) -> Result<AssistantTurn>;

    /// Produce one simulated-user turn.
    ///
    /// # Errors
    /// [`Error::Provider`] if the command fails or returns malformed output.
    fn simulate_user(&self, model: &str, persona: &str, messages: &[Message]) -> Result<UserTurn>;

    /// Score a criterion against the conversation.
    ///
    /// # Errors
    /// [`Error::Provider`] if the command fails or returns malformed output.
    fn judge(
        &self,
        model: &str,
        query: &JudgeQuery<'_>,
        messages: &[Message],
    ) -> Result<JudgeVerdict>;

    /// True iff `respond` on `platform` will faithfully continue a prior
    /// session when given its `session_id`. The default is `false`; providers
    /// that support resume override this so the runner knows to thread the
    /// session id through.
    fn supports_resume(&self, _platform: &str) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Wire types (CommandProvider JSON-lines protocol)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct SkillPayload<'a> {
    name: &'a str,
    path: &'a str,
    instructions: &'a str,
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum Request<'a> {
    Respond {
        platform: &'a str,
        model: &'a str,
        skill: SkillPayload<'a>,
        messages: &'a [Message],
        #[serde(skip_serializing_if = "Option::is_none")]
        session: Option<&'a str>,
    },
    User {
        model: &'a str,
        persona: &'a str,
        messages: &'a [Message],
    },
    Judge {
        model: &'a str,
        kind: &'a str,
        criterion: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
        messages: &'a [Message],
    },
}

#[derive(Deserialize)]
struct RespondPayload {
    message: String,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    usage: Option<Usage>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Deserialize)]
struct UserPayload {
    message: String,
    #[serde(default)]
    stop: bool,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct JudgePayload {
    value: JudgeValue,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    usage: Option<Usage>,
}

// ---------------------------------------------------------------------------
// CommandProvider
// ---------------------------------------------------------------------------

/// A [`Provider`] backed by an external command speaking the JSON protocol.
pub struct CommandProvider {
    argv: Vec<String>,
}

impl CommandProvider {
    /// Build a provider from an argv vector (program + args). The program is
    /// resolved on `PATH`.
    ///
    /// # Errors
    /// [`Error::Invalid`] if `argv` is empty.
    pub fn new(argv: Vec<String>) -> Result<Self> {
        if argv.is_empty() {
            return Err(Error::Invalid("provider command is empty".into()));
        }
        Ok(Self { argv })
    }

    /// Send one request and parse the single response object from stdout.
    fn call<T: for<'de> Deserialize<'de>>(&self, request: &Request<'_>, op: &str) -> Result<T> {
        let payload = serde_json::to_vec(request).map_err(|e| {
            Error::provider(op.to_string(), format!("could not encode request: {e}"))
        })?;

        let mut child = Command::new(&self.argv[0])
            .args(&self.argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                Error::provider(
                    op.to_string(),
                    format!(
                        "could not run provider `{}`: {e}. Is it installed and on PATH?",
                        self.argv[0]
                    ),
                )
            })?;

        // Write the request, then close stdin so the child can finish. Writing
        // before reading stdout is safe here because responses are small.
        {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| Error::provider(op.to_string(), "could not open provider stdin"))?;
            stdin
                .write_all(&payload)
                .and_then(|()| stdin.write_all(b"\n"))
                .map_err(|e| {
                    Error::provider(op.to_string(), format!("could not write request: {e}"))
                })?;
        }

        let output = child.wait_with_output().map_err(|e| {
            Error::provider(op.to_string(), format!("provider did not complete: {e}"))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::provider(
                op.to_string(),
                format!("provider exited with {}: {}", output.status, stderr.trim()),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let line = stdout.trim();
        if line.is_empty() {
            return Err(Error::provider(
                op.to_string(),
                "provider produced no output (expected one JSON response object)",
            ));
        }
        serde_json::from_str(line).map_err(|e| {
            Error::provider(
                op.to_string(),
                format!("provider response was not valid JSON for `{op}`: {e}; got: {line}"),
            )
        })
    }
}

impl Provider for CommandProvider {
    fn respond(
        &self,
        platform: &str,
        model: &str,
        skill: &SkillRef<'_>,
        messages: &[Message],
        session: Option<&str>,
    ) -> Result<AssistantTurn> {
        let request = Request::Respond {
            platform,
            model,
            skill: SkillPayload {
                name: skill.name,
                path: skill.dir,
                instructions: skill.instructions,
            },
            messages,
            session,
        };
        let payload: RespondPayload = self.call(&request, "respond")?;
        Ok(AssistantTurn {
            message: payload.message,
            done: payload.done,
            usage: payload.usage,
            session_id: payload.session_id,
        })
    }

    fn simulate_user(&self, model: &str, persona: &str, messages: &[Message]) -> Result<UserTurn> {
        let request = Request::User {
            model,
            persona,
            messages,
        };
        let payload: UserPayload = self.call(&request, "user")?;
        Ok(UserTurn {
            message: payload.message,
            stop: payload.stop,
            usage: payload.usage,
        })
    }

    fn judge(
        &self,
        model: &str,
        query: &JudgeQuery<'_>,
        messages: &[Message],
    ) -> Result<JudgeVerdict> {
        let (min, max) = match query.scale {
            Some((lo, hi)) => (Some(lo), Some(hi)),
            None => (None, None),
        };
        let request = Request::Judge {
            model,
            kind: query.kind.as_str(),
            criterion: query.criterion,
            min,
            max,
            messages,
        };
        let payload: JudgePayload = self.call(&request, "judge")?;
        Ok(JudgeVerdict {
            value: payload.value,
            reason: payload.reason,
            usage: payload.usage,
        })
    }
}

// ---------------------------------------------------------------------------
// OneharnessProvider
// ---------------------------------------------------------------------------

/// The default [`Provider`]: runs each prompt on a harness through the
/// `oneharness` CLI.
///
/// Wires four real oneharness features that ship in v0.2.0:
///
/// * `--system <skill instructions>` — the skill becomes a *real* system prompt
///   on the underlying harness (e.g. `--append-system-prompt` for claude-code),
///   instead of being inlined into the user message.
/// * `--resume <session>` — multi-turn `respond` calls thread the previous
///   `session_id` so the harness sees a continuing conversation (and keeps its
///   tool state, files, etc.) instead of being re-prompted with a stringified
///   transcript. Used only for harnesses that report `supports_resume` in the
///   registry (claude-code, opencode, cursor today); other harnesses fall back
///   to the inline-transcript path.
/// * Normalized `usage` (`input_tokens`, `output_tokens`, `cost_usd`) — surfaced
///   on every turn so cross-model cost reporting is portable.
/// * Normalized `failure_kind` (`auth`, `rate_limit`, `model_not_found`, …) —
///   classified provider errors so the CLI can distinguish a broken environment
///   from a broken skill.
///
/// Evals and the simulated user always run on the configured `judge_harness`,
/// independent of the harness under test, so the evaluator does not drift with
/// the matrix.
pub struct OneharnessProvider {
    bin: String,
    judge_harness: String,
    timeout_secs: u64,
}

/// The subset of the `oneharness run` JSON envelope we consume.
#[derive(Deserialize)]
struct OhEnvelope {
    results: Vec<OhResult>,
}

#[derive(Deserialize)]
struct OhResult {
    status: String,
    #[serde(default)]
    text: Option<String>,
    /// Raw harness stdout. oneharness's `text` extraction is best-effort and may
    /// be null when a harness's output shape defeats it, with stdout as the
    /// documented fallback; we honor that rather than hard-failing. No harness in
    /// the live matrix relies on it today (OpenCode's JSONL — the case that
    /// motivated this — is extracted natively as of oneharness v0.2.37), but the
    /// contract holds for any harness, so the fallback stays as defense-in-depth.
    #[serde(default)]
    stdout: String,
    #[serde(default)]
    stderr: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    usage: Option<Usage>,
    #[serde(default)]
    failure_kind: Option<String>,
}

/// Parameters for one `oneharness run` invocation.
struct RunArgs<'a> {
    harness: &'a str,
    model: &'a str,
    prompt: &'a str,
    /// Becomes `--system <text>`; only set on `respond` so the skill is the
    /// system prompt rather than inlined into the user turn.
    system: Option<&'a str>,
    /// Becomes `--resume <id>`; only set when the runner wants to continue a
    /// prior harness session.
    resume: Option<&'a str>,
}

/// What we get back from one `oneharness run`.
struct RunOutcome {
    text: String,
    session_id: Option<String>,
    usage: Option<Usage>,
}

/// Choose the harness's reply text: oneharness's extracted `text` when non-empty,
/// otherwise its raw stdout. oneharness extracts `text` on a best-effort basis
/// and, per its contract, may leave it null when a harness's output shape defeats
/// extraction — the reply still survives in stdout. (OpenCode's JSONL once hit
/// this; oneharness v0.2.37 extracts it natively, so the fallback is now
/// defense-in-depth.) Returns `None` only when both are empty, the one case that
/// is a genuine "the harness said nothing" error.
fn select_reply_text(text: Option<String>, stdout: &str) -> Option<String> {
    text.filter(|t| !t.trim().is_empty())
        .or_else(|| (!stdout.trim().is_empty()).then(|| stdout.to_string()))
}

impl OneharnessProvider {
    /// Build a provider from its configuration.
    #[must_use]
    pub fn new(config: &OneharnessConfig) -> Self {
        Self {
            bin: config.bin.clone(),
            judge_harness: config.judge_harness.clone(),
            timeout_secs: config.timeout_secs,
        }
    }

    /// Run one prompt on `harness` and return the normalized text plus the
    /// session id and usage (when oneharness lifted them from the harness's
    /// output).
    fn run(&self, args: &RunArgs<'_>) -> Result<RunOutcome> {
        let timeout = self.timeout_secs.to_string();
        let mut cmd = Command::new(&self.bin);
        // Intentionally no `--output-format` override: oneharness already requests
        // each harness's *default* format (json for claude-code/opencode,
        // stream-json for cursor, text for codex/goose/qwen/crush/copilot) and
        // extracts the reply accordingly. Forcing `json` everywhere broke the
        // text-native harnesses — oneharness would json-extract their plain-text
        // reply and find nothing ("harness produced no extractable text").
        cmd.args([
            "run",
            "--harness",
            args.harness,
            "--compact",
            "--timeout",
            &timeout,
            "--prompt-file",
            "-",
        ]);
        // An empty model means "unspecified" — omit `--model` so the harness uses
        // its own default (cursor/crush/copilot) or an env-selected model (qwen
        // via OPENAI_MODEL, goose via GOOSE_MODEL), exactly as oneharness's own
        // smoke scripts do. Forwarding `--model ""` would push a broken empty
        // model flag to the harness CLI.
        if !args.model.is_empty() {
            cmd.args(["--model", args.model]);
        }
        if let Some(system) = args.system {
            cmd.args(["--system", system]);
        }
        if let Some(resume) = args.resume {
            cmd.args(["--resume", resume]);
        }

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                Error::provider(
                    "oneharness",
                    format!(
                        "could not run `{}`: {e}. Is oneharness installed and on PATH?",
                        self.bin
                    ),
                )
            })?;

        child
            .stdin
            .as_mut()
            .ok_or_else(|| Error::provider("oneharness", "could not open oneharness stdin"))?
            .write_all(args.prompt.as_bytes())
            .map_err(|e| Error::provider("oneharness", format!("could not write prompt: {e}")))?;

        let output = child.wait_with_output().map_err(|e| {
            Error::provider("oneharness", format!("oneharness did not complete: {e}"))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let envelope: OhEnvelope = serde_json::from_str(stdout.trim()).map_err(|e| {
            Error::provider(
                "oneharness",
                format!(
                    "could not parse oneharness output: {e}; stderr: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            )
        })?;

        let result = envelope
            .results
            .into_iter()
            .next()
            .ok_or_else(|| Error::provider("oneharness", "oneharness returned no results"))?;

        if result.status != "ok" {
            let detail = result
                .error
                .filter(|e| !e.is_empty())
                .or_else(|| Some(result.stderr.clone()).filter(|s| !s.is_empty()))
                .unwrap_or_else(|| format!("status `{}`", result.status));
            let context = format!("oneharness:{}", args.harness);
            let message = format!("harness run failed: {detail}");
            return Err(match result.failure_kind {
                Some(kind) if !kind.is_empty() => {
                    Error::provider_classified(context, message, kind)
                }
                _ => Error::provider(context, message),
            });
        }

        // Prefer oneharness's extracted `text`; fall back to raw stdout when a
        // harness's output shape defeats extraction (oneharness's documented
        // contract — see OhResult::stdout). Only a run that produced *neither* is
        // a real error.
        let text = select_reply_text(result.text, &result.stdout).ok_or_else(|| {
            Error::provider(
                format!("oneharness:{}", args.harness),
                "harness produced neither extractable text nor stdout",
            )
        })?;
        Ok(RunOutcome {
            text,
            session_id: result.session_id,
            usage: result.usage,
        })
    }
}

impl Provider for OneharnessProvider {
    fn respond(
        &self,
        platform: &str,
        model: &str,
        skill: &SkillRef<'_>,
        messages: &[Message],
        session: Option<&str>,
    ) -> Result<AssistantTurn> {
        // If we have a real session to continue on a supporting harness, only
        // send the last user message — the harness still has its prior state.
        // Otherwise inline the whole transcript so harnesses without resume
        // still see the conversation.
        let prompt = if session.is_some() {
            latest_user_message(messages).unwrap_or_default()
        } else {
            render_transcript_for_respond(messages)
        };
        let outcome = self.run(&RunArgs {
            harness: platform,
            model,
            prompt: &prompt,
            system: Some(skill.instructions),
            resume: session,
        })?;
        Ok(AssistantTurn {
            message: outcome.text.trim().to_string(),
            done: false,
            usage: outcome.usage,
            session_id: outcome.session_id,
        })
    }

    fn simulate_user(&self, model: &str, persona: &str, messages: &[Message]) -> Result<UserTurn> {
        let prompt = build_user_prompt(persona, messages);
        let outcome = self.run(&RunArgs {
            harness: &self.judge_harness,
            model,
            prompt: &prompt,
            system: None,
            resume: None,
        })?;
        Ok(UserTurn {
            message: outcome.text.trim().to_string(),
            stop: false,
            usage: outcome.usage,
        })
    }

    fn judge(
        &self,
        model: &str,
        query: &JudgeQuery<'_>,
        messages: &[Message],
    ) -> Result<JudgeVerdict> {
        let prompt = build_judge_prompt(query, messages);
        let outcome = self.run(&RunArgs {
            harness: &self.judge_harness,
            model,
            prompt: &prompt,
            system: None,
            resume: None,
        })?;
        let mut verdict = parse_verdict(query.kind, &outcome.text)?;
        verdict.usage = outcome.usage;
        Ok(verdict)
    }

    fn supports_resume(&self, platform: &str) -> bool {
        supports_resume(platform)
    }
}

/// The harnesses oneharness's adapter table marks `supports_resume = true`
/// (claude-code's `--resume`, opencode's `--session`, cursor's `--resume`). Kept
/// in sync with the `oneharness list` registry — when a new harness ships
/// session continuation, add it here so the runner threads `session_id`.
#[must_use]
pub fn supports_resume(harness: &str) -> bool {
    matches!(harness, "claude-code" | "opencode" | "cursor")
}

// ---------------------------------------------------------------------------
// ApiJudgeProvider + SplitProvider
// ---------------------------------------------------------------------------

/// A judge-only [`Provider`] that scores evals and plays the simulated user with
/// a *direct* model API call (Anthropic or OpenAI), rather than running them
/// through a harness.
///
/// Why this exists: routing the judge through a full agentic harness pays an
/// agent-loop cold start on every short verdict. A direct API call is one HTTP
/// round trip — faster and cheaper on API-key auth — and still reuses the exact
/// same judge/user prompts and tolerant verdict parsing as
/// [`OneharnessProvider`], so the two are directly comparable.
///
/// It does not run skills: `respond` returns an error. Compose it with a
/// skill-running provider via [`SplitProvider`] so the harness under test still
/// drives `respond`, while the judge runs on the API.
///
/// The request is sent with `curl` (Rust has no official vendor SDK). The API
/// key is read from an env var and passed through a private (`0600`) `curl`
/// config file, so it never appears in `argv` / `ps`.
pub struct ApiJudgeProvider {
    vendor: ApiVendor,
    api_key_env: String,
    endpoint: String,
    timeout_secs: u64,
    curl_bin: String,
}

/// One model reply plus the usage the API reported for it.
#[derive(Debug)]
struct ChatOutcome {
    text: String,
    usage: Option<Usage>,
}

/// A minimal system prompt; the full judge / user-simulation instructions live
/// in the shared prompt builders, so this stays identical across vendors.
const JUDGE_SYSTEM: &str =
    "Follow the user's instructions exactly and respond with only what they ask for.";

impl ApiJudgeProvider {
    /// Build a provider from its configuration, resolving per-vendor defaults
    /// for the API-key env var and endpoint.
    #[must_use]
    pub fn new(config: &ApiJudgeConfig) -> Self {
        let api_key_env = config
            .api_key_env
            .clone()
            .unwrap_or_else(|| match config.vendor {
                ApiVendor::Anthropic => "ANTHROPIC_API_KEY".to_string(),
                ApiVendor::Openai => "OPENAI_API_KEY".to_string(),
            });
        let endpoint = config
            .base_url
            .clone()
            .unwrap_or_else(|| match config.vendor {
                ApiVendor::Anthropic => "https://api.anthropic.com/v1/messages".to_string(),
                ApiVendor::Openai => "https://api.openai.com/v1/chat/completions".to_string(),
            });
        Self {
            vendor: config.vendor,
            api_key_env,
            endpoint,
            timeout_secs: config.timeout_secs,
            curl_bin: config.curl_bin.clone(),
        }
    }

    /// One chat round trip: build the vendor request, POST it, parse the reply.
    fn chat(&self, model: &str, system: &str, user: &str) -> Result<ChatOutcome> {
        let key = std::env::var(&self.api_key_env).map_err(|_| {
            Error::provider_classified(
                "api-judge",
                format!("API key env var `{}` is not set", self.api_key_env),
                "auth",
            )
        })?;
        let body = build_chat_body(self.vendor, model, system, user);
        let payload = serde_json::to_vec(&body)
            .map_err(|e| Error::provider("api-judge", format!("could not encode request: {e}")))?;
        let raw = self.run_curl(&key, &payload)?;
        parse_chat_response(self.vendor, &raw)
    }

    /// Per-vendor request headers.
    fn headers(&self, key: &str) -> Vec<(String, String)> {
        match self.vendor {
            ApiVendor::Anthropic => vec![
                ("x-api-key".to_string(), key.to_string()),
                ("anthropic-version".to_string(), "2023-06-01".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
            ],
            ApiVendor::Openai => vec![
                ("authorization".to_string(), format!("Bearer {key}")),
                ("content-type".to_string(), "application/json".to_string()),
            ],
        }
    }

    /// POST `body` via `curl`, with the URL + headers (including the API key) in
    /// a private config file so the key stays out of `argv`. Returns stdout.
    fn run_curl(&self, key: &str, body: &[u8]) -> Result<String> {
        let path = std::env::temp_dir().join(format!(
            "skilltest-judge-{}-{}.cfg",
            std::process::id(),
            curl_config_nonce()
        ));
        write_curl_config(&path, &self.endpoint, &self.headers(key), self.timeout_secs)?;
        let outcome = self.exec_curl(&path, body);
        // The key-bearing config is needed only for this one invocation.
        let _ = std::fs::remove_file(&path);
        outcome
    }

    fn exec_curl(&self, config_path: &std::path::Path, body: &[u8]) -> Result<String> {
        let mut child = Command::new(&self.curl_bin)
            .arg("--config")
            .arg(config_path)
            .arg("--data-binary")
            .arg("@-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                Error::provider(
                    "api-judge",
                    format!(
                        "could not run `{}`: {e}. Is curl installed and on PATH?",
                        self.curl_bin
                    ),
                )
            })?;

        child
            .stdin
            .as_mut()
            .ok_or_else(|| Error::provider("api-judge", "could not open curl stdin"))?
            .write_all(body)
            .map_err(|e| Error::provider("api-judge", format!("could not write request: {e}")))?;

        let output = child
            .wait_with_output()
            .map_err(|e| Error::provider("api-judge", format!("curl did not complete: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::provider(
                "api-judge",
                format!("curl failed ({}): {}", output.status, stderr.trim()),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl Provider for ApiJudgeProvider {
    fn respond(
        &self,
        _platform: &str,
        _model: &str,
        _skill: &SkillRef<'_>,
        _messages: &[Message],
        _session: Option<&str>,
    ) -> Result<AssistantTurn> {
        Err(Error::provider(
            "api-judge",
            "the API judge does not run skills; use it as the judge in a SplitProvider",
        ))
    }

    fn simulate_user(&self, model: &str, persona: &str, messages: &[Message]) -> Result<UserTurn> {
        let prompt = build_user_prompt(persona, messages);
        let outcome = self.chat(model, JUDGE_SYSTEM, &prompt)?;
        Ok(UserTurn {
            message: outcome.text.trim().to_string(),
            stop: false,
            usage: outcome.usage,
        })
    }

    fn judge(
        &self,
        model: &str,
        query: &JudgeQuery<'_>,
        messages: &[Message],
    ) -> Result<JudgeVerdict> {
        let prompt = build_judge_prompt(query, messages);
        let outcome = self.chat(model, JUDGE_SYSTEM, &prompt)?;
        let mut verdict = parse_verdict(query.kind, &outcome.text)?;
        verdict.usage = outcome.usage;
        Ok(verdict)
    }
}

/// A [`Provider`] that runs skills with one provider and judges with another:
/// `respond` (and `supports_resume`) go to the skill-running provider; `judge`
/// and `simulate_user` go to the judge. This keeps harness fidelity for the
/// thing under test while letting the judge run on a fast, cheap, deterministic
/// backend (typically [`ApiJudgeProvider`]).
pub struct SplitProvider {
    responder: Box<dyn Provider>,
    judge: ApiJudgeProvider,
}

impl SplitProvider {
    /// Compose a skill-running `responder` with an API `judge`.
    #[must_use]
    pub fn new(responder: Box<dyn Provider>, judge: ApiJudgeProvider) -> Self {
        Self { responder, judge }
    }
}

impl Provider for SplitProvider {
    fn respond(
        &self,
        platform: &str,
        model: &str,
        skill: &SkillRef<'_>,
        messages: &[Message],
        session: Option<&str>,
    ) -> Result<AssistantTurn> {
        self.responder
            .respond(platform, model, skill, messages, session)
    }

    fn simulate_user(&self, model: &str, persona: &str, messages: &[Message]) -> Result<UserTurn> {
        self.judge.simulate_user(model, persona, messages)
    }

    fn judge(
        &self,
        model: &str,
        query: &JudgeQuery<'_>,
        messages: &[Message],
    ) -> Result<JudgeVerdict> {
        self.judge.judge(model, query, messages)
    }

    fn supports_resume(&self, platform: &str) -> bool {
        self.responder.supports_resume(platform)
    }
}

/// A process-local monotonic counter, combined with the pid to make a unique
/// temp-file name for each concurrent `curl` config.
fn curl_config_nonce() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Escape a value for a double-quoted `curl` config entry.
fn curl_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Write a `curl` config file (`0600` on Unix) carrying the URL, headers, and
/// timeout. The request body is streamed separately on stdin (`--data-binary
/// @-`), so it never needs escaping into this file.
fn write_curl_config(
    path: &std::path::Path,
    url: &str,
    headers: &[(String, String)],
    timeout_secs: u64,
) -> Result<()> {
    let mut config = String::new();
    config.push_str(&format!("url = \"{}\"\n", curl_escape(url)));
    config.push_str("request = \"POST\"\n");
    for (name, value) in headers {
        config.push_str(&format!("header = \"{}: {}\"\n", name, curl_escape(value)));
    }
    config.push_str(&format!("max-time = {timeout_secs}\n"));
    config.push_str("silent\nshow-error\n");

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| Error::provider("api-judge", format!("could not write curl config: {e}")))?;
    file.write_all(config.as_bytes())
        .map_err(|e| Error::provider("api-judge", format!("could not write curl config: {e}")))?;
    Ok(())
}

/// Build the JSON request body for one chat completion. Outgoing data, so it is
/// constructed directly; responses are parsed into typed models below.
fn build_chat_body(vendor: ApiVendor, model: &str, system: &str, user: &str) -> serde_json::Value {
    match vendor {
        ApiVendor::Anthropic => serde_json::json!({
            "model": model,
            "max_tokens": 1024,
            "system": system,
            "messages": [{ "role": "user", "content": user }],
        }),
        ApiVendor::Openai => serde_json::json!({
            "model": model,
            "max_tokens": 1024,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
        }),
    }
}

// Typed views of the vendor responses (trust-boundary input — always parsed,
// never string-matched).

#[derive(Deserialize)]
struct ApiErrorBody {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<AnthropicBlock>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    error: Option<ApiErrorBody>,
}

#[derive(Deserialize)]
struct OpenAiMessage {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    #[serde(default)]
    message: Option<OpenAiMessage>,
}

#[derive(Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
    #[serde(default)]
    error: Option<ApiErrorBody>,
}

/// Map a vendor error `type` onto skilltest's classified provider-error kinds so
/// the CLI can give the same pointed hints it gives for harness failures.
fn classify_api_error(kind: Option<&str>) -> Option<String> {
    match kind? {
        "authentication_error" | "invalid_api_key" | "permission_error" => Some("auth".to_string()),
        "rate_limit_error" | "rate_limit_exceeded" => Some("rate_limit".to_string()),
        "insufficient_quota" | "billing_error" => Some("quota".to_string()),
        "not_found_error" => Some("model_not_found".to_string()),
        _ => None,
    }
}

fn api_error(err: ApiErrorBody) -> Error {
    let message = err
        .message
        .unwrap_or_else(|| "API returned an error".to_string());
    match classify_api_error(err.kind.as_deref()) {
        Some(kind) => Error::provider_classified("api-judge", message, kind),
        None => Error::provider("api-judge", message),
    }
}

/// Take the first chars of `raw` for an error message, on a UTF-8 boundary.
fn truncate_for_error(raw: &str) -> String {
    raw.chars().take(500).collect()
}

/// Parse a vendor chat response into the reply text plus normalized usage.
fn parse_chat_response(vendor: ApiVendor, raw: &str) -> Result<ChatOutcome> {
    match vendor {
        ApiVendor::Anthropic => {
            let resp: AnthropicResponse = serde_json::from_str(raw.trim()).map_err(|e| {
                Error::provider(
                    "api-judge",
                    format!(
                        "could not parse API response: {e}; got: {}",
                        truncate_for_error(raw)
                    ),
                )
            })?;
            if let Some(err) = resp.error {
                return Err(api_error(err));
            }
            let text = resp
                .content
                .iter()
                .filter(|b| b.kind == "text")
                .filter_map(|b| b.text.as_deref())
                .collect::<String>();
            if text.trim().is_empty() {
                return Err(Error::provider(
                    "api-judge",
                    format!(
                        "judge returned no text (stop_reason: {:?})",
                        resp.stop_reason
                    ),
                ));
            }
            let usage = resp.usage.map(|u| Usage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cost_usd: None,
            });
            Ok(ChatOutcome { text, usage })
        }
        ApiVendor::Openai => {
            let resp: OpenAiResponse = serde_json::from_str(raw.trim()).map_err(|e| {
                Error::provider(
                    "api-judge",
                    format!(
                        "could not parse API response: {e}; got: {}",
                        truncate_for_error(raw)
                    ),
                )
            })?;
            if let Some(err) = resp.error {
                return Err(api_error(err));
            }
            let text = resp
                .choices
                .into_iter()
                .next()
                .and_then(|c| c.message)
                .and_then(|m| m.content)
                .unwrap_or_default();
            if text.trim().is_empty() {
                return Err(Error::provider("api-judge", "judge returned no text"));
            }
            let usage = resp.usage.map(|u| Usage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
                cost_usd: None,
            });
            Ok(ChatOutcome { text, usage })
        }
    }
}

/// Render the conversation as `Role: content` lines for inlining in a prompt.
/// Used by the judge, the simulated user, and the no-resume fallback path of
/// `respond`.
fn render_transcript(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
                Role::System => "System",
            };
            format!("{role}: {}", m.content)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The prompt for `respond` when we cannot resume a harness session: inline the
/// whole conversation so the stateless harness call sees it. The skill is
/// passed separately as `--system`, so it does *not* appear here.
fn render_transcript_for_respond(messages: &[Message]) -> String {
    format!(
        "Conversation so far (most recent last):\n{}\n\n\
         Write only the assistant's next reply, following your system \
         instructions. Output the reply text and nothing else.",
        render_transcript(messages),
    )
}

/// The most recent user message in the transcript — used as the next-turn
/// prompt when resuming a real harness session.
fn latest_user_message(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|m| m.role == Role::User)
        .map(|m| m.content.clone())
}

fn build_user_prompt(persona: &str, messages: &[Message]) -> String {
    format!(
        "You are role-playing the USER in a conversation with an AI assistant. \
         Stay in character:\n\n{persona}\n\n\
         Conversation so far (most recent last):\n{transcript}\n\n\
         Write only the user's next message. Output the message text and nothing \
         else.",
        transcript = render_transcript(messages),
    )
}

fn build_judge_prompt(query: &JudgeQuery<'_>, messages: &[Message]) -> String {
    let transcript = render_transcript(messages);
    match query.kind {
        JudgeKind::Boolean => format!(
            "You are a strict, careful evaluator of an AI assistant's behavior.\n\n\
             Criterion: {criterion}\n\n\
             Transcript:\n{transcript}\n\n\
             Decide whether the criterion is satisfied. Respond with ONLY a \
             single-line JSON object and nothing else:\n\
             {{\"value\": true or false, \"reason\": \"<one short sentence>\"}}",
            criterion = query.criterion,
        ),
        JudgeKind::Numeric => {
            let (min, max) = query.scale.unwrap_or((0.0, 10.0));
            format!(
                "You are a strict, careful evaluator of an AI assistant's behavior.\n\n\
                 Criterion: {criterion}\n\n\
                 Transcript:\n{transcript}\n\n\
                 Score how well the criterion is satisfied on a scale from {min} to \
                 {max} (inclusive). Respond with ONLY a single-line JSON object and \
                 nothing else:\n\
                 {{\"value\": <number between {min} and {max}>, \"reason\": \"<one short sentence>\"}}",
                criterion = query.criterion,
            )
        }
    }
}

/// Extract the first JSON object from `text`, tolerating code fences and prose
/// around it (real models do not always emit bare JSON).
fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end > start {
        Some(&text[start..=end])
    } else {
        None
    }
}

fn parse_verdict(kind: JudgeKind, text: &str) -> Result<JudgeVerdict> {
    let json = extract_json_object(text).ok_or_else(|| {
        Error::provider(
            "oneharness:judge",
            format!("judge did not return a JSON object; got: {text}"),
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| {
        Error::provider(
            "oneharness:judge",
            format!("judge verdict was not valid JSON: {e}; got: {json}"),
        )
    })?;
    let reason = value
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let raw = value
        .get("value")
        .ok_or_else(|| Error::provider("oneharness:judge", "judge verdict has no `value` field"))?;

    let verdict_value = match kind {
        JudgeKind::Boolean => JudgeValue::Bool(raw.as_bool().ok_or_else(|| {
            Error::provider(
                "oneharness:judge",
                format!("boolean judge `value` was not a bool: {raw}"),
            )
        })?),
        JudgeKind::Numeric => JudgeValue::Number(raw.as_f64().ok_or_else(|| {
            Error::provider(
                "oneharness:judge",
                format!("numeric judge `value` was not a number: {raw}"),
            )
        })?),
    };

    Ok(JudgeVerdict {
        value: verdict_value,
        reason,
        usage: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_argv_is_rejected() {
        assert!(CommandProvider::new(vec![]).is_err());
    }

    #[test]
    fn request_serializes_with_op_tag() {
        let req = Request::Judge {
            model: "m",
            kind: "numeric",
            criterion: "polite",
            min: Some(0.0),
            max: Some(10.0),
            messages: &[],
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"op\":\"judge\""));
        assert!(json.contains("\"kind\":\"numeric\""));
    }

    #[test]
    fn respond_no_session_inlines_transcript_but_not_skill() {
        // The skill is passed via --system now, so the prompt the harness sees
        // for respond carries only the transcript.
        let messages = [
            Message::user("Hi"),
            Message::assistant("Hello"),
            Message::user("Again?"),
        ];
        let prompt = render_transcript_for_respond(&messages);
        assert!(prompt.contains("User: Hi"));
        assert!(prompt.contains("Assistant: Hello"));
        assert!(prompt.contains("User: Again?"));
        // The skill body must not leak here — it belongs in --system.
        assert!(!prompt.contains("SKILL"));
    }

    #[test]
    fn respond_with_session_sends_only_latest_user_message() {
        let messages = [
            Message::user("Hi"),
            Message::assistant("Hello"),
            Message::user("Again?"),
        ];
        assert_eq!(latest_user_message(&messages).as_deref(), Some("Again?"));
    }

    #[test]
    fn extracts_json_from_fenced_or_prose_text() {
        assert_eq!(
            extract_json_object("```json\n{\"value\": true}\n```"),
            Some("{\"value\": true}")
        );
        assert_eq!(
            extract_json_object("Sure! {\"value\": 8, \"reason\": \"x\"} done"),
            Some("{\"value\": 8, \"reason\": \"x\"}")
        );
        assert_eq!(extract_json_object("no json here"), None);
    }

    #[test]
    fn parses_boolean_and_numeric_verdicts() {
        let b = parse_verdict(JudgeKind::Boolean, "{\"value\": true, \"reason\": \"ok\"}").unwrap();
        assert!(matches!(b.value, JudgeValue::Bool(true)));
        assert_eq!(b.reason, "ok");

        let n =
            parse_verdict(JudgeKind::Numeric, "{\"value\": 8.5, \"reason\": \"good\"}").unwrap();
        assert!(matches!(n.value, JudgeValue::Number(v) if (v - 8.5).abs() < f64::EPSILON));
    }

    #[test]
    fn verdict_with_wrong_value_type_errors() {
        assert!(parse_verdict(JudgeKind::Boolean, "{\"value\": 3}").is_err());
        assert!(parse_verdict(JudgeKind::Numeric, "{\"value\": true}").is_err());
        assert!(parse_verdict(JudgeKind::Boolean, "no json").is_err());
    }

    #[test]
    fn usage_accumulates_independently_per_field() {
        let mut total = Usage::default();
        total.add(&Usage {
            input_tokens: Some(10),
            output_tokens: None,
            cost_usd: Some(0.01),
        });
        total.add(&Usage {
            input_tokens: Some(5),
            output_tokens: Some(3),
            cost_usd: None,
        });
        assert_eq!(total.input_tokens, Some(15));
        assert_eq!(total.output_tokens, Some(3));
        assert!((total.cost_usd.unwrap() - 0.01).abs() < f64::EPSILON);
        assert!(!total.is_empty());
    }

    #[test]
    fn reply_text_prefers_extracted_then_falls_back_to_stdout() {
        // Extracted text wins when present.
        assert_eq!(
            select_reply_text(Some("clean reply".into()), "raw noise"),
            Some("clean reply".into())
        );
        // Null/blank extracted text falls back to raw stdout (the contract's
        // escape hatch when oneharness can't extract but the reply is in stdout).
        assert_eq!(
            select_reply_text(None, "{\"type\":\"text\",\"part\":{\"text\":\"pong\"}}"),
            Some("{\"type\":\"text\",\"part\":{\"text\":\"pong\"}}".into())
        );
        assert_eq!(
            select_reply_text(Some("   ".into()), "fallback"),
            Some("fallback".into())
        );
        // Neither present is the only real error.
        assert_eq!(select_reply_text(None, "   \n"), None);
        assert_eq!(select_reply_text(Some(String::new()), ""), None);
    }

    #[test]
    fn supports_resume_covers_known_harnesses() {
        assert!(supports_resume("claude-code"));
        assert!(supports_resume("opencode"));
        assert!(supports_resume("cursor"));
        assert!(!supports_resume("codex"));
        assert!(!supports_resume("goose"));
    }

    fn api_config(vendor: ApiVendor) -> ApiJudgeConfig {
        ApiJudgeConfig {
            vendor,
            api_key_env: None,
            base_url: None,
            timeout_secs: 60,
            curl_bin: "curl".to_string(),
        }
    }

    #[test]
    fn api_judge_resolves_vendor_defaults() {
        let anthropic = ApiJudgeProvider::new(&api_config(ApiVendor::Anthropic));
        assert_eq!(anthropic.api_key_env, "ANTHROPIC_API_KEY");
        assert_eq!(anthropic.endpoint, "https://api.anthropic.com/v1/messages");

        let openai = ApiJudgeProvider::new(&api_config(ApiVendor::Openai));
        assert_eq!(openai.api_key_env, "OPENAI_API_KEY");
        assert_eq!(
            openai.endpoint,
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn api_judge_honors_overrides() {
        let provider = ApiJudgeProvider::new(&ApiJudgeConfig {
            vendor: ApiVendor::Openai,
            api_key_env: Some("MY_KEY".to_string()),
            base_url: Some("https://proxy.example/v1/chat/completions".to_string()),
            timeout_secs: 5,
            curl_bin: "curl".to_string(),
        });
        assert_eq!(provider.api_key_env, "MY_KEY");
        assert_eq!(
            provider.endpoint,
            "https://proxy.example/v1/chat/completions"
        );
    }

    #[test]
    fn build_chat_body_shapes_per_vendor() {
        let anthropic = build_chat_body(ApiVendor::Anthropic, "claude-x", "sys", "hi");
        assert_eq!(anthropic["model"], "claude-x");
        assert_eq!(anthropic["system"], "sys");
        assert_eq!(anthropic["messages"][0]["role"], "user");
        // Anthropic carries the system prompt in its own top-level field.
        assert!(anthropic.get("messages").unwrap().as_array().unwrap().len() == 1);

        let openai = build_chat_body(ApiVendor::Openai, "gpt-x", "sys", "hi");
        assert_eq!(openai["messages"][0]["role"], "system");
        assert_eq!(openai["messages"][1]["role"], "user");
        assert!(openai.get("system").is_none());
    }

    #[test]
    fn parses_anthropic_success_with_usage() {
        let raw = r#"{"content":[{"type":"text","text":"{\"value\": true}"}],
            "stop_reason":"end_turn","usage":{"input_tokens":12,"output_tokens":3}}"#;
        let outcome = parse_chat_response(ApiVendor::Anthropic, raw).unwrap();
        assert_eq!(outcome.text, "{\"value\": true}");
        let usage = outcome.usage.unwrap();
        assert_eq!(usage.input_tokens, Some(12));
        assert_eq!(usage.output_tokens, Some(3));
        assert!(usage.cost_usd.is_none());
    }

    #[test]
    fn parses_openai_success_with_usage() {
        let raw = r#"{"choices":[{"message":{"content":"{\"value\": 8}"}}],
            "usage":{"prompt_tokens":20,"completion_tokens":4}}"#;
        let outcome = parse_chat_response(ApiVendor::Openai, raw).unwrap();
        assert_eq!(outcome.text, "{\"value\": 8}");
        let usage = outcome.usage.unwrap();
        assert_eq!(usage.input_tokens, Some(20));
        assert_eq!(usage.output_tokens, Some(4));
    }

    #[test]
    fn parses_and_classifies_api_errors() {
        let auth = r#"{"error":{"type":"authentication_error","message":"bad key"}}"#;
        let err = parse_chat_response(ApiVendor::Anthropic, auth).unwrap_err();
        assert!(matches!(err, Error::Provider { kind: Some(k), .. } if k == "auth"));

        let rate = r#"{"error":{"type":"rate_limit_exceeded","message":"slow down"}}"#;
        let err = parse_chat_response(ApiVendor::Openai, rate).unwrap_err();
        assert!(matches!(err, Error::Provider { kind: Some(k), .. } if k == "rate_limit"));
    }

    #[test]
    fn empty_reply_is_an_error() {
        let raw = r#"{"content":[],"stop_reason":"refusal"}"#;
        assert!(parse_chat_response(ApiVendor::Anthropic, raw).is_err());
    }

    #[test]
    fn classify_api_error_maps_known_kinds() {
        assert_eq!(
            classify_api_error(Some("invalid_api_key")).as_deref(),
            Some("auth")
        );
        assert_eq!(
            classify_api_error(Some("insufficient_quota")).as_deref(),
            Some("quota")
        );
        assert_eq!(
            classify_api_error(Some("not_found_error")).as_deref(),
            Some("model_not_found")
        );
        assert_eq!(classify_api_error(Some("something_else")), None);
        assert_eq!(classify_api_error(None), None);
    }

    #[test]
    fn curl_escape_handles_quotes_and_backslashes() {
        assert_eq!(curl_escape(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    /// A skill-running provider stub so the SplitProvider's delegation can be
    /// checked without touching the network.
    struct StubResponder;

    impl Provider for StubResponder {
        fn respond(
            &self,
            _platform: &str,
            _model: &str,
            _skill: &SkillRef<'_>,
            _messages: &[Message],
            _session: Option<&str>,
        ) -> Result<AssistantTurn> {
            Ok(AssistantTurn {
                message: "stub reply".to_string(),
                ..Default::default()
            })
        }

        fn simulate_user(
            &self,
            _model: &str,
            _persona: &str,
            _messages: &[Message],
        ) -> Result<UserTurn> {
            unreachable!("split provider routes user simulation to the judge")
        }

        fn judge(
            &self,
            _model: &str,
            _query: &JudgeQuery<'_>,
            _messages: &[Message],
        ) -> Result<JudgeVerdict> {
            unreachable!("split provider routes judging to the judge")
        }

        fn supports_resume(&self, platform: &str) -> bool {
            platform == "claude-code"
        }
    }

    #[test]
    fn split_provider_delegates_respond_and_resume() {
        let split = SplitProvider::new(
            Box::new(StubResponder),
            ApiJudgeProvider::new(&api_config(ApiVendor::Anthropic)),
        );
        // respond + supports_resume go to the responder...
        assert!(split.supports_resume("claude-code"));
        assert!(!split.supports_resume("codex"));
        let skill = SkillRef {
            name: "s",
            dir: "/tmp/s",
            instructions: "do things",
        };
        let turn = split
            .respond("claude-code", "m", &skill, &[], None)
            .unwrap();
        assert_eq!(turn.message, "stub reply");
    }

    #[test]
    fn api_judge_does_not_run_skills() {
        let provider = ApiJudgeProvider::new(&api_config(ApiVendor::Anthropic));
        let skill = SkillRef {
            name: "s",
            dir: "/tmp/s",
            instructions: "x",
        };
        assert!(provider.respond("p", "m", &skill, &[], None).is_err());
    }
}
