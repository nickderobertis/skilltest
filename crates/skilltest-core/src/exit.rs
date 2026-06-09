//! Documented process exit codes. Defined in the core so they are part of the
//! library's contract; the CLI maps [`crate::Error`] onto them.
//!
//! These codes are a stable contract: scripts and CI branch on them, so do not
//! renumber existing variants.

/// Stable exit codes for the `skilltest` CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    /// Everything ran and every case/eval passed.
    Success = 0,
    /// The run completed but at least one test case failed its evals, or a
    /// skill failed validation. The tool worked; the thing under test did not.
    TestFailure = 1,
    /// Bad usage or bad input: malformed config, malformed test-case YAML, a
    /// missing file. The user must fix the input.
    UsageError = 2,
    /// The provider command failed: not found, crashed, or returned output that
    /// did not satisfy the protocol. The environment must be fixed.
    ProviderError = 3,
}

impl ExitCode {
    /// The raw integer code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }
}
