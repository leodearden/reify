//! Shared by the reify-audit integration test binaries: the git-environment
//! plumbing, and the ambient-replay harness built on it.
//!
//! - [`git_cmd`] — the constructor every fixture-repo helper should use.
//! - [`decoy_repo`] / [`poison_with_hook_git_env`] — a stand-in for the parent
//!   repository a hook would point at, and that hook's exported environment
//!   applied to a command.
//! - [`replay_self_under_hook_git_env`] /
//!   [`replay_self_under_hook_git_env_expecting_envelope`] /
//!   [`replay_child_command`] — re-run this binary's own tests with that
//!   environment genuinely ambient, under the weaker or the stronger mark.
//! - [`ReplayMark`] / [`replay_child_expects_envelope`] /
//!   [`assert_not_in_replay_child`] / [`announce_replay_mark`] — what a replay
//!   child may conclude about the parent that spawned it, and what it reports
//!   back about the mark it carries.
//! - [`libtest_summary_count`] / [`SummaryField`] — the one parser every replay
//!   child's libtest summary is read through, and the named counts it reads,
//!   here because the replay's own non-vacuity checks are its first reader.
//!
//! Generic only: this module is compiled into every `tests/*.rs` in this crate,
//! so a helper hard-coding one script's path, argv or skip protocol belongs in
//! the binary that consumes it.
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

/// The line [`announce_replay_mark`] emits, and
/// [`replay_self_under_hook_git_env_expecting_envelope`] requires, when a
/// replay child carries [`ReplayMark::Envelope`].
///
/// Deliberately a sentence no other output in these binaries produces: the
/// parent greps the child's WHOLE stderr for it, `--nocapture` and all.
const ENVELOPE_BREADCRUMB: &str = "reify-audit replay child: carrying the envelope mark";

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
/// instead. The only other reader is [`replay_with_mark`]'s re-entrancy guard,
/// which asks the same question for the same reason.
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

/// Emit [`ENVELOPE_BREADCRUMB`] iff this process is a replay child carrying
/// [`ReplayMark::Envelope`]; a no-op everywhere else, so it is safe as a test's
/// unconditional first statement.
///
/// The child's half of the round trip that lets
/// [`replay_self_under_hook_git_env_expecting_envelope`] observe the mark its
/// own child actually carried, rather than trusting the value it passed. Keyed
/// on the PREDICATE, not on the mark's name or `Debug` spelling, so the child
/// reports the exact quantity the tightening branches on instead of a raw value
/// the parent would have to re-derive a verdict from. A drift between stamping
/// and reading the mark is not its job: [`ReplayMark::value`] and
/// [`replay_child_expects_envelope`] read the same [`REPLAY_ENVELOPE_MARK`], so
/// that drift is a compile error.
#[allow(dead_code)]
pub fn announce_replay_mark() {
    if replay_child_expects_envelope() {
        eprintln!("{ENVELOPE_BREADCRUMB} (${REPLAY_GUARD})");
    }
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

/// [`replay_with_mark`] stamping [`ReplayMark::Plain`] — the variant for a
/// caller that has verified nothing about this environment.
///
/// Discards the child's stderr: a `Plain` replay makes no claim about the mark
/// its child carried, so it has nothing to check that stderr for. A caller that
/// DOES want the envelope-gated tightening armed wants
/// [`replay_self_under_hook_git_env_expecting_envelope`] instead.
#[allow(dead_code)]
pub fn replay_self_under_hook_git_env(filters: &[&str], expected_min: usize) {
    let _ = replay_with_mark(filters, expected_min, ReplayMark::Plain);
}

/// [`replay_with_mark`] stamping [`ReplayMark::Envelope`] — the ONLY place that
/// mark is stamped for a real replay, and so the only way to arm the
/// envelope-gated tightening.
///
/// PRECONDITION: call this only after this process has itself verified, in this
/// same environment, that the audit under replay produces an envelope. That is
/// the whole content of the mark; arming it unconditionally renames the weaker
/// mark rather than tightening anything.
///
/// Beyond delegating, this OBSERVES the mark its own child carried, by
/// requiring the child's stderr to carry [`ENVELOPE_BREADCRUMB`] —
/// [`announce_replay_mark`]'s half of the round trip. That check is
/// UNCONDITIONAL and reads no `mark` parameter, which is exactly why it lives
/// here rather than in [`replay_with_mark`]: under the mutation it exists to
/// catch, that parameter reads [`ReplayMark::Plain`], so a check conditioned on
/// it would simply not run. This being the only public way to arm the
/// tightening, a caller cannot stamp the mark and skip the observation — a
/// structural property, not a matter of this assertion's wording.
#[allow(dead_code)]
pub fn replay_self_under_hook_git_env_expecting_envelope(filters: &[&str], expected_min: usize) {
    let Some(stderr) = replay_with_mark(filters, expected_min, ReplayMark::Envelope) else {
        return;
    };

    assert!(
        stderr.contains(ENVELOPE_BREADCRUMB),
        "the replay child never reported carrying the envelope mark: its stderr \
         lacks {ENVELOPE_BREADCRUMB:?}. So this replay did NOT arm the \
         envelope-gated tightening, and its green says only that the child \
         passed — which a child that skipped the audit entirely also does. Two \
         causes. (1) This function stamps a mark other than \
         `ReplayMark::Envelope`. (2) The test the filters {:?} select dropped \
         its `announce_replay_mark()` call, or no longer reaches it before it \
         can skip or panic. A caller that did not mean to arm the tightening \
         wants `replay_self_under_hook_git_env` instead.\n\
         --- child stderr (truncated) ---\n{:.600}",
        filters,
        stderr,
    );
}

/// Re-run this test binary's `filters`-matching tests under a poisoned
/// *ambient* git environment, stamping `mark`, and assert they all still pass.
///
/// Returns the child's stderr, or `None` when this process was itself a replay
/// child and so spawned nothing. PRIVATE so that `mark` is not a value an
/// arbitrary call site chooses: the two entry points above stamp one mark each,
/// and each is named for the claim it makes. They share this body, so the two
/// spawn paths cannot drift.
///
/// `filters` are libtest positional filters, OR-combined; a single `""` selects
/// every test in the binary. `expected_min` is the caller's declared floor on
/// how many tests the selection must contain — the guard against a vacuous
/// pass. Set it to the count you actually intend to cover; the selection may
/// grow past it freely, but it may not silently shrink below it.
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
fn replay_with_mark(filters: &[&str], expected_min: usize, mark: ReplayMark) -> Option<String> {
    // Re-entrancy guard: we ARE the replayed child. Do not recurse. The
    // question is child-ness and nothing more, so it goes through the one
    // predicate that answers it — never a second hand-rolled read of
    // `REPLAY_GUARD`, which a change to what counts as "set" would reach only
    // half of.
    if in_replay_child() {
        return None;
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

    Some(stderr.into_owned())
}

/// The `Command` shape EVERY replay child is spawned with: this test binary,
/// the caller's `filters`, `--test-threads=1 --nocapture` so the child's
/// libtest summary and its stderr notes both reach the parent intact, and
/// `mark` stamped into [`REPLAY_GUARD`].
///
/// Public so a binary that needs this same child on its own terms builds it
/// from this one body, and so cannot drift from the real replay whose behaviour
/// it claims to pin: `g_allow.rs`'s deprived-`PATH` fixture wants it in a
/// DIFFERENT environment, and its refusal fixture wants it with nothing but the
/// guard variable set. An argument or a second guard variable added here
/// reaches all of them; added at one call site it would silently make the
/// children different processes while the tests comparing them kept passing.
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

/// Which count [`libtest_summary_count`] reads out of libtest's summary line.
///
/// Same bargain as [`ReplayMark`], for the same reason: callers name the count
/// and this enum resolves it to libtest's spelling, so the word appears exactly
/// once and a misspelling is a compile error. A free-form `&str` field would
/// make one a runtime `None` instead — and `None` is the same answer this
/// parser gives for a child that printed no summary at all, so the caller's
/// typo would be reported as the CHILD's output being malformed.
///
/// Carries only the counts something reads: `measured` and `filtered out` are
/// absent because nothing here has ever needed them, and a variant no caller
/// constructs is a claim of coverage this module does not have.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryField {
    /// Tests that ran and passed.
    Passed,
    /// Tests that ran and failed.
    Failed,
    /// Tests libtest skipped as `#[ignore]`d.
    Ignored,
}

impl SummaryField {
    /// The word libtest prints immediately after this field's count.
    fn word(self) -> &'static str {
        match self {
            SummaryField::Passed => "passed",
            SummaryField::Failed => "failed",
            SummaryField::Ignored => "ignored",
        }
    }
}

/// Extract one count from libtest's summary line — e.g. `5` for
/// [`SummaryField::Passed`] given `test result: ok. 5 passed; 0 failed;
/// 0 ignored; 0 measured; 30 filtered out; finished in 0.92s`.
///
/// Takes the LAST such line, since `--nocapture` interleaves test output that
/// could in principle contain the same prefix. Parses the count as a NUMBER
/// rather than substring-matching `"1 passed"`, which would also match
/// `"21 passed"`.
///
/// A `None` therefore has exactly one cause left — no `test result:` line in
/// `stdout` — which is what lets every caller attribute it to the child.
///
/// Public because a caller that spawns its own child must read the same summary
/// this module reads, and one parser with two readers cannot drift the way two
/// parsers would. Pinned in `tests/replay_harness.rs` rather than beside either
/// reader, so retiring one reader cannot take its only coverage with it.
#[allow(dead_code)]
pub fn libtest_summary_count(stdout: &str, field: SummaryField) -> Option<usize> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("test result:"))?;

    let suffix = format!(" {}", field.word());
    line.split(';')
        .map(str::trim)
        .find_map(|seg| seg.strip_suffix(suffix.as_str()))
        .and_then(|prefix| prefix.split_whitespace().next_back())
        .and_then(|n| n.parse().ok())
}

/// `(passed, ignored)` from libtest's summary line — the pair
/// [`replay_with_mark`]'s two non-vacuity checks need.
fn parse_passed_and_ignored(stdout: &str) -> Option<(usize, usize)> {
    Some((
        libtest_summary_count(stdout, SummaryField::Passed)?,
        libtest_summary_count(stdout, SummaryField::Ignored)?,
    ))
}
