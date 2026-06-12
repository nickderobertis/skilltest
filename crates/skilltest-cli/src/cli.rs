//! Argument parsing and command dispatch for the `skilltest` CLI.
//!
//! Output discipline (see AGENTS.md): minimal on success, the exact problem plus
//! a suggested action on stderr, and a distinct [`ExitCode`] per failure class.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use skilltest_core::{
    discover_cases, validate_path, ApiJudgeProvider, CommandProvider, Config, Error, ExitCode,
    JudgeConfig, OneharnessProvider, Overrides, Provider, ProviderConfig, Report, Result, Runner,
    SplitProvider, TestCase, ValidationReport,
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
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,

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
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    /// A compact, human-readable summary.
    Human,
    /// The stable machine-readable JSON report (consumed by the plugins).
    Json,
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
    })?;

    let provider = build_provider(&config)?;

    let mut cases = Vec::new();
    for path in &args.paths {
        for file in discover_cases(path)? {
            cases.push(TestCase::load(&file)?);
        }
    }

    let runner = Runner::new(provider.as_ref(), &config);
    let report = runner.run_all(&cases)?;

    print_report(&report, args.format)?;

    Ok(if report.passed {
        ExitCode::Success
    } else {
        ExitCode::TestFailure
    })
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
        Format::Json => {
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
        Format::Json => {
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
