//! The repository's tracked `.ri` corpus, as git's index sees it.
//!
//! Every repo-wide `.ri` sweep in this test binary lists its inputs here, so
//! the sweeps agree on what "the corpus" is.

use std::path::{Path, PathBuf};

/// The workspace root, resolved from this crate's manifest dir
/// (`<root>/crates/reify-syntax` → up two).
pub fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be <workspace>/crates/reify-syntax")
        .to_path_buf()
}

/// Every `*.ri` file TRACKED by git under `root`, as sorted repo-relative
/// paths exactly as git prints them (forward slashes).
///
/// Asked of git rather than derived by walking the filesystem: a sweep over
/// this listing makes a claim about this repository's SOURCE, and a filesystem
/// walk cannot tell source from a scratch design a developer is mid-way through
/// authoring (the reify-design workflow writes `.ri` files in-tree) or from a
/// fixture some aborted run left behind. Either would turn a sweep red while
/// asserting, wrongly, something about tracked source.
///
/// Built through `reify_git_env::command`, so an ambient `GIT_DIR`/
/// `GIT_WORK_TREE` cannot point this `git -C <root>` at a different repository.
pub fn tracked_ri_paths(root: &Path) -> Vec<String> {
    let output = reify_git_env::command(root)
        .args(["ls-files", "-z", "--", "*.ri"])
        .output()
        .unwrap_or_else(|e| panic!("cannot run `git ls-files` in {}: {e}", root.display()));
    assert!(
        output.status.success(),
        "`git ls-files` failed in {} ({}): {}",
        root.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let listing = String::from_utf8(output.stdout).expect("git ls-files output must be UTF-8");
    let mut out: Vec<String> = listing
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect();
    out.sort();
    out
}
