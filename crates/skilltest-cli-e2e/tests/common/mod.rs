//! Shared by every suite in this crate: where the built binaries are.

use std::path::PathBuf;

/// A binary `skilltest-cli:build` produced (`skilltest`, or the
/// `skilltest-fake-provider` its `fake-provider` feature adds).
///
/// This crate is not the binaries' own package, so cargo gives it no
/// `CARGO_BIN_EXE_*`. Cargo puts test executables in `<target>/<profile>/deps/`
/// and workspace binaries in `<target>/<profile>/`, so resolve beside this test
/// executable. That holds for a plain build (`target/debug`) and under `cargo
/// llvm-cov` (its own target dir, where it builds the CLI's binaries
/// instrumented, so these suites still count toward the CLI's coverage).
pub fn built_bin(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("the test executable has a path");
    let path = exe
        .parent()
        .and_then(std::path::Path::parent)
        .expect("test executables live in <target>/<profile>/deps")
        .join(name);
    assert!(
        path.is_file(),
        "{} is missing: build the CLI first (`cargo build -p skilltest-cli --features \
         fake-provider`, or run these suites through nx, whose \
         `skilltest-cli-e2e:test` target depends on `skilltest-cli:build`)",
        path.display()
    );
    path
}
