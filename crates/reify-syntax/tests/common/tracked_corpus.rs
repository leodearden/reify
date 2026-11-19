//! The repository's tracked `.ri` corpus, as git's index sees it.
//!
//! Every repo-wide `.ri` sweep in a binary that includes this module reads
//! [`swept_ri_paths`], so the sweeps agree on what "the corpus" is, including
//! that it omits every path verify's scope classifier marks inert.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use reify_test_support::{TempDir, prefixed_tempdir};

/// The corpus every repo-wide sweep reads: [`tracked_ri_paths`] without every
/// path verify's scope classifier (the `inert` fact of
/// `scripts/reify_path_attribution.py`) marks inert, still sorted. The
/// classifier is asked once per listing.
///
/// A change to an inert path runs no heavy checks, and docs land on `main` by
/// a hook-gated commit, so a sweep that read such a `.ri` would let that commit
/// land green and then red every later merge gate. Recorded on the guard row of
/// `docs/prds/v0_6/syntax-lowering-contract.md` §7.
pub fn swept_ri_paths(root: &Path) -> Vec<String> {
    swept_ri_paths_classified_by(root, &scope_classifier())
}

/// A repo-relative `.ri` path verify's scope classifier marks inert, for the
/// tests that need one. `docs/` is the tree its `_INERT_GLOBS` marks inert
/// today; if that set is narrowed past this path, repoint this one constant.
pub const INERT_PROBE_RI: &str = "docs/zz_section/zz_synthetic.ri";

/// Repo-relative paths split by a scope classifier's `inert` fact, each side
/// in input order.
pub struct InertPartition<T> {
    pub inert: Vec<T>,
    pub not_inert: Vec<T>,
}

/// `paths` split by verify's scope-classifier `inert` fact, from one
/// classifier run.
pub fn partition_by_verify_inert<T: AsRef<str>>(paths: Vec<T>) -> InertPartition<T> {
    partition_inert_by(&scope_classifier(), paths)
}

/// Verify's own scope classifier, taken from this workspace rather than from
/// a listing root, so a fixture repo, which carries no `scripts/`, is
/// classified by the real one.
fn scope_classifier() -> PathBuf {
    workspace_root().join("scripts/reify_path_attribution.py")
}

/// [`tracked_ri_paths`] under `root` without the paths `classifier` marks
/// inert, still sorted.
fn swept_ri_paths_classified_by(root: &Path, classifier: &Path) -> Vec<String> {
    partition_inert_by(classifier, tracked_ri_paths(root)).not_inert
}

/// `paths` split by `classifier`'s `inert` fact, from one classifier run.
fn partition_inert_by<T: AsRef<str>>(classifier: &Path, paths: Vec<T>) -> InertPartition<T> {
    let facts = classifier_inert_facts(classifier, &paths);
    let mut partition = InertPartition {
        inert: Vec::new(),
        not_inert: Vec::new(),
    };
    for (path, inert) in paths.into_iter().zip(facts) {
        if inert {
            partition.inert.push(path);
        } else {
            partition.not_inert.push(path);
        }
    }
    partition
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

/// The scope classifier's `inert` fact for each of `paths`, in order, from one
/// `<classifier> attribute --format tsv` run.
fn classifier_inert_facts(classifier: &Path, paths: &[impl AsRef<str>]) -> Vec<bool> {
    let mut child = Command::new("python3")
        .arg("-I")
        .arg(classifier)
        .args(["attribute", "--format", "tsv"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("cannot spawn python3 {}: {e}", classifier.display()));
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let input: String = paths
        .iter()
        .map(|path| format!("{}\n", path.as_ref()))
        .collect();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|e| panic!("cannot wait for python3 {}: {e}", classifier.display()));
    assert!(
        output.status.success(),
        "{} attribute failed ({}): {}",
        classifier.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    writer
        .join()
        .expect("the stdin writer thread does not panic")
        .unwrap_or_else(|e| panic!("cannot write paths to {}: {e}", classifier.display()));

    // One record per input path, in order:
    // `<is_global>\t<inert>\t<tree>\t<seed_crates>\t<unioned_crates>\t<path>`.
    let stdout = String::from_utf8(output.stdout).expect("classifier output must be UTF-8");
    let records: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        records.len(),
        paths.len(),
        "{} must print one record per path",
        classifier.display()
    );
    paths
        .iter()
        .zip(records)
        .map(|(path, record)| {
            let path = path.as_ref();
            let fields: Vec<&str> = record.split('\t').collect();
            match fields.as_slice() {
                [_, "0", _, _, _, echoed] if *echoed == path => false,
                [_, "1", _, _, _, echoed] if *echoed == path => true,
                _ => panic!("unexpected classifier record for {path}: {record:?}"),
            }
        })
        .collect()
}

// ── this module's own behaviour ─────────────────────────────────────────────

#[test]
fn swept_ri_paths_omits_what_the_scope_classifier_marks_inert() {
    let dir = tracked_fixture_repo(&[(INERT_PROBE_RI, ""), ("examples/zz_kept.ri", "")]);
    assert_eq!(
        swept_ri_paths(dir.path()),
        ["examples/zz_kept.ri"],
        "swept_ri_paths must drop what verify's real scope classifier marks inert; if \
         _INERT_GLOBS no longer covers INERT_PROBE_RI, repoint that constant"
    );
}

/// A stand-in for the scope classifier that honours its `attribute --format
/// tsv` record contract but marks only `notes/` inert.
const FAKE_SCOPE_CLASSIFIER: &str = r#"import sys

for line in sys.stdin:
    path = line.rstrip("\n")
    inert = 1 if path.startswith("notes/") else 0
    sys.stdout.write(f"0\t{inert}\tother\t-\t-\t{path}\n")
"#;

#[test]
fn swept_ri_paths_follows_the_classifier_it_is_given() {
    let repo = tracked_fixture_repo(&[
        ("docs/zz_section/zz_synthetic.ri", ""),
        ("examples/zz_kept.ri", ""),
        ("notes/zz_section/zz_synthetic.ri", ""),
    ]);
    let classifier_dir = prefixed_tempdir("reify-fake-scope-classifier-");
    let script = classifier_dir.path().join("fake_scope_classifier.py");
    std::fs::write(&script, FAKE_SCOPE_CLASSIFIER)
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", script.display()));
    assert_eq!(
        swept_ri_paths_classified_by(repo.path(), &script),
        ["docs/zz_section/zz_synthetic.ri", "examples/zz_kept.ri"],
        "the listing drops exactly what the classifier marks inert: a tree it newly marks \
         (notes/) leaves the listing, and a tree it does not mark (docs/) stays"
    );
}

/// A stand-in for the scope classifier that exits before reading stdin.
const EARLY_EXIT_CLASSIFIER: &str = r#"import sys

sys.stderr.write("zz-classifier-stderr-marker: exited before reading its paths\n")
sys.exit(3)
"#;

/// About 2 MiB of paths: more than a pipe buffer holds, so the write to the
/// classifier's stdin is certain to hit its exit rather than race it.
#[test]
#[should_panic(expected = "zz-classifier-stderr-marker")]
fn a_classifier_that_exits_before_reading_is_reported_with_its_stderr() {
    let classifier_dir = prefixed_tempdir("reify-early-exit-classifier-");
    let script = classifier_dir.path().join("early_exit_classifier.py");
    std::fs::write(&script, EARLY_EXIT_CLASSIFIER)
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", script.display()));
    let paths = vec![format!("{}.ri", "z".repeat(4096)); 512];
    partition_inert_by(&script, paths);
}
