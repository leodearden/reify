//! Git-environment helpers shared by the reify-audit integration test
//! binaries.
//!
//! Everything here is deliberately thin, and nothing here duplicates a
//! variable list: the *sanitized* set lives once in
//! [`reify_audit::git_env::REPO_REDIRECT_VARS`], and the *poisoned* set lives
//! once in [`hook_git_env`] (which asserts it is a subset of the sanitized
//! one, so the two cannot drift apart silently).
//!
//! - [`git_cmd`] — the constructor every fixture-repo helper should use.
//! - [`decoy_repo`] / [`poison_with_hook_git_env`] — build a stand-in for the
//!   parent repository a hook would point at, and apply the hook's exported
//!   environment to a command.
//! - [`replay_self_under_hook_git_env`] — the outer harness that proves the
//!   fix under a real *ambient* environment rather than a per-child one.
//! - [`replay_self_under_hook_git_env_expecting_envelope`] — the same harness
//!   for a caller that has already verified, in this environment, that the
//!   audit produces an envelope. That verified fact is what the child's
//!   stronger mark carries.
//! - [`replay_child_expects_envelope`] — the ONLY predicate a test may use to
//!   turn an otherwise-graceful skip into a hard failure. The weaker "am I
//!   inside ANY replay child?" question has a private helper, so no sibling
//!   binary can reach for it by mistake.
//! - [`spawn_replay_child_lacking_audit_prereqs`] — the inverse fixture: one
//!   replay child in an environment that genuinely cannot run the audit, so a
//!   test can pin which mark may tighten a skip and which may not.
//!
//! Generic git-environment plumbing only. A helper that hard-codes one
//! script's path, argv or skip protocol belongs in the binary that consumes it
//! — this module is compiled into every `tests/*.rs` in this crate, so a
//! domain-specific helper here is a dozen copies of a `.parent()` walk plus a
//! reachability hazard from binaries that never wanted it.
//!
//! Why the weak/strong split exists, and the regression that forced it, are
//! stated ONCE — in `tests/g_allow.rs`'s
//! `replay_child_hard_fails_only_when_the_parent_verified_an_envelope`, the
//! live guard holding it. Point there; do not re-derive it here.
//!
//! # Why a replay harness
//!
//! The reported condition is a hook environment: `hooks/pre-commit` ->
//! `hooks/project-checks` -> `scripts/verify.sh` -> the workspace test run,
//! with `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE` exported into the whole
//! process tree. Reproducing that *inside* a test would mean mutating the
//! test process's own environment, and `std::env::set_var` is process-global:
//! under nextest's process-per-test isolation it would appear to work, hiding
//! the hazard, while under `cargo test`'s thread-per-test model it would race
//! and intermittently poison sibling tests — trading a deterministic bug for
//! a flaky one.
//!
//! So instead of poisoning ourselves, we re-exec ourselves poisoned: spawn
//! `current_exe()` with the poison in the CHILD's environment, where it is
//! genuinely ambient for every test that child runs.
//!
//! # Why the replay counts tests
//!
//! libtest exits 0 when a filter matches ZERO tests. Asserting only
//! on the child's exit status would therefore turn this harness into a silent
//! green the instant a filter stops matching — a rename, a dropped `mod`
//! wrapper, or a test moving to another binary — which is precisely the
//! failure class this whole change set exists to close. So the replay lists
//! the selection first, requires it to be non-empty and at least the caller's
//! declared floor, and then requires the poisoned run to actually account for
//! every listed test.

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// Environment variable marking the replayed child process, so the replay
/// test does not recurse when the child re-runs it.
const REPLAY_GUARD: &str = "REIFY_AUDIT_HOOK_ENV_REPLAY";

/// [`REPLAY_GUARD`]'s value for a child spawned by a parent that has verified
/// NOTHING about this environment beyond the fact that it is replaying.
const REPLAY_PLAIN_MARK: &str = "1";

/// [`REPLAY_GUARD`]'s value for a child whose parent DID verify, in this same
/// process and this same environment, that the orphan audit produces an
/// envelope. The stronger claim, and the only one that entitles the child to
/// treat a skip as a failure.
///
/// Kept private alongside [`REPLAY_GUARD`], for the same single-source reason:
/// [`ReplayMark::value`] and [`replay_child_expects_envelope`] are its only
/// readers, so no call site can re-read or re-stamp the marker under its own
/// name — a caller names [`ReplayMark::Envelope`] instead.
const REPLAY_ENVELOPE_MARK: &str = "envelope";

/// Which claim a replay child's [`REPLAY_GUARD`] value carries.
///
/// The public spelling of the two private marks: a caller names the claim and
/// this enum resolves it to the value, so no call site re-spells a mark and a
/// drift is a compile error rather than something a runtime check must catch.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayMark {
    /// The parent verified NOTHING beyond the fact that it is replaying.
    Plain,
    /// The parent saw an audit envelope in this same environment moments
    /// before spawning — the only claim that entitles the child to treat a
    /// skip as a failure (see [`replay_child_expects_envelope`]).
    Envelope,
}

impl ReplayMark {
    /// The value stamped into [`REPLAY_GUARD`] for a child carrying this
    /// claim.
    fn value(self) -> &'static str {
        match self {
            ReplayMark::Plain => REPLAY_PLAIN_MARK,
            ReplayMark::Envelope => REPLAY_ENVELOPE_MARK,
        }
    }
}

/// True when this process is a replay child, spawned by EITHER variant. It
/// answers exactly one question — "am I inside any replay child?" — and
/// nothing more.
///
/// PRIVATE on purpose: this is not the predicate that may tighten a graceful
/// skip into a hard failure (that is [`replay_child_expects_envelope`]), and a
/// predicate the sibling test binaries cannot name is one they cannot misuse.
/// Both readers are in this module:
///
/// - [`assert_not_in_replay_child`], which exposes the weak question to
///   callers as a REFUSAL rather than as a `bool` — nothing they can branch a
///   skip on.
/// - [`replay_with_mark`]'s re-entrancy guard, which asks the same question
///   for the same reason: child-ness alone decides whether to recurse.
fn in_replay_child() -> bool {
    std::env::var_os(REPLAY_GUARD).is_some()
}

/// Refuse to proceed if this process is a replay child of ANY mark.
///
/// For a helper that is unsound inside a replay child — one that would panic
/// three frames down in another crate rather than take the skip its caller
/// expects. `helper` names the caller and `consequence` says what would go
/// wrong; both land in the panic message, so the diagnosis stays with the
/// helper that knows it.
///
/// Returns nothing, deliberately. A `bool` here would be exactly the weak
/// "am I in a replay child?" predicate [`in_replay_child`] is private to
/// withhold — usable to tighten a graceful skip, which only
/// [`replay_child_expects_envelope`] may do. A refusal cannot be repurposed
/// that way.
#[allow(dead_code)]
pub fn assert_not_in_replay_child(helper: &str, consequence: &str) {
    assert!(
        !in_replay_child(),
        "{helper} must not run inside a replay child — {consequence}. Narrow the \
         replay filter so it does not select this helper's caller."
    );
}

/// True when this process is a replay child whose parent verified an audit
/// envelope before spawning it — replay child-ness PLUS the fact that makes a
/// skip inexplicable. The ONLY predicate a test may use to tighten an
/// otherwise-graceful skip into a hard failure.
///
/// The guarantee comes from the spawn side:
/// [`replay_self_under_hook_git_env_expecting_envelope`] is the only thing
/// that stamps [`ReplayMark::Envelope`], and its contract is that the caller
/// has already seen an envelope in this environment. So inside such a child a
/// skip means the environment changed underfoot between two runs seconds
/// apart — which, under an ambient hook git environment, is exactly the hazard
/// the replay exists to catch.
///
/// Why mere child-ness may not be used here is stated once, in
/// `tests/g_allow.rs`'s
/// `replay_child_hard_fails_only_when_the_parent_verified_an_envelope`.
#[allow(dead_code)]
pub fn replay_child_expects_envelope() -> bool {
    std::env::var(REPLAY_GUARD).as_deref() == Ok(REPLAY_ENVELOPE_MARK)
}

/// The line a replay child emits when [`replay_child_expects_envelope`] holds
/// in it, and which
/// [`replay_self_under_hook_git_env_expecting_envelope`] reads back out of that
/// child's stderr.
const ENVELOPE_BREADCRUMB: &str = "replay child: replay_child_expects_envelope() == true";

/// Emit [`ENVELOPE_BREADCRUMB`] iff this process is a replay child carrying the
/// envelope mark. A no-op everywhere else, so it is safe to call
/// unconditionally from a test's first line.
///
/// Call it from every test a
/// [`replay_self_under_hook_git_env_expecting_envelope`] caller selects. That
/// spawner asserts the breadcrumb came back, and that round trip is the only
/// thing pinning the envelope path end-to-end: stamping [`ReplayMark::Plain`]
/// there instead is a ONE-TOKEN change that otherwise leaves every test in this
/// crate green while silently disabling the tightening
/// `replay_child_hard_fails_only_when_the_parent_verified_an_envelope` bounds.
/// Deleting this call from the target test reddens that spawner for the same
/// reason — fail-closed in both directions.
///
/// Keyed on the PREDICATE rather than on the mark's name, deliberately: the
/// breadcrumb then also dies if [`replay_child_expects_envelope`] stops
/// recognising the value the spawner stamps, which is the other half of the
/// wiring and is invisible to a check that merely re-prints `mark`.
#[allow(dead_code)]
pub fn announce_replay_mark() {
    if replay_child_expects_envelope() {
        eprintln!("{ENVELOPE_BREADCRUMB}");
    }
}

/// A pre-sanitized `git -C <dir>` command for fixture-repo setup.
///
/// Thin by design: the sanitized variable list lives once, in
/// [`reify_audit::git_env::REPO_REDIRECT_VARS`]. A fixture helper that shells
/// a bare `Command::new("git")` is exactly as vulnerable as production code
/// was — an ambient `GIT_INDEX_FILE` overrides `-C <tempdir>`, so
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
/// The init itself goes through [`git_cmd`] like every other repo-targeting
/// call. That is load-bearing, not decorative: a caller of this function may
/// itself be re-run inside a poisoned replay child, where a bare
/// `Command::new("git")` would re-init the *harness's* decoy instead of
/// creating this one.
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

/// The exact `(name, value)` set git exports into a hook's entire process
/// tree, pointed at `decoy`.
///
/// The single home for the poisoned list on the test side. Every var here is
/// asserted to be one [`reify_audit::git_env::sanitize`] removes, so adding a
/// fourth var without teaching the sanitizer about it fails loudly here
/// instead of silently reducing coverage.
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

/// Re-run this test binary's `filters`-matching tests under a poisoned
/// *ambient* git environment, and assert they all still pass.
///
/// `filters` are libtest positional filters, OR-combined. A single `""`
/// selects every test in the binary.
///
/// `expected_min` is the caller's declared floor on how many tests the
/// selection must contain — the guard against a vacuous pass. Set it to the
/// count you actually intend to cover; the selection may grow past it freely,
/// but it may not silently shrink below it.
///
/// Call this from a test whose own name does NOT match `filters`, so the
/// replay cannot select itself. The `REIFY_AUDIT_HOOK_ENV_REPLAY` guard is
/// the second line of defence: inside the replayed child this function
/// returns immediately, so even a self-matching filter terminates.
///
/// `current_exe()` is the libtest binary itself, which accepts filter
/// positionals plus `--list`/`--test-threads`/`--nocapture`. Under nextest
/// this is the per-test binary, and nextest's own process-per-test invocation
/// is unaffected because the child is spawned by us, not by nextest.
#[allow(dead_code)]
pub fn replay_self_under_hook_git_env(filters: &[&str], expected_min: usize) {
    let _child_stderr = replay_with_mark(filters, expected_min, ReplayMark::Plain);
}

/// [`replay_self_under_hook_git_env`], but stamping the marker that entitles
/// the replayed test to treat a skip as a hard failure
/// ([`replay_child_expects_envelope`]).
///
/// Call this ONLY after this process has verified, in this same environment,
/// that the audit under replay actually produces an envelope. That verified
/// fact is the whole content of the stronger mark — stamping it
/// unconditionally would not tighten anything, it would just rename the
/// weaker mark and restore the false RED this variant exists to prevent.
///
/// Everything else — the re-entrancy guard, the `--list` non-vacuity floor,
/// the decoy, the poison, the status assertion and both post-run count checks
/// — is shared verbatim with the plain variant, so the two spawn paths cannot
/// drift apart.
///
/// The breadcrumb assertion below is what makes THIS function's own claim
/// checkable rather than merely asserted — see [`announce_replay_mark`], which
/// the selected test must call. It lives here and not in [`replay_with_mark`]
/// on purpose: a check keyed on that function's `mark` parameter would simply
/// not run under the one-token mutation it exists to catch.
#[allow(dead_code)]
pub fn replay_self_under_hook_git_env_expecting_envelope(filters: &[&str], expected_min: usize) {
    let Some(child_stderr) = replay_with_mark(filters, expected_min, ReplayMark::Envelope) else {
        // We are ourselves a replay child, so nothing was spawned and there is
        // no breadcrumb to read.
        return;
    };

    assert!(
        child_stderr.contains(ENVELOPE_BREADCRUMB),
        "the replay child spawned for filters {:?} never reported \
         {ENVELOPE_BREADCRUMB:?}, so nothing establishes that it carried the \
         envelope mark — and a child that does not carry it can never reach the \
         tightening this variant exists to arm. Two causes, both real: this \
         function stamps a mark other than `ReplayMark::Envelope` (or \
         `replay_child_expects_envelope` no longer recognises the value it \
         stamps), or the selected test dropped its \
         `common::git_env::announce_replay_mark()` call. Use \
         `replay_self_under_hook_git_env` if you did not mean to arm the \
         tightening.\n\
         --- child stderr (truncated) ---\n{:.800}",
        filters,
        child_stderr,
    );
}

/// The `Command` shape EVERY replay child is spawned with: this test binary,
/// the caller's `filters`, `--test-threads=1 --nocapture` so the child's
/// libtest summary and its stderr notes both reach the parent intact, and
/// `mark` stamped into [`REPLAY_GUARD`].
///
/// One body with two callers — [`replay_with_mark`], which adds the decoy
/// poison, and [`spawn_replay_child_lacking_audit_prereqs`], which adds a
/// deprived `PATH` — so the fixture cannot drift from the real replay whose
/// behaviour it claims to pin. An argument or a second guard variable added
/// here reaches both; added at one call site it would silently make the two
/// children different processes while the test that compares them kept
/// passing.
fn replay_child_command(filters: &[&str], mark: ReplayMark) -> Command {
    let mut cmd = Command::new(std::env::current_exe().expect("current_exe"));
    cmd.args(filters)
        .args(["--test-threads=1", "--nocapture"])
        .env(REPLAY_GUARD, mark.value());
    cmd
}

/// The shared body of both replay variants; `mark` is the value stamped into
/// [`REPLAY_GUARD`] for the child, and the ONLY difference between them.
///
/// Returns the child's stderr, or `None` when this process is itself a replay
/// child and so spawned nothing. Only
/// [`replay_self_under_hook_git_env_expecting_envelope`] reads it, to check its
/// own spawn against [`ENVELOPE_BREADCRUMB`].
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
    // runs in a CLEAN environment on purpose — the selection is what we want
    // to compare the poisoned run against.
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

/// Spawn ONE replay child in an environment that genuinely CANNOT run the
/// orphan audit, stamping `mark` as the replay guard's value.
///
/// The fixture behind
/// `replay_child_hard_fails_only_when_the_parent_verified_an_envelope`. Its
/// whole purpose is to hold everything fixed except `mark`, so the caller's
/// two children differ only in what the mark claims. The body is
/// [`replay_child_command`] — see its doc for why that is shared.
///
/// `PATH` is an EMPTY [`tempfile::tempdir`], which makes
/// `reify_test_support::run_orphan_audit`'s FIRST probe —
/// `Command::new("python3")` — fail with `NotFound` and take its documented
/// skip path. That is a SUPPORTED environment, not a broken one. The caller
/// asserts on the skip note the child actually emits rather than trusting a
/// copy of it quoted here.
///
/// Deliberately NOT poisoned with the hook git environment. With `PATH`
/// deprived the child skips long before it reaches the audit script, so a
/// decoy would add a tempdir and no signal — the discrimination this fixture
/// buys is the MARK's meaning, not the poison's.
///
/// Spawn failures are hard failures: `current_exe()` is this very binary, so
/// a failure to exec it is a broken harness rather than an environmental
/// condition the caller could sensibly skip on.
#[allow(dead_code)]
pub fn spawn_replay_child_lacking_audit_prereqs(
    filters: &[&str],
    mark: ReplayMark,
) -> std::process::Output {
    // Held until after `output()` returns, so the child sees a PATH that
    // exists and is empty rather than one pointing at a deleted directory.
    let empty_path = tempfile::tempdir().expect("create empty PATH dir for the deprived child");

    // The deprived `PATH` is the ONLY thing this fixture adds to the shape
    // every replay child is spawned with — see [`replay_child_command`].
    replay_child_command(filters, mark)
        .env("PATH", empty_path.path())
        .output()
        .expect("re-exec self with the orphan audit's prerequisites removed")
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
/// given `test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 30 filtered out`.
///
/// Takes the LAST such line, since `--nocapture` interleaves test output that
/// could in principle contain the same prefix. Parses the count as a NUMBER
/// rather than substring-matching `"1 passed"`, which would also match
/// `"21 passed"`.
///
/// Public because a caller that spawns its own child (see
/// [`spawn_replay_child_lacking_audit_prereqs`]) must read the same summary
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
/// [`replay_self_under_hook_git_env`]'s two non-vacuity checks need.
fn parse_passed_and_ignored(stdout: &str) -> Option<(usize, usize)> {
    Some((
        libtest_summary_count(stdout, "passed")?,
        libtest_summary_count(stdout, "ignored")?,
    ))
}
