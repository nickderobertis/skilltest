//! Entry point for the `skilltest` CLI. All logic lives in [`cli`]; `main` only
//! translates the resulting [`skilltest_core::ExitCode`] into a process exit.

mod cli;

fn main() {
    let code = cli::run(std::env::args_os());
    std::process::exit(code.code());
}
