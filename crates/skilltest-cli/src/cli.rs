//! Argument parsing and command dispatch for the `skilltest` CLI.
//!
//! Output discipline (see AGENTS.md): minimal on success, the exact problem plus
//! a suggested action on stderr, and a distinct [`ExitCode`] per failure class.

use std::ffi::OsString;
use std::io::Write as _;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use skilltest_core::{
    discover_cases, validate_path, ApiJudgeProvider, CommandProvider, Config, Error, ExitCode,
    JudgeConfig, OneharnessProvider, Overrides, Provider, ProviderConfig, Report, Result, Runner,
    SplitProvider, StreamEvent, TestCase, ValidationReport,
};

/// Test AI skills across harness/model platforms with natural-language evals.
#[derive(Parser)]
#[command(name = "skilltest", version, about, long_about = None)]
struct Cli {
    /// Config file. Defaults to `skilltest.yaml` in the working directory if it
    /// exists; otherwise built-in defaults are used.
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
// `Run`'s arg struct is by far the largest variant, but this enum is a
// short-lived, once-per-process parse target — boxing the variant (clap does
// not flatten through `Box` in a `Subcommand` derive) would add indirection for
// no real benefit here.
#[allow(clippy::large_enum_variant)]
enum Command {
    /// Run test cases against a skill and score the transcripts.
    Run(RunArgs),
    /// Validate one or more skill definitions (a skill dir, or a folder of them).
    Validate(ValidateArgs),
    /// Scaffold a starter project: a config, an example skill, and a case.
    Init(InitArgs),
    /// Print the JSON Schema for a machine-readable output (the SDK contract).
    Schema(SchemaArgs),
}

#[derive(Args)]
struct RunArgs {
    /// Test-case YAML files, or directories containing them.
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// Fully-specified test case(s) as a JSON file — a single case object or an
    /// array, the shape the language SDKs emit for a case built in code (see
    /// `docs/schema.md`). Unlike a YAML `PATH`, `skill` paths inside resolve
    /// relative to the working directory. Repeatable, and combines with any
    /// positional YAML `PATH`s.
    #[arg(long = "case-json", value_name = "FILE")]
    case_json: Vec<PathBuf>,

    /// Harness platform(s) to run on (repeatable; overrides config).
    #[arg(short = 'p', long = "platform", value_name = "PLATFORM")]
    platforms: Vec<String>,

    /// Model(s) to run on (repeatable; overrides config).
    #[arg(short = 'm', long = "model", value_name = "MODEL")]
    models: Vec<String>,

    /// Use a custom provider command (JSON-lines protocol) instead of
    /// oneharness. Whitespace-split into argv, e.g.
    /// `--provider "python3 provider.py"`.
    #[arg(long, value_name = "CMD")]
    provider: Option<String>,

    /// Path to the oneharness binary (oneharness provider only; overrides config).
    #[arg(long, value_name = "PATH")]
    oneharness_bin: Option<String>,

    /// Harness used for evals and the simulated user (oneharness provider only).
    #[arg(long, value_name = "ID")]
    judge_harness: Option<String>,

    /// Per-call timeout in seconds (oneharness provider only).
    #[arg(long, value_name = "SECS")]
    timeout: Option<u64>,

    /// Model used for evals and the simulated user (overrides config).
    #[arg(long, value_name = "MODEL")]
    judge_model: Option<String>,

    /// Cap on assistant turns for multi-turn cases (overrides config).
    #[arg(long, value_name = "N")]
    max_turns: Option<u32>,

    /// Mock/spy declarations (a YAML/JSON list, same shape as a case's `mocks`
    /// block) applied to every case, before the case's own — first match wins.
    /// This is how the SDKs deliver code-level mocks.
    #[arg(long, value_name = "FILE")]
    mocks: Option<PathBuf>,

    /// Record every tool call through the mock/spy channel even for cases with
    /// no `mocks`, so the report carries `mock_calls` (how SDK spies observe).
    #[arg(long)]
    spy: bool,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,
}

#[derive(Args)]
struct ValidateArgs {
    /// Skill directories, or folders containing skill directories.
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,
}

#[derive(Args)]
struct InitArgs {
    /// Directory to scaffold into (default: the current directory).
    #[arg(value_name = "DIR", default_value = ".")]
    dir: PathBuf,
}

#[derive(Args)]
struct SchemaArgs {
    /// Which output contract to describe.
    #[arg(value_enum, value_name = "TARGET")]
    target: SchemaTarget,
}

#[derive(Clone, Copy, ValueEnum)]
enum SchemaTarget {
    /// The `skilltest run --format json` report.
    Report,
    /// The `skilltest validate --format json` report.
    Validation,
    /// The test-case **input** (one `--case-json` object / YAML case file):
    /// the shape the SDKs' generated case models are built from.
    Case,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    /// A compact, human-readable summary.
    Human,
    /// The stable machine-readable JSON report (consumed by the plugins).
    Json,
    /// A live NDJSON stream: one `{"type":"event","event":{…}}` line per tool
    /// event as it happens, then a terminal `{"type":"result","report":{…}}`.
    /// The SDKs' streaming API consumes this; closing the stream short-circuits
    /// the run.
    JsonStream,
}

/// Parse `args` and run the requested command, returning the process exit code.
pub fn run<I, T>(args: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => {
            let _ = err.print();
            return if matches!(
                err.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) {
                ExitCode::Success
            } else {
                ExitCode::UsageError
            };
        }
    };

    let result = match &cli.command {
        Command::Run(args) => cmd_run(cli.config.as_deref(), args),
        Command::Validate(args) => cmd_validate(args),
        Command::Init(args) => cmd_init(args),
        Command::Schema(args) => cmd_schema(args),
    };

    match result {
        Ok(code) => code,
        Err(err) => report_error(&err),
    }
}

fn cmd_run(config_path: Option<&Path>, args: &RunArgs) -> Result<ExitCode> {
    let mut config = match config_path {
        Some(path) => Config::load(path)?,
        None => Config::load_or_default(Path::new("skilltest.yaml"))?,
    };

    let command_provider = args
        .provider
        .as_ref()
        .map(|s| s.split_whitespace().map(String::from).collect::<Vec<_>>());

    config.apply_overrides(Overrides {
        command_provider,
        oneharness_bin: args.oneharness_bin.clone(),
        judge_harness: args.judge_harness.clone(),
        timeout_secs: args.timeout,
        platforms: args.platforms.clone(),
        models: args.models.clone(),
        judge_model: args.judge_model.clone(),
        max_turns: args.max_turns,
        mocks: args
            .mocks
            .as_deref()
            .map(load_mocks_file)
            .transpose()?
            .unwrap_or_default(),
        spy: args.spy,
    })?;

    let provider = build_provider(&config)?;

    let mut cases = Vec::new();
    for path in &args.paths {
        for file in discover_cases(path)? {
            cases.push(TestCase::load(&file)?);
        }
    }
    for file in &args.case_json {
        cases.extend(load_case_json(file)?);
    }
    if cases.is_empty() {
        return Err(Error::Invalid(
            "no test cases given: pass a test-case PATH or --case-json <FILE>".into(),
        ));
    }

    let runner = Runner::new(provider.as_ref(), &config);

    if args.format == Format::JsonStream {
        return run_streamed(&runner, &cases);
    }

    let report = runner.run_all(&cases)?;
    print_report(&report, args.format)?;

    Ok(if report.passed {
        ExitCode::Success
    } else {
        ExitCode::TestFailure
    })
}

/// Drive the run in streaming mode: emit one NDJSON `event` line per tool event
/// as it happens, then a terminal `result` line carrying the full report. A
/// failed stdout write (the consumer closed the stream) short-circuits the run —
/// the runner tears the harness down.
fn run_streamed(runner: &Runner, cases: &[TestCase]) -> Result<ExitCode> {
    let stdout = std::io::stdout();
    let mut sink = |ev: &StreamEvent| -> ControlFlow<()> {
        let line = serde_json::json!({
            "type": "event",
            "case": ev.case,
            "platform": ev.platform,
            "model": ev.model,
            "turn": ev.turn,
            "event": ev.event,
        });
        if write_ndjson(&stdout, &line).is_err() {
            // The consumer closed the stream; ask the runner to short-circuit.
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let report = runner.run_all_streaming(cases, &mut sink)?;
    let passed = report.passed;
    // Best-effort terminal line: the consumer may have already closed the pipe.
    let _ = write_ndjson(
        &stdout,
        &serde_json::json!({ "type": "result", "report": report }),
    );
    Ok(if passed {
        ExitCode::Success
    } else {
        ExitCode::TestFailure
    })
}

/// Serialize `value` as one line to `stdout` and flush. Returns `Err` when the
/// write fails (e.g. the consumer closed the read end — a broken pipe), which
/// the streaming sink uses as the short-circuit signal.
fn write_ndjson(stdout: &std::io::Stdout, value: &serde_json::Value) -> std::io::Result<()> {
    let mut lock = stdout.lock();
    serde_json::to_writer(&mut lock, value)?;
    lock.write_all(b"\n")?;
    lock.flush()
}

/// Load a `--mocks` file: a YAML (or JSON — YAML is a superset here) list of
/// mock/spy declarations, validated by `Config::apply_overrides`.
fn load_mocks_file(path: &Path) -> Result<Vec<skilltest_core::MockDecl>> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        Error::Invalid(format!(
            "could not read --mocks file `{}`: {e}",
            path.display()
        ))
    })?;
    serde_yaml::from_str(&text).map_err(|e| {
        Error::Invalid(format!(
            "--mocks file `{}` is not a valid mock declaration list: {e}",
            path.display()
        ))
    })
}

/// Load a `--case-json` file: one or more fully-specified test cases as JSON
/// (a single case object or an array), the delivery channel the SDKs use for a
/// code-defined case. A code-defined case has no source file, so `skill` paths
/// resolve relative to the working directory.
fn load_case_json(path: &Path) -> Result<Vec<TestCase>> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        Error::Invalid(format!(
            "could not read --case-json file `{}`: {e}",
            path.display()
        ))
    })?;
    let base = std::env::current_dir()
        .map_err(|e| Error::Invalid(format!("could not resolve the working directory: {e}")))?;
    TestCase::from_json(&text, &base)
}

fn build_provider(config: &Config) -> Result<Box<dyn Provider>> {
    let base: Box<dyn Provider> = match &config.provider {
        ProviderConfig::Oneharness(oh) => Box::new(OneharnessProvider::new(oh)),
        ProviderConfig::Command(c) => Box::new(CommandProvider::new(c.command.clone())?),
    };
    // An optional judge backend overrides judging/user-simulation while the base
    // provider keeps running the skill under test.
    match &config.judge {
        Some(JudgeConfig::Api(api)) => Ok(Box::new(SplitProvider::new(
            base,
            ApiJudgeProvider::new(api),
        ))),
        None => Ok(base),
    }
}

fn print_report(report: &Report, format: Format) -> Result<()> {
    match format {
        // `json-stream` is handled by `run_streamed` before this point; if it
        // ever reaches here (e.g. a non-run command), fall back to the buffered
        // JSON document.
        Format::Json | Format::JsonStream => {
            let json = report
                .to_json()
                .map_err(|e| Error::Invalid(format!("could not serialize report: {e}")))?;
            println!("{json}");
        }
        Format::Human => print!("{}", report.to_human()),
    }
    Ok(())
}

fn cmd_validate(args: &ValidateArgs) -> Result<ExitCode> {
    let mut findings = Vec::new();
    for path in &args.paths {
        findings.extend(validate_path(path)?);
    }
    let valid = findings.is_empty();

    match args.format {
        // `validate` has no streaming output; treat `json-stream` as `json`.
        Format::Json | Format::JsonStream => {
            let report = ValidationReport::new(&findings);
            let json = report
                .to_json()
                .map_err(|e| Error::Invalid(format!("could not serialize findings: {e}")))?;
            println!("{json}");
        }
        Format::Human => {
            if valid {
                println!("OK    all skill definitions valid");
            } else {
                for finding in &findings {
                    eprintln!("INVALID  {}: {}", finding.skill.display(), finding.message);
                }
                eprintln!("FAIL  {} validation finding(s)", findings.len());
            }
        }
    }

    Ok(if valid {
        ExitCode::Success
    } else {
        ExitCode::TestFailure
    })
}

fn cmd_init(args: &InitArgs) -> Result<ExitCode> {
    let created = crate::scaffold::scaffold(&args.dir)?;
    for path in &created {
        println!("created {}", path.display());
    }
    println!(
        "\nNext: skilltest run cases/example.yaml\n\
         Try it offline:  skilltest run cases/example.yaml --provider skilltest-fake-provider"
    );
    Ok(ExitCode::Success)
}

fn cmd_schema(args: &SchemaArgs) -> Result<ExitCode> {
    // Draft-07 on purpose: it is the dialect the SDK model generators
    // (datamodel-code-generator, json-schema-to-typescript) digest reliably.
    let generator = schemars::generate::SchemaSettings::draft07().into_generator();
    let schema = match args.target {
        SchemaTarget::Report => generator.into_root_schema_for::<Report>(),
        SchemaTarget::Validation => generator.into_root_schema_for::<ValidationReport>(),
        SchemaTarget::Case => generator.into_root_schema_for::<TestCase>(),
    };
    let json = serde_json::to_string_pretty(&schema)
        .map_err(|e| Error::Invalid(format!("could not serialize schema: {e}")))?;
    println!("{json}");
    Ok(ExitCode::Success)
}

fn report_error(err: &Error) -> ExitCode {
    eprintln!("error: {err}");
    match err {
        Error::Provider { kind, .. } => {
            let hint = match kind.as_deref() {
                Some("auth") => "hint: authentication failed — check your provider credentials (e.g. `claude` login)",
                Some("rate_limit") => "hint: the harness rate-limited the call — retry after a backoff",
                Some("model_not_found") => {
                    "hint: the harness does not recognize this model — check `--model` and `oneharness list`"
                }
                Some("quota") => "hint: provider quota exhausted — check your account limits",
                Some("overloaded") => "hint: the API is temporarily overloaded — retried already; try again shortly",
                Some(other) => {
                    eprintln!("classified as: {other}");
                    "hint: see provider docs for this failure class"
                }
                None => {
                    "hint: ensure the provider command is installed and on PATH, or pass --provider"
                }
            };
            eprintln!("{hint}");
            ExitCode::ProviderError
        }
        _ => ExitCode::UsageError,
    }
}
