//! The release commit stages everything `scripts/set-version.sh` rewrites.
//!
//! Not an e2e journey — it drives no CLI and spawns nothing. It is a
//! repository-consistency gate, like `pins.rs` beside it: the lockstep version
//! is written by `scripts/set-version.sh`, and `.releaserc.json` independently
//! restates the files semantic-release then commits. When the two lists
//! disagree, the release *succeeds* — it just publishes a manifest or lockfile
//! that still carries the previous version, and nothing says so until an
//! install resolves the wrong one.
//!
//! The script is the authority here: every manifest it rewrites and every
//! lockfile it refreshes must be a git asset, and every asset must exist.

use std::collections::BTreeSet;
use std::path::PathBuf;

// llmlint: ignore[code_lands_in_the_domain_that_owns_it] Every read below goes through this one helper. Reading repo-root release files from the CLI project follows `pins.rs` and the contract check in `tests/e2e.rs`, which already read cross-language files from here; the version these files restate is the published CLI's own, and no scripts- or release-owned project exists to hold the reconciliation.
fn repo_file(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn read_repo_file(rel: &str) -> String {
    std::fs::read_to_string(repo_file(rel)).unwrap_or_else(|e| panic!("{rel} is readable: {e}"))
}

fn release_assets() -> BTreeSet<String> {
    let config: serde_json::Value = serde_json::from_str(&read_repo_file(".releaserc.json"))
        .expect(".releaserc.json is valid JSON");
    let assets = config["plugins"]
        .as_array()
        .expect("plugins is an array")
        .iter()
        .find_map(|plugin| {
            let entry = plugin.as_array()?;
            (entry.first()?.as_str()? == "@semantic-release/git").then(|| entry.get(1))?
        })
        .expect(".releaserc.json configures @semantic-release/git")["assets"]
        .as_array()
        .expect("the git plugin lists assets")
        .iter()
        .map(|a| a.as_str().expect("each asset is a path string").to_string())
        .collect::<BTreeSet<String>>();
    assert!(
        !assets.is_empty(),
        "the git plugin's asset list is not empty"
    );
    assets
}

/// Everything `set-version.sh` writes, read out of the script itself: the last
/// argument of each `perl -i -pe` rewrite (the file it edits in place), the
/// platform manifests its one `for` loop globs, and the lockfile each
/// `run "refresh <path>"` step regenerates.
fn files_set_version_writes() -> BTreeSet<String> {
    let script = read_repo_file("scripts/set-version.sh");
    let mut written = BTreeSet::new();
    for line in script.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("perl -i -pe ") {
            let target = rest.split_whitespace().last().expect("perl edits a file");
            // `"$pkg"` — the loop variable, covered by the glob below.
            if !target.starts_with('"') {
                written.insert(target.to_string());
            }
        }
        if let Some(rest) = line.strip_prefix("for pkg in ") {
            let glob = rest.split(';').next().expect("the loop names a glob");
            written.extend(expand_one_star(glob));
        }
        if let Some(rest) = line.strip_prefix("run \"refresh ") {
            let path = rest.split('"').next().expect("the label names the file");
            written.insert(path.to_string());
        }
    }
    assert!(
        written.len() > 3,
        "set-version.sh must still be parsed as a list of files it writes, got {written:?}"
    );
    written
}

/// Expand a `dir/*/name` glob (the only shape the script uses) against the tree.
fn expand_one_star(glob: &str) -> Vec<String> {
    let (prefix, suffix) = glob.split_once("/*/").expect("a dir/*/name glob");
    let mut matches: Vec<String> = std::fs::read_dir(repo_file(prefix))
        .unwrap_or_else(|e| panic!("{prefix} is a directory: {e}"))
        .map(|entry| entry.expect("readable dir entry").file_name())
        .filter(|name| repo_file(prefix).join(name).join(suffix).is_file())
        .map(|name| format!("{prefix}/{}/{suffix}", name.to_string_lossy()))
        .collect();
    matches.sort();
    assert!(!matches.is_empty(), "{glob} matches nothing");
    matches
}

/// A file the script rewrites but the release commit does not stage keeps its
/// old version in git while the tag says otherwise.
// llmlint: ignore[shell_test_tiers_stay_split] No scripts-owned project exists — the same reason `pins.rs` records beside its installer gate — and this test spawns nothing: it reads two repo files and finishes in ~0.01s, inside a tier `just coverage` runs in full on every `just check`.
#[test]
fn every_file_set_version_writes_is_staged_by_the_release_commit() {
    let assets = release_assets();
    for written in files_set_version_writes() {
        assert!(
            assets.contains(&written),
            "scripts/set-version.sh writes {written}, so .releaserc.json's git assets must \
             stage it — otherwise the release commit leaves it at the previous version"
        );
    }
}

/// And the reverse: an asset that no longer exists (a lockfile that moved, say)
/// is dead configuration that silently stages nothing.
// llmlint: ignore[shell_test_tiers_stay_split] No scripts-owned project exists — the same reason `pins.rs` records beside its installer gate — and this test spawns nothing: it reads two repo files and finishes in ~0.01s, inside a tier `just coverage` runs in full on every `just check`.
#[test]
fn every_release_asset_exists_in_the_tree() {
    for asset in release_assets() {
        assert!(
            repo_file(&asset).exists(),
            ".releaserc.json stages {asset}, which is not in the tree — drop it or restore it"
        );
    }
}
