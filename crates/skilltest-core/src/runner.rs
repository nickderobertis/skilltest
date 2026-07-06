//! The runner: orchestrates a test case into a conversation, drives the
//! provider across turns, scores the transcript with evals, and fans out over
//! the configured platform × model matrix.

use std::ops::ControlFlow;

use crate::config::Config;
use crate::conversation::{Message, ToolEvent, Transcript};
use crate::error::{Error, Result};
use crate::eval::{Eval, JudgeValue};
use crate::mock::{describe_records, where_matches, MockCall, MockPlan, MockSet};
use crate::provider::{JudgeKind, JudgeQuery, Provider, SkillRef, Usage};
use crate::report::{CaseRun, Report};
use crate::skill::{load_skill, SkillDefinition};
use crate::testcase::TestCase;

/// One streamed tool event, tagged with the run it belongs to, delivered live to
/// a [`Runner::run_all_streaming`] sink so a consumer can watch what a skill does
/// and short-circuit.
pub struct StreamEvent<'a> {
    /// The test case's name.
    pub case: &'a str,
    /// The platform (harness) under test.
    pub platform: &'a str,
    /// The model under test.
    pub model: &'a str,
    /// 1-based assistant-turn index within this run.
    pub turn: usize,
    /// The normalized tool event.
    pub event: &'a ToolEvent,
}

/// The streaming knobs threaded through a run: whether to drive turns live (via
/// [`Provider::respond_streaming`]) and the sink each tool event is delivered to.
struct Streaming<'s> {
    on: bool,
    sink: &'s mut (dyn FnMut(&StreamEvent) -> ControlFlow<()> + 's),
}

/// Runs test cases against a provider using a configuration.
pub struct Runner<'a> {
    provider: &'a dyn Provider,
    config: &'a Config,
}

impl<'a> Runner<'a> {
    /// Build a runner.
    #[must_use]
    pub fn new(provider: &'a dyn Provider, config: &'a Config) -> Self {
        Self { provider, config }
    }

    /// Run every supplied case across the full platform × model matrix and
    /// collect a [`Report`].
    ///
    /// # Errors
    /// Propagates the first [`crate::Error`] from loading a skill or a provider
    /// failure. Eval *failures* are not errors — they are recorded in the report.
    pub fn run_all(&self, cases: &[TestCase]) -> Result<Report> {
        let mut sink = |_: &StreamEvent| ControlFlow::Continue(());
        self.run_all_inner(
            cases,
            &mut Streaming {
                on: false,
                sink: &mut sink,
            },
        )
    }

    /// Like [`Runner::run_all`], but drives each turn through
    /// [`Provider::respond_streaming`] and delivers each skill tool event to
    /// `on_event` the instant it is observed. `on_event` returns
    /// [`ControlFlow::Break`] to short-circuit: the current run is torn down (the
    /// provider kills the harness), no further runs start, and the partial
    /// [`Report`] built so far is returned.
    ///
    /// # Errors
    /// As [`Runner::run_all`].
    pub fn run_all_streaming(
        &self,
        cases: &[TestCase],
        on_event: &mut dyn FnMut(&StreamEvent) -> ControlFlow<()>,
    ) -> Result<Report> {
        self.run_all_inner(
            cases,
            &mut Streaming {
                on: true,
                sink: on_event,
            },
        )
    }

    /// The matrix loop shared by the buffered and streaming entry points.
    /// `streaming.on` selects the buffered [`Provider::respond`] (`false`) or the
    /// live [`Provider::respond_streaming`] (`true`) per turn.
    fn run_all_inner(&self, cases: &[TestCase], streaming: &mut Streaming) -> Result<Report> {
        let mut runs = Vec::new();
        for case in cases {
            let skill = load_skill(&case.skill)?;
            for platform in &self.config.platforms {
                for model in &self.config.models {
                    let (run, flow) = self.run_case_on(case, &skill, platform, model, streaming)?;
                    runs.push(run);
                    if flow.is_break() {
                        return Ok(Report::new(runs));
                    }
                }
            }
        }
        Ok(Report::new(runs))
    }

    /// Run a single case across the matrix.
    ///
    /// # Errors
    /// As [`Runner::run_all`].
    pub fn run_case(&self, case: &TestCase) -> Result<Vec<CaseRun>> {
        let skill = load_skill(&case.skill)?;
        let mut runs = Vec::new();
        let mut sink = |_: &StreamEvent| ControlFlow::Continue(());
        let mut streaming = Streaming {
            on: false,
            sink: &mut sink,
        };
        for platform in &self.config.platforms {
            for model in &self.config.models {
                let (run, _flow) =
                    self.run_case_on(case, &skill, platform, model, &mut streaming)?;
                runs.push(run);
            }
        }
        Ok(runs)
    }

    /// Run a single case on one platform/model pair. Returns the run plus whether
    /// the streaming sink asked to short-circuit ([`ControlFlow::Break`]).
    fn run_case_on(
        &self,
        case: &TestCase,
        skill: &SkillDefinition,
        platform: &str,
        model: &str,
        streaming: &mut Streaming,
    ) -> Result<(CaseRun, ControlFlow<()>)> {
        let mut totals = Usage::default();
        // The effective mock/spy set: CLI/SDK declarations first (first match
        // wins, so the most local rule shadows), then the case's own.
        let mock_set =
            MockSet::build(&self.config.mocks, &case.mocks, self.config.spy || case.spy)?;
        let (transcript, mock_calls, flow) = self.converse(
            case,
            skill,
            platform,
            model,
            &mock_set,
            &mut totals,
            streaming,
        )?;
        let mock_calls = mock_calls.map(|records| mock_set.resolve(records));
        // On an abort we don't spend judge calls scoring a torn-off transcript.
        let evals = if flow.is_break() {
            Vec::new()
        } else {
            self.score(
                case,
                &transcript,
                &mock_set,
                mock_calls.as_deref(),
                &mut totals,
            )?
        };
        let passed = flow.is_continue() && evals.iter().all(|e| e.passed);
        Ok((
            CaseRun {
                case: case.name.clone(),
                skill: skill.dir.to_string_lossy().into_owned(),
                platform: platform.to_string(),
                model: model.to_string(),
                passed,
                turns: transcript.assistant_turns(),
                evals,
                transcript,
                usage: (!totals.is_empty()).then_some(totals),
                mock_calls,
            },
            flow,
        ))
    }

    /// Drive the conversation: a single assistant turn for single-turn cases, or
    /// a simulated-user loop for multi-turn cases. Streams each turn's tool
    /// events to `on_event`; returns the transcript plus whether the sink asked
    /// to short-circuit.
    #[allow(clippy::too_many_arguments)]
    fn converse(
        &self,
        case: &TestCase,
        skill: &SkillDefinition,
        platform: &str,
        model: &str,
        mock_set: &MockSet,
        totals: &mut Usage,
        streaming: &mut Streaming,
    ) -> Result<(Transcript, Option<Vec<MockCall>>, ControlFlow<()>)> {
        let dir = skill.dir.to_string_lossy().into_owned();
        let skill_ref = SkillRef {
            name: &skill.name,
            dir: &dir,
            instructions: &skill.instructions,
        };
        let judge_model = self.config.effective_judge_model();
        let max_turns = case
            .user
            .as_ref()
            .and_then(|u| u.max_turns)
            .unwrap_or(self.config.max_turns) as usize;
        let resume_supported = self.provider.supports_resume(platform);

        let mut transcript = Transcript::from_input(&case.input);
        // On harnesses that support it, thread the session_id from each
        // respond into the next one so the harness keeps real state instead of
        // being re-prompted with a stringified transcript.
        let mut session: Option<String> = None;
        // The mock/spy plan, handed to every skill turn (never to the judge or
        // the simulated user), and the records accumulated across turns.
        // `None` until some turn reports a channel, so "channel off" and
        // "channel on, zero calls" stay distinguishable.
        let plan = mock_set.active().then(|| MockPlan {
            rules: mock_set.rules(),
        });
        let mut mock_calls: Option<Vec<MockCall>> = None;

        loop {
            let session_arg = if resume_supported {
                session.as_deref()
            } else {
                None
            };
            // In streaming mode, drive the turn through `respond_streaming` and
            // tag each event with the run it belongs to; if the sink breaks, the
            // provider tears the harness down and returns the partial turn, and we
            // stop the run below. The buffered path uses the plain `respond` so a
            // non-streaming run keeps oneharness's buffered (`--compact`) contract.
            let case_name = case.name.as_str();
            let turn_index = transcript.assistant_turns() + 1;
            let mut turn_flow = ControlFlow::Continue(());
            let turn = if streaming.on {
                let sink = &mut streaming.sink;
                self.provider.respond_streaming_with_mocks(
                    platform,
                    model,
                    &skill_ref,
                    &transcript.messages,
                    session_arg,
                    plan.as_ref(),
                    &mut |event| {
                        let flow = sink(&StreamEvent {
                            case: case_name,
                            platform,
                            model,
                            turn: turn_index,
                            event,
                        });
                        if flow.is_break() {
                            turn_flow = ControlFlow::Break(());
                        }
                        flow
                    },
                )?
            } else {
                self.provider.respond_with_mocks(
                    platform,
                    model,
                    &skill_ref,
                    &transcript.messages,
                    session_arg,
                    plan.as_ref(),
                )?
            };
            if let Some(records) = turn.mock_calls {
                mock_calls.get_or_insert_with(Vec::new).extend(records);
            }
            if let Some(u) = &turn.usage {
                totals.add(u);
            }
            // Capture or refresh the session handle for the next turn.
            if let Some(id) = turn.session_id {
                session = Some(id);
            }
            let skill_done = turn.done;
            // Carry the turn's normalized tool events onto its assistant message
            // so consumers can analyze what the skill did.
            transcript.push(Message::assistant(turn.message).with_events(turn.events));

            // The streaming sink asked to short-circuit: stop the run now.
            if turn_flow.is_break() {
                return Ok((transcript, mock_calls, ControlFlow::Break(())));
            }

            // Single-turn cases stop after the first assistant turn.
            let Some(user) = &case.user else {
                break;
            };

            if skill_done || transcript.assistant_turns() >= max_turns {
                break;
            }

            // Stop early if the configured done-condition holds.
            if let Some(done_when) = &user.done_when {
                let query = JudgeQuery {
                    kind: JudgeKind::Boolean,
                    criterion: done_when,
                    scale: None,
                };
                let verdict = self
                    .provider
                    .judge(judge_model, &query, &transcript.messages)?;
                if let Some(u) = &verdict.usage {
                    totals.add(u);
                }
                if matches!(verdict.value, JudgeValue::Bool(true)) {
                    break;
                }
            }

            // Otherwise the simulated user replies and the loop continues.
            let user_turn =
                self.provider
                    .simulate_user(judge_model, &user.persona, &transcript.messages)?;
            if let Some(u) = &user_turn.usage {
                totals.add(u);
            }
            let stop = user_turn.stop;
            transcript.push(Message::user(user_turn.message));
            if stop {
                break;
            }
        }

        Ok((transcript, mock_calls, ControlFlow::Continue(())))
    }

    /// Run every eval against the finished transcript: judge-backed kinds go
    /// to the provider's judge; `called`/`not_called` are scored
    /// deterministically against the mock/spy channel's records.
    fn score(
        &self,
        case: &TestCase,
        transcript: &Transcript,
        mock_set: &MockSet,
        mock_calls: Option<&[MockCall]>,
        totals: &mut Usage,
    ) -> Result<Vec<crate::eval::EvalOutcome>> {
        let judge_model = self.config.effective_judge_model();
        let mut outcomes = Vec::with_capacity(case.evals.len());
        for eval in &case.evals {
            let query = match eval {
                Eval::Boolean { criterion, .. } => JudgeQuery {
                    kind: JudgeKind::Boolean,
                    criterion,
                    scale: None,
                },
                Eval::Numeric {
                    criterion,
                    min,
                    max,
                    ..
                } => JudgeQuery {
                    kind: JudgeKind::Numeric,
                    criterion,
                    scale: Some((*min, *max)),
                },
                Eval::Called { mock, r#where, .. } | Eval::NotCalled { mock, r#where, .. } => {
                    // Deterministic: no judge call. A missing channel is loud —
                    // a `not_called` scored against nothing must never pass.
                    let records = mock_calls.ok_or_else(|| {
                        Error::Invalid(format!(
                            "eval `{}` needs the mock/spy channel, but the provider reported no                              observations for this run",
                            eval.label()
                        ))
                    })?;
                    let matching = mock_set
                        .records_for(mock, records)?
                        .into_iter()
                        .filter(|r| where_matches(r#where, r.input.as_ref()))
                        .count();
                    outcomes.push(eval.outcome_for_calls(matching, &describe_records(records))?);
                    continue;
                }
            };
            let verdict = self
                .provider
                .judge(judge_model, &query, &transcript.messages)?;
            if let Some(u) = &verdict.usage {
                totals.add(u);
            }
            outcomes.push(eval.outcome(&verdict.value, verdict.reason)?);
        }
        Ok(outcomes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::Message;
    use crate::provider::{AssistantTurn, JudgeVerdict, UserTurn};
    use std::cell::RefCell;

    /// An in-memory provider scripted with canned turns and verdicts, so the
    /// runner's orchestration can be tested without any subprocess.
    struct ScriptedProvider {
        assistant: Vec<AssistantTurn>,
        user: Vec<UserTurn>,
        judge: Vec<JudgeVerdict>,
        calls: RefCell<Calls>,
    }

    #[derive(Default)]
    struct Calls {
        assistant: usize,
        user: usize,
        judge: usize,
    }

    impl Provider for ScriptedProvider {
        fn respond(
            &self,
            _platform: &str,
            _model: &str,
            _skill: &SkillRef<'_>,
            _messages: &[Message],
            _session: Option<&str>,
        ) -> Result<AssistantTurn> {
            let i = self.calls.borrow().assistant;
            self.calls.borrow_mut().assistant += 1;
            Ok(self.assistant[i.min(self.assistant.len() - 1)].clone())
        }

        fn simulate_user(
            &self,
            _model: &str,
            _persona: &str,
            _messages: &[Message],
        ) -> Result<UserTurn> {
            let i = self.calls.borrow().user;
            self.calls.borrow_mut().user += 1;
            Ok(self.user[i.min(self.user.len() - 1)].clone())
        }

        fn judge(
            &self,
            _model: &str,
            _query: &JudgeQuery<'_>,
            _messages: &[Message],
        ) -> Result<JudgeVerdict> {
            let i = self.calls.borrow().judge;
            self.calls.borrow_mut().judge += 1;
            let v = &self.judge[i.min(self.judge.len() - 1)];
            Ok(JudgeVerdict {
                value: v.value,
                reason: v.reason.clone(),
                usage: v.usage.clone(),
            })
        }
    }

    /// Create a throwaway skill directory with a minimal SKILL.md so the runner
    /// (which loads the skill from disk) has something real to read.
    fn temp_skill(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("skilltest-ut-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: greeter\ndescription: a test skill\n---\nfake-reply: hi\n",
        )
        .unwrap();
        dir
    }

    fn boolean_case(skill: std::path::PathBuf) -> TestCase {
        TestCase {
            name: "greets".into(),
            skill,
            input: "Greet Dr. Smith".into(),
            user: None,
            mocks: Vec::new(),
            spy: false,
            evals: vec![Eval::Boolean {
                criterion: "greets Dr. Smith".into(),
                expected: true,
                name: None,
            }],
        }
    }

    #[test]
    fn single_turn_runs_one_assistant_turn_and_scores() {
        let provider = ScriptedProvider {
            assistant: vec![AssistantTurn {
                message: "Hello, Dr. Smith!".into(),
                done: false,
                ..Default::default()
            }],
            user: vec![],
            judge: vec![JudgeVerdict {
                value: JudgeValue::Bool(true),
                reason: "names her".into(),
                usage: None,
            }],
            calls: RefCell::new(Calls::default()),
        };
        let config = Config::default();
        let runner = Runner::new(&provider, &config);
        let runs = runner
            .run_case(&boolean_case(temp_skill("single")))
            .unwrap();
        assert_eq!(runs.len(), 1);
        assert!(runs[0].passed);
        assert_eq!(runs[0].turns, 1);
        assert_eq!(provider.calls.borrow().assistant, 1);
    }

    #[test]
    fn multi_turn_stops_when_done_when_holds() {
        let mut case = boolean_case(temp_skill("multi"));
        case.user = Some(crate::testcase::SimulatedUser {
            persona: "a terse patient".into(),
            done_when: Some("the assistant has greeted".into()),
            max_turns: Some(5),
        });
        let provider = ScriptedProvider {
            assistant: vec![AssistantTurn {
                message: "Hi there".into(),
                done: false,
                ..Default::default()
            }],
            user: vec![UserTurn {
                message: "continue".into(),
                stop: false,
                ..Default::default()
            }],
            // First judge call is the done_when check (true -> stop), second is
            // the eval.
            judge: vec![
                JudgeVerdict {
                    value: JudgeValue::Bool(true),
                    reason: "done".into(),
                    usage: None,
                },
                JudgeVerdict {
                    value: JudgeValue::Bool(true),
                    reason: "greeted".into(),
                    usage: None,
                },
            ],
            calls: RefCell::new(Calls::default()),
        };
        let config = Config::default();
        let runner = Runner::new(&provider, &config);
        let runs = runner.run_case(&case).unwrap();
        assert!(runs[0].passed);
        // One assistant turn, the simulated user never had to speak.
        assert_eq!(provider.calls.borrow().assistant, 1);
        assert_eq!(provider.calls.borrow().user, 0);
    }

    #[test]
    fn failing_eval_marks_run_failed() {
        let provider = ScriptedProvider {
            assistant: vec![AssistantTurn {
                message: "Hello".into(),
                done: false,
                ..Default::default()
            }],
            user: vec![],
            judge: vec![JudgeVerdict {
                value: JudgeValue::Bool(false),
                reason: "no name".into(),
                usage: None,
            }],
            calls: RefCell::new(Calls::default()),
        };
        let config = Config::default();
        let runner = Runner::new(&provider, &config);
        let report = runner
            .run_all(&[boolean_case(temp_skill("faileval"))])
            .unwrap();
        assert!(!report.passed);
        assert_eq!(report.summary.failed, 1);
    }

    #[test]
    fn matrix_fans_out_over_platforms_and_models() {
        let provider = ScriptedProvider {
            assistant: vec![AssistantTurn {
                message: "Hello".into(),
                done: false,
                ..Default::default()
            }],
            user: vec![],
            judge: vec![JudgeVerdict {
                value: JudgeValue::Bool(true),
                reason: String::new(),
                usage: None,
            }],
            calls: RefCell::new(Calls::default()),
        };
        let config = Config {
            platforms: vec!["a".into(), "b".into()],
            models: vec!["m1".into(), "m2".into()],
            ..Config::default()
        };
        let runner = Runner::new(&provider, &config);
        let runs = runner
            .run_case(&boolean_case(temp_skill("matrix")))
            .unwrap();
        assert_eq!(runs.len(), 4);
    }

    #[test]
    fn run_all_streaming_short_circuits_on_break() {
        use crate::conversation::ToolEvent;
        // A turn that took a disallowed action; the streaming sink breaks on it.
        let provider = ScriptedProvider {
            assistant: vec![AssistantTurn {
                message: "did a bad thing".into(),
                done: false,
                usage: None,
                session_id: None,
                mock_calls: None,
                events: vec![ToolEvent {
                    kind: "tool_call".into(),
                    name: Some("bash".into()),
                    input: Some(serde_json::json!({ "command": "rm -rf /" })),
                    output: None,
                    index: 0,
                }],
            }],
            user: vec![],
            judge: vec![JudgeVerdict {
                value: JudgeValue::Bool(true),
                reason: String::new(),
                usage: None,
            }],
            calls: RefCell::new(Calls::default()),
        };
        let config = Config {
            platforms: vec!["a".into(), "b".into()],
            ..Config::default()
        };
        let runner = Runner::new(&provider, &config);
        let mut seen = 0usize;
        let report = runner
            .run_all_streaming(
                &[boolean_case(temp_skill("stream-abort"))],
                &mut |ev: &StreamEvent| {
                    seen += 1;
                    assert_eq!(ev.event.name.as_deref(), Some("bash"));
                    assert_eq!(ev.turn, 1);
                    ControlFlow::Break(())
                },
            )
            .unwrap();
        // The sink saw the first event and aborted; only the first matrix cell
        // ran, and it is not passing.
        assert_eq!(seen, 1);
        assert_eq!(report.runs.len(), 1);
        assert!(!report.runs[0].passed);
        // No judge call was spent scoring the torn-off run.
        assert_eq!(provider.calls.borrow().judge, 0);
    }

    fn usage(input: u64) -> Option<Usage> {
        Some(Usage {
            input_tokens: Some(input),
            output_tokens: None,
            cost_usd: None,
        })
    }

    #[test]
    fn multi_turn_loops_through_simulated_user_and_aggregates_usage() {
        // No early stop: the done_when check returns false, so the simulated
        // user speaks after turn 1, then turn 2 satisfies done_when and the loop
        // ends. Every provider call reports usage, so totals must accumulate.
        let mut case = boolean_case(temp_skill("loop"));
        case.user = Some(crate::testcase::SimulatedUser {
            persona: "a chatty patient".into(),
            done_when: Some("the booking is confirmed".into()),
            max_turns: Some(8),
        });
        let provider = ScriptedProvider {
            assistant: vec![
                AssistantTurn {
                    message: "Hello, how can I help?".into(),
                    done: false,
                    usage: usage(3),
                    // A session id the runner should capture for the next turn.
                    session_id: Some("sess-1".into()),
                    events: Vec::new(),
                    mock_calls: None,
                },
                AssistantTurn {
                    message: "Booked!".into(),
                    done: false,
                    usage: usage(4),
                    session_id: Some("sess-2".into()),
                    events: Vec::new(),
                    mock_calls: None,
                },
            ],
            user: vec![UserTurn {
                message: "Please book me in.".into(),
                stop: false,
                usage: usage(2),
            }],
            judge: vec![
                // done_when after assistant turn 1 -> not done yet.
                JudgeVerdict {
                    value: JudgeValue::Bool(false),
                    reason: "not yet".into(),
                    usage: usage(1),
                },
                // done_when after assistant turn 2 -> done, the loop ends.
                JudgeVerdict {
                    value: JudgeValue::Bool(true),
                    reason: "confirmed".into(),
                    usage: usage(1),
                },
                // The final eval.
                JudgeVerdict {
                    value: JudgeValue::Bool(true),
                    reason: "greeted".into(),
                    usage: usage(5),
                },
            ],
            calls: RefCell::new(Calls::default()),
        };
        let config = Config::default();
        let runner = Runner::new(&provider, &config);
        let runs = runner.run_case(&case).unwrap();
        assert!(runs[0].passed);
        // Two assistant turns; the user spoke exactly once between them.
        assert_eq!(provider.calls.borrow().assistant, 2);
        assert_eq!(provider.calls.borrow().user, 1);
        assert_eq!(provider.calls.borrow().judge, 3);
        // Usage across every call:
        // 3(resp) + 1(done_when) + 2(user) + 4(resp) + 1(done_when) + 5(eval) = 16.
        assert_eq!(runs[0].usage.as_ref().unwrap().input_tokens, Some(16));
    }

    /// A provider with mock support: records the plan it was handed and
    /// returns scripted mock records, so the runner's threading, resolution,
    /// and deterministic scoring can be tested without a subprocess.
    struct MockingProvider {
        records: Vec<crate::mock::MockCall>,
        seen_rules: RefCell<Vec<Option<serde_json::Value>>>,
        judge_calls: RefCell<usize>,
    }

    impl Provider for MockingProvider {
        fn respond(
            &self,
            _platform: &str,
            _model: &str,
            _skill: &SkillRef<'_>,
            _messages: &[Message],
            _session: Option<&str>,
        ) -> Result<AssistantTurn> {
            unreachable!("the runner must route through respond_with_mocks")
        }

        fn respond_with_mocks(
            &self,
            _platform: &str,
            _model: &str,
            _skill: &SkillRef<'_>,
            _messages: &[Message],
            _session: Option<&str>,
            mocks: Option<&crate::mock::MockPlan<'_>>,
        ) -> Result<AssistantTurn> {
            self.seen_rules
                .borrow_mut()
                .push(mocks.and_then(|p| p.rules.cloned()));
            Ok(AssistantTurn {
                message: "did things".into(),
                mock_calls: mocks.map(|_| self.records.clone()),
                ..Default::default()
            })
        }

        fn simulate_user(
            &self,
            _model: &str,
            _persona: &str,
            _messages: &[Message],
        ) -> Result<UserTurn> {
            unreachable!("single-turn case")
        }

        fn judge(
            &self,
            _model: &str,
            _query: &JudgeQuery<'_>,
            _messages: &[Message],
        ) -> Result<JudgeVerdict> {
            *self.judge_calls.borrow_mut() += 1;
            Ok(JudgeVerdict {
                value: JudgeValue::Bool(true),
                reason: "fine".into(),
                usage: None,
            })
        }
    }

    fn mock_record(
        tool: &str,
        command: &str,
        action: &str,
        rule: Option<usize>,
    ) -> crate::mock::MockCall {
        crate::mock::MockCall {
            tool: Some(tool.into()),
            input: Some(serde_json::json!({ "command": command })),
            action: action.into(),
            rule,
            mock: None,
        }
    }

    #[test]
    fn mocked_case_scores_call_evals_deterministically() {
        let mut case = boolean_case(temp_skill("mocked"));
        case.mocks = serde_yaml::from_str(
            r#"
- name: push
  match: { tool: bash, pattern: "git push( --force)?\\b" }
  stub: Everything up-to-date
- name: danger
  match: { contains: "rm -rf" }
  deny: blocked
- name: git
  match: { tool: bash, pattern: "\\bgit\\b" }
"#,
        )
        .unwrap();
        case.evals = vec![
            serde_yaml::from_str("type: called\nmock: push\ntimes: 1\n").unwrap(),
            serde_yaml::from_str("type: not_called\nmock: danger\n").unwrap(),
            serde_yaml::from_str(
                "type: called\nmock: git\nwhere: { command: { contains: status } }\n",
            )
            .unwrap(),
        ];
        let provider = MockingProvider {
            records: vec![
                mock_record("bash", "git push origin", "stub", Some(0)),
                mock_record("bash", "git status", "allow", None),
            ],
            seen_rules: RefCell::new(Vec::new()),
            judge_calls: RefCell::new(0),
        };
        let config = Config::default();
        let runner = Runner::new(&provider, &config);
        let report = runner.run_all(&[case]).unwrap();
        assert!(report.passed, "all call evals hold: {report:?}");
        // No judge was ever consulted — the call evals are deterministic.
        assert_eq!(*provider.judge_calls.borrow(), 0);
        // The provider received the compiled ruleset (two action rules; the
        // spy is matched locally, never compiled).
        let seen = provider.seen_rules.borrow();
        let rules = seen[0].as_ref().expect("plan carried rules");
        assert_eq!(rules["rules"].as_array().unwrap().len(), 2);
        // The report's records got their mock names resolved.
        let run = &report.runs[0];
        let records = run.mock_calls.as_ref().expect("channel was on");
        assert_eq!(records[0].mock.as_deref(), Some("push"));
        assert_eq!(records[1].mock, None);
    }

    #[test]
    fn failing_not_called_eval_reports_the_observed_calls() {
        let mut case = boolean_case(temp_skill("mock-violate"));
        case.mocks = serde_yaml::from_str(
            "- name: danger\n  match: { contains: \"rm -rf\" }\n  deny: blocked\n",
        )
        .unwrap();
        case.evals = vec![serde_yaml::from_str("type: not_called\nmock: danger\n").unwrap()];
        let provider = MockingProvider {
            records: vec![mock_record("bash", "rm -rf /", "deny", Some(0))],
            seen_rules: RefCell::new(Vec::new()),
            judge_calls: RefCell::new(0),
        };
        let config = Config::default();
        let runner = Runner::new(&provider, &config);
        let report = runner.run_all(&[case]).unwrap();
        assert!(!report.passed);
        let outcome = &report.runs[0].evals[0];
        assert!(!outcome.passed);
        // The failure reason lists what actually ran, verdict included.
        assert!(
            outcome.reason.contains("rm -rf /") && outcome.reason.contains("[deny]"),
            "reason: {}",
            outcome.reason
        );
    }

    #[test]
    fn call_eval_without_a_channel_is_loud_never_vacuous() {
        // A `not_called` eval on a case with no mocks/spy: there is no
        // observation channel, so the run must error, not pass on zero records.
        let mut case = boolean_case(temp_skill("mock-nochannel"));
        case.evals = vec![serde_yaml::from_str("type: not_called\nmock: danger\n").unwrap()];
        let provider = ScriptedProvider {
            assistant: vec![AssistantTurn {
                message: "hi".into(),
                ..Default::default()
            }],
            user: vec![],
            judge: vec![],
            calls: RefCell::new(Calls::default()),
        };
        let config = Config::default();
        let runner = Runner::new(&provider, &config);
        let err = runner.run_all(&[case]).unwrap_err();
        assert!(
            err.to_string().contains("needs the mock/spy channel"),
            "{err}"
        );
    }

    #[test]
    fn spy_flag_activates_channel_without_declarations() {
        // `spy: true` turns the channel on with no mocks: the plan carries no
        // rules, and the records land on the run for SDK spies to bind.
        let mut case = boolean_case(temp_skill("spy-flag"));
        case.spy = true;
        let provider = MockingProvider {
            records: vec![mock_record("bash", "ls", "allow", None)],
            seen_rules: RefCell::new(Vec::new()),
            judge_calls: RefCell::new(0),
        };
        let config = Config::default();
        let runner = Runner::new(&provider, &config);
        let report = runner.run_all(&[case]).unwrap();
        // The plan was present but rule-less.
        assert_eq!(provider.seen_rules.borrow()[0], None);
        let records = report.runs[0].mock_calls.as_ref().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].action, "allow");
    }

    #[test]
    fn multi_turn_threads_session_when_resume_supported() {
        // A provider that supports resume should be handed the session id the
        // previous respond returned. Capture the session arg each respond sees.
        #[derive(Default)]
        struct Sessions(RefCell<Vec<Option<String>>>);
        struct ResumeProvider {
            sessions: Sessions,
        }
        impl Provider for ResumeProvider {
            fn respond(
                &self,
                _platform: &str,
                _model: &str,
                _skill: &SkillRef<'_>,
                _messages: &[Message],
                session: Option<&str>,
            ) -> Result<AssistantTurn> {
                self.sessions
                    .0
                    .borrow_mut()
                    .push(session.map(str::to_string));
                let n = self.sessions.0.borrow().len();
                Ok(AssistantTurn {
                    message: format!("turn {n}"),
                    done: false,
                    usage: None,
                    session_id: Some(format!("sess-{n}")),
                    events: Vec::new(),
                    mock_calls: None,
                })
            }
            fn simulate_user(
                &self,
                _model: &str,
                _persona: &str,
                _messages: &[Message],
            ) -> Result<UserTurn> {
                Ok(UserTurn {
                    message: "go on".into(),
                    stop: false,
                    usage: None,
                })
            }
            fn judge(
                &self,
                _model: &str,
                _query: &JudgeQuery<'_>,
                _messages: &[Message],
            ) -> Result<JudgeVerdict> {
                Ok(JudgeVerdict {
                    value: JudgeValue::Bool(true),
                    reason: String::new(),
                    usage: None,
                })
            }
            fn supports_resume(&self, _platform: &str) -> bool {
                true
            }
        }
        let mut case = boolean_case(temp_skill("resume"));
        case.user = Some(crate::testcase::SimulatedUser {
            persona: "a patient".into(),
            // No done_when, so the loop runs to max_turns.
            done_when: None,
            max_turns: Some(2),
        });
        let provider = ResumeProvider {
            sessions: Sessions::default(),
        };
        let config = Config::default();
        let runner = Runner::new(&provider, &config);
        let runs = runner.run_case(&case).unwrap();
        assert_eq!(runs[0].turns, 2);
        // First respond saw no session; the second saw the id from the first.
        let seen = provider.sessions.0.borrow();
        assert_eq!(seen[0], None);
        assert_eq!(seen[1].as_deref(), Some("sess-1"));
    }
}
