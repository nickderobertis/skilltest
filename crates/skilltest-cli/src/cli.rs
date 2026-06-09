//! Argument parsing and command dispatch for the `skilltest` CLI.
//!
//! Output discipline (see AGENTS.md): minimal on success, the exact problem plus
//! a suggested action on stderr, and a distinct [`ExitCode`] per failure class.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use skilltest_core::{
    discover_cases, validate_path, CommandProvider, Config, Error, ExitCode, Overrides, Report,
    Result, Runner, TestCase,
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

    /// Provider command, overriding config. Whitespace-split into argv, e.g.
    /// `--provider oneharness` or `--provider "python3 provider.py"`.
    #[arg(long, value_name = "CMD")]
    provider: Option<String>,

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

    let provider_argv = args
        .provider
        .as_ref()
        .map(|s| s.split_whitespace().map(String::from).collect::<Vec<_>>());

    config.apply_overrides(Overrides {
        provider: provider_argv,
        platforms: args.platforms.clone(),
        models: args.models.clone(),
        judge_model: args.judge_model.clone(),
        max_turns: args.max_turns,
    })?;

    let provider = CommandProvider::new(config.provider.clone())?;

    let mut cases = Vec::new();
    for path in &args.paths {
        for file in discover_cases(path)? {
            cases.push(TestCase::load(&file)?);
        }
    }

    let runner = Runner::new(&provider, &config);
    let report = runner.run_all(&cases)?;

    print_report(&report, args.format)?;

    Ok(if report.passed {
        ExitCode::Success
    } else {
        ExitCode::TestFailure
    })
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
            let report = serde_json::json!({
                "valid": valid,
                "findings": findings
                    .iter()
                    .map(|f| serde_json::json!({
                        "skill": f.skill.to_string_lossy(),
                        "message": f.message,
                    }))
                    .collect::<Vec<_>>(),
            });
            let json = serde_json::to_string_pretty(&report)
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

fn report_error(err: &Error) -> ExitCode {
    eprintln!("error: {err}");
    match err {
        Error::Provider { .. } => {
            eprintln!(
                "hint: ensure the provider command is installed and on PATH, or pass --provider"
            );
            ExitCode::ProviderError
        }
        _ => ExitCode::UsageError,
    }
}
