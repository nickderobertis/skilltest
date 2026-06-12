//! Live end-to-end tests for the direct-API judge ([`ApiJudgeProvider`]) against
//! the **real** Anthropic and OpenAI APIs. Like `live.rs`, these are `#[ignore]`
//! and make real, paid, non-deterministic model calls — never in the gate.
//!
//! They exercise the path the gate cannot: strict-JSON structured outputs, the
//! `curl` transport, verdict parsing, and usage normalization, end to end. Each
//! test self-skips (prints a note and returns) when its API key is absent, so
//! `--ignored` is safe to run locally without keys.
//!
//! Run them:
//!
//! ```bash
//! ANTHROPIC_API_KEY=... OPENAI_API_KEY=... \
//!   cargo test -p skilltest-cli --test live_api_judge -- --ignored
//! ```
//!
//! Model knobs (optional): `SKILLTEST_ANTHROPIC_JUDGE_MODEL` (default
//! `claude-haiku-4-5`), `SKILLTEST_OPENAI_JUDGE_MODEL` (default `gpt-4o-mini`).
//! The fixtures are intentionally unambiguous so a real judge has one right
//! answer.

use skilltest_core::{
    ApiJudgeConfig, ApiJudgeProvider, ApiVendor, JudgeKind, JudgeQuery, JudgeValue, Message,
    Provider,
};

fn key_env(vendor: ApiVendor) -> &'static str {
    match vendor {
        ApiVendor::Anthropic => "ANTHROPIC_API_KEY",
        ApiVendor::Openai => "OPENAI_API_KEY",
    }
}

fn judge_model(vendor: ApiVendor) -> String {
    match vendor {
        ApiVendor::Anthropic => std::env::var("SKILLTEST_ANTHROPIC_JUDGE_MODEL")
            .unwrap_or_else(|_| "claude-haiku-4-5".to_string()),
        ApiVendor::Openai => std::env::var("SKILLTEST_OPENAI_JUDGE_MODEL")
            .unwrap_or_else(|_| "gpt-4o-mini".to_string()),
    }
}

fn provider(vendor: ApiVendor) -> ApiJudgeProvider {
    ApiJudgeProvider::new(&ApiJudgeConfig {
        vendor,
        api_key_env: None,
        base_url: None,
        timeout_secs: 60,
        curl_bin: "curl".to_string(),
        strict_json: true,
    })
}

/// A tiny, unambiguous transcript: the assistant clearly replied "pong".
fn pong_transcript() -> Vec<Message> {
    vec![Message::user("ping"), Message::assistant("pong")]
}

/// Drive every judge op against one real vendor. Skips when the key is absent.
fn exercise(vendor: ApiVendor) {
    if std::env::var(key_env(vendor)).is_err() {
        eprintln!(
            "skipping {:?} live judge: {} not set",
            vendor,
            key_env(vendor)
        );
        return;
    }
    let model = judge_model(vendor);
    let provider = provider(vendor);
    let transcript = pong_transcript();

    // 1. Boolean verdict — true case. Strict JSON guarantees a clean bool.
    let yes = provider
        .judge(
            &model,
            &JudgeQuery {
                kind: JudgeKind::Boolean,
                criterion: "The assistant's reply is exactly the word \"pong\".",
                scale: None,
            },
            &transcript,
        )
        .expect("boolean judge call succeeds");
    assert_eq!(
        yes.value,
        JudgeValue::Bool(true),
        "{vendor:?} should judge the pong criterion true; reason: {}",
        yes.reason
    );
    // Usage flowed through from the real response.
    let usage = yes.usage.expect("the API reports usage");
    assert!(
        usage.input_tokens.unwrap_or(0) > 0 && usage.output_tokens.unwrap_or(0) > 0,
        "{vendor:?} usage should carry token counts; got {usage:?}"
    );

    // 2. Boolean verdict — false case.
    let no = provider
        .judge(
            &model,
            &JudgeQuery {
                kind: JudgeKind::Boolean,
                criterion: "The assistant asked the user for their full mailing address.",
                scale: None,
            },
            &transcript,
        )
        .expect("boolean judge call succeeds");
    assert_eq!(
        no.value,
        JudgeValue::Bool(false),
        "{vendor:?} should judge the false criterion false; reason: {}",
        no.reason
    );

    // 3. Numeric verdict — a real number within the requested scale.
    let scored = provider
        .judge(
            &model,
            &JudgeQuery {
                kind: JudgeKind::Numeric,
                criterion:
                    "Rate from 0 to 10 how clearly the assistant responded (10 = perfectly clear).",
                scale: Some((0.0, 10.0)),
            },
            &transcript,
        )
        .expect("numeric judge call succeeds");
    match scored.value {
        JudgeValue::Number(n) => assert!(
            (0.0..=10.0).contains(&n),
            "{vendor:?} numeric verdict {n} out of [0, 10]"
        ),
        other => panic!("{vendor:?} expected a numeric verdict, got {other:?}"),
    }

    // 4. Simulated user — free-form (never schema-constrained) and non-empty.
    let user = provider
        .simulate_user(
            &model,
            "a terse traveler who wants tomorrow's weather in Paris",
            &[Message::assistant("Hi! How can I help you today?")],
        )
        .expect("user simulation succeeds");
    assert!(
        !user.message.trim().is_empty(),
        "{vendor:?} simulated user produced an empty message"
    );
}

#[test]
#[ignore = "live: needs ANTHROPIC_API_KEY; run with --ignored"]
fn live_anthropic_judge_strict_json() {
    exercise(ApiVendor::Anthropic);
}

#[test]
#[ignore = "live: needs OPENAI_API_KEY; run with --ignored"]
fn live_openai_judge_strict_json() {
    exercise(ApiVendor::Openai);
}
