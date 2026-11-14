//! The repository's tracked `.ri` corpus, as git's index sees it.
//!
//! Every repo-wide `.ri` sweep in a binary that includes this module reads
//! [`swept_ri_paths`], so the sweeps agree on what "the corpus" is, including
//! that it omits [`VERIFY_INERT_RI_TREES`].

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use reify_test_support::{TempDir, prefixed_tempdir};

/// Trees whose `.ri` files verify's scope classifier marks inert (the `inert`
/// fact of `scripts/reify_path_attribution.py`), so a change there runs no
/// heavy checks. Docs land on `main` by a hook-gated commit, so a sweep that
/// read a `.ri` here would let such a commit land green and then red every
/// later merge gate. Each entry ends with `/`, so it cannot match a sibling
/// tree, and `is_verify_inert_agrees_with_the_scope_classifier` holds the table
/// equal to the classifier. Recorded on the guard row of
/// `docs/prds/v0_6/syntax-lowering-contract.md` §7.
pub const VERIFY_INERT_RI_TREES: &[&str] = &["docs/"];

/// Whether the repo-relative `rel` lies in a [`VERIFY_INERT_RI_TREES`] tree.
pub fn is_verify_inert(rel: &str) -> bool {
    VERIFY_INERT_RI_TREES
        .iter()
        .any(|tree| rel.starts_with(tree))
}

/// The corpus every repo-wide sweep reads: [`tracked_ri_paths`] without the
/// [`is_verify_inert`] paths, still sorted.
pub fn swept_ri_paths(root: &Path) -> Vec<String> {
    let mut paths = tracked_ri_paths(root);
    paths.retain(|rel| !is_verify_inert(rel));
    paths
}

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
fn tracked_ri_paths(root: &Path) -> Vec<String> {
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

/// A throwaway git repository whose index tracks each `(repo-relative path,
/// source)` pair. No commit is made: `git ls-files` reads the index.
pub fn tracked_fixture_repo(files: &[(&str, &str)]) -> TempDir {
    let dir = prefixed_tempdir("reify-tracked-corpus-fixture-");
    for (rel, source) in files {
        let path = dir.path().join(rel);
        let parent = path.parent().expect("a repo-relative path has a parent");
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("cannot create {}: {e}", parent.display()));
        std::fs::write(&path, source)
            .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
    }
    for args in [&["init", "-q"][..], &["add", "--", "."][..]] {
        let output = reify_git_env::command(dir.path())
            .args(args)
            .output()
            .unwrap_or_else(|e| panic!("cannot run `git {}`: {e}", args.join(" ")));
        assert!(
            output.status.success(),
            "`git {}` failed in {} ({}): {}",
            args.join(" "),
            dir.path().display(),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    dir
}

/// The scope classifier's `inert` fact for each of `paths`, in order, from
/// `scripts/reify_path_attribution.py attribute --format tsv` under `root`.
fn classifier_inert_facts(root: &Path, paths: &[String]) -> Vec<bool> {
    let script = root.join("scripts/reify_path_attribution.py");
    let mut child = Command::new("python3")
        .arg("-I")
        .arg(&script)
        .args(["attribute", "--format", "tsv"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("cannot spawn python3 {}: {e}", script.display()));
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let input: String = paths.iter().map(|path| format!("{path}\n")).collect();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|e| panic!("cannot wait for python3 {}: {e}", script.display()));
    writer
        .join()
        .expect("the stdin writer thread does not panic")
        .unwrap_or_else(|e| panic!("cannot write paths to {}: {e}", script.display()));
    assert!(
        output.status.success(),
        "{} attribute failed ({}): {}",
        script.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    // One record per input path, in order:
    // `<is_global>\t<inert>\t<tree>\t<seed_crates>\t<unioned_crates>\t<path>`.
    let stdout = String::from_utf8(output.stdout).expect("classifier output must be UTF-8");
    let records: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        records.len(),
        paths.len(),
        "the classifier must print one record per path"
    );
    paths
        .iter()
        .zip(records)
        .map(|(path, record)| {
            let fields: Vec<&str> = record.split('\t').collect();
            match fields.as_slice() {
                [_, "0", _, _, _, echoed] if echoed == path => false,
                [_, "1", _, _, _, echoed] if echoed == path => true,
                _ => panic!("unexpected classifier record for {path}: {record:?}"),
            }
        })
        .collect()
}

// ── this module's own behaviour ─────────────────────────────────────────────

#[test]
fn verify_inert_trees_are_slash_terminated() {
    let unterminated: Vec<&str> = VERIFY_INERT_RI_TREES
        .iter()
        .copied()
        .filter(|tree| !tree.ends_with('/'))
        .collect();
    assert!(
        unterminated.is_empty(),
        "VERIFY_INERT_RI_TREES entries must end with '/' so they cannot match a sibling \
         tree: {unterminated:?}"
    );
}

#[test]
fn swept_ri_paths_omits_verify_inert_trees() {
    let dir = tracked_fixture_repo(&[
        ("docs/zz_section/zz_synthetic.ri", ""),
        ("docsx/zz_sibling.ri", ""),
        ("examples/zz_kept.ri", ""),
    ]);
    assert_eq!(
        swept_ri_paths(dir.path()),
        ["docsx/zz_sibling.ri", "examples/zz_kept.ri"],
        "a docs/ member leaves the listing; docsx/ shares the name's prefix but is not the tree"
    );
}

/// `is_verify_inert` and the scope classifier agree on every tracked `.ri`,
/// and on a probe inside and beside each table entry, so the probes still
/// test both answers when no tracked `.ri` lies in a table tree.
#[test]
fn is_verify_inert_agrees_with_the_scope_classifier() {
    let root = workspace_root();
    let mut paths = tracked_ri_paths(&root);
    for tree in VERIFY_INERT_RI_TREES {
        paths.push(format!("{tree}zz_inert_probe.ri"));
        paths.push(format!(
            "{}x/zz_sibling_probe.ri",
            tree.trim_end_matches('/')
        ));
    }
    let disagreements: Vec<String> = paths
        .iter()
        .zip(classifier_inert_facts(&root, &paths))
        .filter(|(path, inert)| is_verify_inert(path) != *inert)
        .map(|(path, inert)| {
            format!(
                "{path}: classifier inert={inert}, is_verify_inert={}",
                !inert
            )
        })
        .collect();
    assert!(
        disagreements.is_empty(),
        "VERIFY_INERT_RI_TREES disagrees with scripts/reify_path_attribution.py's inert \
         fact; change the table to match the classifier:\n{}",
        disagreements.join("\n")
    );
}
