//! Git-environment helpers shared by the reify-audit integration test
//! binaries.
//!
//! - [`git_cmd`] — the constructor every fixture-repo helper should use.
//! - [`decoy_repo`] / [`poison_with_hook_git_env`] — a stand-in for the parent
//!   repository a hook would point at, and that hook's exported environment
//!   applied to a command.
//! - [`replay_self_under_hook_git_env`] / [`replay_child_command`] — re-run
//!   this binary's own tests with that environment genuinely ambient.
//! - [`ReplayMark`] / [`replay_child_expects_envelope`] /
//!   [`assert_not_in_replay_child`] — what a replay child may conclude about
//!   the parent that spawned it.
//!
//! Generic git-environment plumbing only: this module is compiled into every
//! `tests/*.rs` in this crate, so a helper hard-coding one script's path, argv
//! or skip protocol belongs in the binary that consumes it.
//!
//! Nothing here duplicates a variable list: the *sanitized* set lives once in
//! [`reify_audit::git_env::REPO_REDIRECT_VARS`], and the *poisoned* set lives
//! once in [`hook_git_env`], which asserts it is a subset of the sanitized one.
//!
//! # Why a replay harness
//!
//! The reported condition is a hook environment: `hooks/pre-commit` ->
//! `hooks/project-checks` -> `scripts/verify.sh` -> the workspace test run,
//! with `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE` exported into the whole
//! process tree. Reproducing that *inside* a test would mean mutating the test
//! process's own environment, and `std::env::set_var` is process-global: under
//! nextest's process-per-test isolation it would appear to work, hiding the
//! hazard, while under `cargo test`'s thread-per-test model it would race and
//! intermittently poison sibling tests — trading a deterministic bug for a
//! flaky one. So instead of poisoning ourselves, we re-exec ourselves poisoned:
//! spawn `current_exe()` with the poison in the CHILD's environment, where it
//! is genuinely ambient for every test that child runs.
//!
//! # Why the replay counts tests
//!
//! libtest exits 0 when a filter matches ZERO tests, so asserting only on the
//! child's exit status would turn this harness into a silent green the instant
//! a filter stops matching — a rename, a dropped `mod` wrapper, a test moving
//! to another binary. The replay therefore lists the selection first, requires
//! it to meet the caller's declared floor, and then requires the poisoned run
//! to account for every listed test.

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// Environment variable marking the replayed child process, so the replay test
/// does not recurse when the child re-runs it.
const REPLAY_GUARD: &str = "REIFY_AUDIT_HOOK_ENV_REPLAY";

/// [`REPLAY_GUARD`]'s value for a child whose parent verified NOTHING about
/// this environment beyond the fact that it is replaying.
const REPLAY_PLAIN_MARK: &str = "1";

/// [`REPLAY_GUARD`]'s value for a child whose parent had itself seen an audit
/// envelope in this same environment moments before spawning it.
const REPLAY_ENVELOPE_MARK: &str = "envelope";

/// Which claim a replay child's [`REPLAY_GUARD`] value carries.
///
/// Callers name the claim and this enum resolves it to the value, so each mark
/// keeps exactly one spelling and a drift between stamping and reading it is a
/// compile error rather than something a runtime check must catch.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayMark {
    /// The parent verified NOTHING beyond the fact that it is replaying.
    Plain,
    /// The parent saw an audit envelope in this same environment moments
    /// before spawning. See [`replay_child_expects_envelope`].
    Envelope,
}

impl ReplayMark {
    /// The value stamped into [`REPLAY_GUARD`] for a child carrying this claim.
    fn value(self) -> &'static str {
        match self {
            ReplayMark::Plain => REPLAY_PLAIN_MARK,
            ReplayMark::Envelope => REPLAY_ENVELOPE_MARK,
        }
    }
}

/// True when this process is a replay child, spawned under EITHER mark.
///
/// PRIVATE on purpose: a `bool` answering mere child-ness is usable to tighten
/// a graceful skip, which only [`replay_child_expects_envelope`] may do.
/// Callers get the weak question as the refusal [`assert_not_in_replay_child`]
/// instead. The only other reader is [`replay_self_under_hook_git_env_with_mark`]'s
/// re-entrancy guard, which asks the same question for the same reason.
fn in_replay_child() -> bool {
    std::env::var_os(REPLAY_GUARD).is_some()
}

/// Refuse to proceed if this process is a replay child of ANY mark.
///
/// For a helper that is unsound or pointless inside a replay child. `helper`
/// names the caller and `consequence` says what would go wrong; both land in
/// the panic message, so the diagnosis stays with the helper that knows it.
#[allow(dead_code)]
pub fn assert_not_in_replay_child(helper: &str, consequence: &str) {
    assert!(
        !in_replay_child(),
        "{helper} must not run inside a replay child — {consequence}. Narrow the \
         replay filter so it does not select this helper's caller."
    );
}

/// True when this process is a replay child whose parent had itself seen an
/// audit envelope in this same environment.
///
/// The one predicate a test may tighten a graceful skip on, since only here
/// does a skip have no innocent reading. Mere child-ness may not: see
/// `g_allow.rs`'s
/// `replay_child_hard_fails_only_when_the_parent_verified_an_envelope`, the
/// live guard holding that boundary.
#[allow(dead_code)]
pub fn replay_child_expects_envelope() -> bool {
    std::env::var(REPLAY_GUARD).as_deref() == Ok(REPLAY_ENVELOPE_MARK)
}

/// A pre-sanitized `git -C <dir>` command for fixture-repo setup.
///
/// Thin by design: the sanitized variable list lives once, in
/// [`reify_audit::git_env::REPO_REDIRECT_VARS`]. A fixture helper that shells a
/// bare `Command::new("git")` is exactly as vulnerable as production code was —
/// an ambient `GIT_INDEX_FILE` overrides `-C <tempdir>`, so
/// `git -C <tempdir> add .` writes the PARENT repository's index (observed as
/// `git ["add", "."] exited Some(128)`, colliding with the parent's
/// `index.lock`).
#[allow(dead_code)]
pub fn git_cmd(dir: &Path) -> Command {
    reify_audit::git_env::command(dir)
}

/// A throwaway repository standing in for the parent repo a hook would point
/// at, carrying a stale `index.lock`.
///
/// The lock is what makes an unsanitized index WRITE fail loudly as well as
/// wrongly, so a regression cannot hide as a silent no-op.
#[allow(dead_code)]
pub struct DecoyRepo {
    /// Held only to keep the temporary directory alive for the decoy's
    /// lifetime; every path below points inside it.
    _dir: TempDir,
    git_dir: PathBuf,
    work_tree: PathBuf,
}

#[allow(dead_code)]
impl DecoyRepo {
    /// The decoy's `.git` directory — what an ambient `GIT_DIR` would name.
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// The decoy's working tree — what an ambient `GIT_WORK_TREE` would name.
    pub fn work_tree(&self) -> &Path {
        &self.work_tree
    }
}

/// Build a [`DecoyRepo`]: `git init` into a fresh tempdir, then plant a stale
/// `index.lock`.
///
/// The init goes through [`git_cmd`] like every other repo-targeting call.
/// That is load-bearing: a caller of this function may itself be re-run inside
/// a poisoned replay child, where a bare `Command::new("git")` would re-init
/// the *harness's* decoy instead of creating this one.
#[allow(dead_code)]
pub fn decoy_repo() -> DecoyRepo {
    let dir = tempfile::tempdir().expect("create decoy repo tempdir");

    let status = git_cmd(dir.path())
        .args(["init", "--initial-branch=main"])
        .status()
        .expect("git init decoy failed to spawn");
    assert!(
        status.success(),
        "decoy git init exited {:?}",
        status.code()
    );

    let git_dir = dir.path().join(".git");
    std::fs::write(git_dir.join("index.lock"), b"").expect("plant stale index.lock in decoy repo");

    let work_tree = dir.path().to_path_buf();
    DecoyRepo {
        _dir: dir,
        git_dir,
        work_tree,
    }
}

/// The exact `(name, value)` set git exports into a hook's entire process tree,
/// pointed at `decoy`.
///
/// The single home for the poisoned list on the test side. Every var here is
/// asserted to be one [`reify_audit::git_env::sanitize`] removes, so adding a
/// fourth var without teaching the sanitizer about it fails loudly here instead
/// of silently reducing coverage.
fn hook_git_env(decoy: &DecoyRepo) -> Vec<(&'static str, PathBuf)> {
    let vars = vec![
        ("GIT_DIR", decoy.git_dir().to_path_buf()),
        ("GIT_WORK_TREE", decoy.work_tree().to_path_buf()),
        ("GIT_INDEX_FILE", decoy.git_dir().join("index")),
    ];

    for (name, _) in &vars {
        assert!(
            reify_audit::git_env::REPO_REDIRECT_VARS.contains(name),
            "poisoning with `{}` proves nothing unless the sanitizer removes it — \
             add it to reify_audit::git_env::REPO_REDIRECT_VARS; current set: {:?}",
            name,
            reify_audit::git_env::REPO_REDIRECT_VARS,
        );
    }

    vars
}

/// Apply a hook's exported git environment, pointed at `decoy`, to `cmd`.
///
/// Poisons the CHILD only. Callers must never touch their own process
/// environment — see the module doc on `std::env::set_var`.
#[allow(dead_code)]
pub fn poison_with_hook_git_env<'a>(cmd: &'a mut Command, decoy: &DecoyRepo) -> &'a mut Command {
    for (name, value) in hook_git_env(decoy) {
        cmd.env(name, value);
    }
    cmd
}

/// [`replay_self_under_hook_git_env_with_mark`] stamping [`ReplayMark::Plain`]
/// — the variant for a caller that has verified nothing about this environment.
#[allow(dead_code)]
pub fn replay_self_under_hook_git_env(filters: &[&str], expected_min: usize) {
    replay_self_under_hook_git_env_with_mark(filters, expected_min, ReplayMark::Plain);
}

/// Re-run this test binary's `filters`-matching tests under a poisoned
/// *ambient* git environment, stamping `mark`, and assert they all still pass.
///
/// `filters` are libtest positional filters, OR-combined; a single `""` selects
/// every test in the binary. `expected_min` is the caller's declared floor on
/// how many tests the selection must contain — the guard against a vacuous
/// pass. Set it to the count you actually intend to cover; the selection may
/// grow past it freely, but it may not silently shrink below it.
///
/// PRECONDITION for [`ReplayMark::Envelope`]: pass it only after this process
/// has itself verified, in this same environment, that the audit under replay
/// produces an envelope. That is the whole content of that mark; stamping it
/// unconditionally renames the weaker mark rather than tightening anything.
///
/// Call this from a test whose own name does NOT match `filters`, so the replay
/// cannot select itself. The [`REPLAY_GUARD`] guard is the second line of
/// defence: inside the replayed child this function returns immediately, so
/// even a self-matching filter terminates.
///
/// `current_exe()` is the libtest binary itself, which accepts filter
/// positionals plus `--list`/`--test-threads`/`--nocapture`. Under nextest this
/// is the per-test binary, and nextest's own process-per-test invocation is
/// unaffected because the child is spawned by us, not by nextest.
#[allow(dead_code)]
pub fn replay_self_under_hook_git_env_with_mark(
    filters: &[&str],
    expected_min: usize,
    mark: ReplayMark,
) {
    // Re-entrancy guard: we ARE the replayed child. Do not recurse. The
    // question is child-ness and nothing more, so it goes through the one
    // predicate that answers it — never a second hand-rolled read of
    // `REPLAY_GUARD`, which a change to what counts as "set" would reach only
    // half of.
    if in_replay_child() {
        return;
    }

    let exe = std::env::current_exe().expect("current_exe");

    // Non-vacuity, step 1: what does this selection actually cover? Listing
    // runs in a CLEAN environment on purpose — the selection is what we want to
    // compare the poisoned run against.
    let listed = list_matching_tests(&exe, filters);
    assert!(
        listed.len() >= expected_min.max(1),
        "the replay filters {:?} select {} test(s), below the declared floor of {} — \
         libtest exits 0 on a zero-match filter, so this harness would have been a \
         vacuous pass. A test was probably renamed, moved to another binary, or lost \
         its `mod` wrapper. Selected: {:?}",
        filters,
        listed.len(),
        expected_min.max(1),
        listed,
    );

    let decoy = decoy_repo();

    let mut cmd = replay_child_command(filters, mark);
    poison_with_hook_git_env(&mut cmd, &decoy);

    let out = cmd
        .output()
        .expect("re-exec self under poisoned ambient git env");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        out.status.success(),
        "a test in `{}` matching {:?} failed while re-run under an ambient hook git \
         env. The failure may or may not be env-related: compare this test against \
         the same test in the clean parent run — if it failed there too, diagnose it \
         there, not here. Child exited {:?}\n\
         --- child stdout ---\n{}\n--- child stderr ---\n{}",
        exe.display(),
        filters,
        out.status.code(),
        stdout,
        stderr,
    );

    // Non-vacuity, step 2: a green exit is only meaningful if the poisoned run
    // actually accounted for every test the clean listing selected.
    let (passed, ignored) = parse_passed_and_ignored(&stdout).unwrap_or_else(|| {
        panic!(
            "could not find libtest's `test result:` summary in the replayed child's \
             stdout; cannot confirm the run was non-vacuous\n\
             --- child stdout ---\n{}\n--- child stderr ---\n{}",
            stdout, stderr,
        )
    });

    assert_eq!(
        passed + ignored,
        listed.len(),
        "the poisoned replay reported {} passed + {} ignored, but the same filters \
         {:?} list {} test(s) — the child ran a different set than the parent \
         selected, so a green exit proves nothing. Selected: {:?}\n\
         --- child stdout ---\n{}",
        passed,
        ignored,
        filters,
        listed.len(),
        listed,
        stdout,
    );
    assert!(
        passed >= expected_min.max(1),
        "the poisoned replay reported only {} passing test(s), below the declared \
         floor of {}\n--- child stdout ---\n{}",
        passed,
        expected_min.max(1),
        stdout,
    );
}

/// The `Command` shape EVERY replay child is spawned with: this test binary,
/// the caller's `filters`, `--test-threads=1 --nocapture` so the child's
/// libtest summary and its stderr notes both reach the parent intact, and
/// `mark` stamped into [`REPLAY_GUARD`].
///
/// Public so a binary needing the same child in a DIFFERENT environment (see
/// `g_allow.rs`'s deprived-`PATH` fixture) builds it from this one body, and so
/// cannot drift from the real replay whose behaviour it claims to pin. An
/// argument or a second guard variable added here reaches both; added at one
/// call site it would silently make the two children different processes while
/// the test that compares them kept passing.
#[allow(dead_code)]
pub fn replay_child_command(filters: &[&str], mark: ReplayMark) -> Command {
    let mut cmd = Command::new(std::env::current_exe().expect("current_exe"));
    cmd.args(filters)
        .args(["--test-threads=1", "--nocapture"])
        .env(REPLAY_GUARD, mark.value());
    cmd
}

/// The test names `filters` select in `exe`, via libtest's `--list`.
///
/// `--list` prints one `<name>: test` line per selected test (benchmarks get
/// `: benchmark`), then a blank line and an `N tests, M benchmarks` summary.
fn list_matching_tests(exe: &Path, filters: &[&str]) -> Vec<String> {
    let out = Command::new(exe)
        .args(filters)
        .arg("--list")
        .output()
        .expect("list this test binary's matching tests");
    assert!(
        out.status.success(),
        "`--list` on {} with filters {:?} exited {:?}\nstderr: {}",
        exe.display(),
        filters,
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );

    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .map(str::to_string)
        .collect()
}

/// Extract one count from libtest's summary line — e.g. `5` for `"passed"`
/// given `test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured;
/// 30 filtered out; finished in 0.92s`.
///
/// Takes the LAST such line, since `--nocapture` interleaves test output that
/// could in principle contain the same prefix. Parses the count as a NUMBER
/// rather than substring-matching `"1 passed"`, which would also match
/// `"21 passed"`.
///
/// Public because a caller that spawns its own child must read the same summary
/// this module reads, and one parser with two readers cannot drift the way two
/// parsers would.
#[allow(dead_code)]
pub fn libtest_summary_count(stdout: &str, field: &str) -> Option<usize> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("test result:"))?;

    let suffix = format!(" {field}");
    line.split(';')
        .map(str::trim)
        .find_map(|seg| seg.strip_suffix(suffix.as_str()))
        .and_then(|prefix| prefix.split_whitespace().next_back())
        .and_then(|n| n.parse().ok())
}

/// `(passed, ignored)` from libtest's summary line — the pair
/// [`replay_self_under_hook_git_env_with_mark`]'s two non-vacuity checks need.
fn parse_passed_and_ignored(stdout: &str) -> Option<(usize, usize)> {
    Some((
        libtest_summary_count(stdout, "passed")?,
        libtest_summary_count(stdout, "ignored")?,
    ))
}
