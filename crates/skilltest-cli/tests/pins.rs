//! The oneharness version pin, reconciled across every place that restates it.
//!
//! Not an e2e journey — it drives no CLI. It is a repository-consistency gate,
//! kept beside the e2e suites because the thing it protects is the *published*
//! artifact's environment: `scripts/install-oneharness.sh` is what CI's live
//! e2e installs, while the SDKs bundle an `oneharness-cli` of their own. When
//! those disagree, skilltest's provider is built against one oneharness and
//! handed another, and nothing says so until a live run fails.

use std::path::PathBuf;

// llmlint: ignore-block[code_lands_in_the_domain_that_owns_it] Reading cross-language files from the CLI project has precedent: the contract check (`assert_schema_matches_golden` in tests/e2e.rs) already reads `schemas/` from here. The pin these manifests restate is the oneharness the CLI's provider drives, so the CLI project owns the reconciliation.
fn repo_file(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn read_repo_file(rel: &str) -> String {
    std::fs::read_to_string(repo_file(rel)).unwrap_or_else(|e| panic!("{rel} is readable: {e}"))
}

/// The one authored copy of the targeted version: the installer's
/// `default_version`. Parsed strictly — a tag-shaped `vMAJOR.MINOR.PATCH` with
/// three numeric components and nothing else — so a malformed pin fails here
/// rather than propagating a half-parsed version into the comparisons below.
fn installer_default_version() -> (String, u64, u64, u64) {
    let script = read_repo_file("scripts/install-oneharness.sh");
    let raw = script
        .lines()
        .find_map(|l| l.strip_prefix("default_version=\"")?.strip_suffix('"'))
        .expect("scripts/install-oneharness.sh declares default_version=\"vX.Y.Z\"")
        .to_string();
    let bare = raw
        .strip_prefix('v')
        .unwrap_or_else(|| panic!("default_version must be tag-shaped (vX.Y.Z), got {raw}"));
    let parts: Vec<&str> = bare.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "default_version must have exactly three components, got {raw}"
    );
    let mut nums = [0u64; 3];
    for (slot, part) in nums.iter_mut().zip(&parts) {
        *slot = part
            .parse()
            .unwrap_or_else(|_| panic!("{raw} has a non-numeric component `{part}`"));
    }
    (bare.to_string(), nums[0], nums[1], nums[2])
}

/// The operational pins: the installer, the `just install-oneharness` recipe
/// that mirrors it, and each SDK's `oneharness-cli` bound.
#[test]
fn oneharness_pin_is_lockstep_across_installer_recipe_and_both_sdks() {
    let (bare, major, minor, _patch) = installer_default_version();

    let justfile = read_repo_file("justfile");
    assert!(
        justfile.contains(&format!("install-oneharness version=\"v{bare}\":")),
        "the `just install-oneharness` default must mirror the installer's v{bare}"
    );

    // `>=X.Y.Z,<X.(Y+1)` — admits the targeted release and its patch line, and
    // nothing older or from a later minor.
    let want_python = format!("\"oneharness-cli>={bare},<{major}.{}\"", minor + 1);
    assert!(
        read_repo_file("sdks/python/pyproject.toml").contains(&want_python),
        "sdks/python/pyproject.toml must bound oneharness-cli as {want_python} to match v{bare}"
    );

    // npm's caret on a 0.x version is `>=0.Y.Z <0.(Y+1).0` — the same window.
    let want_ts = format!("\"oneharness-cli\": \"^{bare}\"");
    assert!(
        read_repo_file("sdks/typescript/package.json").contains(&want_ts),
        "sdks/typescript/package.json must bound oneharness-cli as {want_ts} to match v{bare}"
    );
}

/// The prose that tells a reader (and the next agent) which release skilltest
/// targets. Stale here is worse than absent: it sends someone to the wrong
/// CLI's documentation for the argv and report the provider depends on.
#[test]
fn documented_oneharness_version_matches_the_installer_pin() {
    let (bare, _major, _minor, _patch) = installer_default_version();
    let want = format!("v{bare}");
    for doc in [
        "AGENTS.md",
        "README.md",
        "docs/protocol.md",
        "crates/skilltest-core/src/provider.rs",
    ] {
        assert!(
            read_repo_file(doc).contains(&want),
            "{doc} must name the targeted oneharness release ({want}); \
             scripts/install-oneharness.sh is the authority"
        );
    }
}
// llmlint: ignore-end[code_lands_in_the_domain_that_owns_it]
