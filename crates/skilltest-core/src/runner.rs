//! The runner: orchestrates a test case into a conversation, drives the
//! provider across turns, scores the transcript with evals, and fans out over
//! the configured platform × model matrix.

use crate::config::Config;
use crate::conversation::{Message, Transcript};
use crate::error::Result;
use crate::eval::{Eval, JudgeValue};
use crate::provider::{JudgeKind, JudgeQuery, Provider, SkillRef, Usage};
use crate::report::{CaseRun, Report};
use crate::skill::{load_skill, SkillDefinition};
use crate::testcase::TestCase;

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
        let mut runs = Vec::new();
        for case in cases {
            runs.extend(self.run_case(case)?);
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
        for platform in &self.config.platforms {
            for model in &self.config.models {
                runs.push(self.run_case_on(case, &skill, platform, model)?);
            }
        }
        Ok(runs)
    }

    /// Run a single case on one platform/model pair.
    fn run_case_on(
        &self,
        case: &TestCase,
        skill: &SkillDefinition,
        platform: &str,
        model: &str,
    ) -> Result<CaseRun> {
        let mut totals = Usage::default();
        let transcript = self.converse(case, skill, platform, model, &mut totals)?;
        let evals = self.score(case, &transcript, &mut totals)?;
        let passed = evals.iter().all(|e| e.passed);
        Ok(CaseRun {
            case: case.name.clone(),
            skill: skill.dir.to_string_lossy().into_owned(),
            platform: platform.to_string(),
            model: model.to_string(),
            passed,
            turns: transcript.assistant_turns(),
            evals,
            transcript,
            usage: (!totals.is_empty()).then_some(totals),
        })
    }

    /// Drive the conversation: a single assistant turn for single-turn cases, or
    /// a simulated-user loop for multi-turn cases.
    fn converse(
        &self,
        case: &TestCase,
        skill: &SkillDefinition,
        platform: &str,
        model: &str,
        totals: &mut Usage,
    ) -> Result<Transcript> {
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

        loop {
            let session_arg = if resume_supported {
                session.as_deref()
            } else {
                None
            };
            let turn = self.provider.respond(
                platform,
                model,
                &skill_ref,
                &transcript.messages,
                session_arg,
            )?;
            if let Some(u) = &turn.usage {
                totals.add(u);
            }
            // Capture or refresh the session handle for the next turn.
            if let Some(id) = turn.session_id {
                session = Some(id);
            }
            let skill_done = turn.done;
            transcript.push(Message::assistant(turn.message));

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

        Ok(transcript)
    }

    /// Run every eval against the finished transcript.
    fn score(
        &self,
        case: &TestCase,
        transcript: &Transcript,
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
}
