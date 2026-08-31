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
//! - [`in_replay_child`] — the weak predicate: "am I inside ANY replay
//!   child?". For a precondition, never for a tightening.
//! - [`replay_child_expects_envelope`] — the strong one, and the only one a
//!   test may use to turn an otherwise-graceful skip into a hard failure.
//! - [`spawn_replay_child_lacking_audit_prereqs`] — the inverse fixture: one
//!   replay child in an environment that genuinely cannot run the audit, so a
//!   test can pin which mark may tighten a skip and which may not.
//! - [`audit_script_stdout_poisoned_and_sanitized`] — spawn the orphan-audit
//!   script exactly twice (poisoned, then stripped), so the hazard's potency
//!   stays demonstrable independently of any production call site.
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
//! libtest exits 0 when a filter matches ZERO tests (measured:
//! `./cli-<hash> 'cli::nonexistent_filter_xyz'` prints
//! `test result: ok. 0 passed; … 35 filtered out` and exits 0). Asserting only
//! on the child's exit status would therefore turn this harness into a silent
//! green the instant a filter stops matching — a rename, a dropped `mod`
//! wrapper, or a test moving to another binary — which is precisely the
//! failure class this whole change set exists to close. So the replay lists
//! the selection first, requires it to be non-empty and at least the caller's
//! declared floor, and then requires the poisoned run to actually account for
//! every listed test.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
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
/// Its one reader is [`audit_script_stdout_poisoned_and_sanitized`]'s
/// precondition, which needs that weak question and no other: that helper's
/// `run_orphan_audit` gate would hit a repo-root mismatch panic inside ANY
/// poisoned child, whatever its parent verified.
///
/// NOT the predicate for tightening a graceful skip into a hard failure — use
/// [`replay_child_expects_envelope`]. Why, and the measured regression:
/// `tests/g_allow.rs`'s
/// `replay_child_hard_fails_only_when_the_parent_verified_an_envelope`.
#[allow(dead_code)]
pub fn in_replay_child() -> bool {
    std::env::var_os(REPLAY_GUARD).is_some()
}

/// True when this process is a replay child whose parent verified an audit
/// envelope before spawning it — [`in_replay_child`] plus the fact that makes
/// a skip inexplicable. The ONLY predicate a test may use to tighten an
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
/// Why the weaker [`in_replay_child`] may not be used here is stated once, in
/// `tests/g_allow.rs`'s
/// `replay_child_hard_fails_only_when_the_parent_verified_an_envelope`.
#[allow(dead_code)]
pub fn replay_child_expects_envelope() -> bool {
    std::env::var(REPLAY_GUARD).as_deref() == Ok(REPLAY_ENVELOPE_MARK)
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

/// stdout, exit status, and stderr from one
/// [`audit_script_stdout_poisoned_and_sanitized`] invocation.
///
/// All three are load-bearing, not just diagnostics: the audit script emits
/// empty stdout BOTH when the hook environment redirects its scan into an
/// empty tree (exit 0, stderr `no source files matched`) and when it aborts
/// before scanning at all (non-zero, git's own `fatal:` line). A caller that
/// reads only `stdout` cannot tell a demonstrated hazard from a broken
/// fixture.
#[allow(dead_code)]
pub struct AuditRun {
    pub stdout: String,
    pub status: ExitStatus,
    pub stderr: String,
}

/// Run `scripts/audit-orphan-producers.sh --scope <scope> --quiet --format
/// json` TWICE against one shared [`decoy_repo`]: once with the hook poison
/// ambient in the child, once with [`reify_audit::git_env::sanitize`] applied
/// — the same baseline `reify_test_support::sanitize` strips in production,
/// not just the three vars this helper poisons. Returns `(poisoned,
/// sanitized)` as [`AuditRun`]s.
///
/// Both commands are built from one closure against one decoy, so the poison
/// is the only difference between them apart from that sanitize call —
/// structural rather than a comment two call sites could drift apart on.
/// Calling the canonical [`reify_audit::git_env::sanitize`] directly, instead
/// of hand-rolling a second removal loop over `REPO_REDIRECT_VARS`, is what
/// keeps "sanitized" meaning what production means by it with no second copy
/// of the strip list to drift out of sync — [`hook_git_env`]'s own assertion
/// that every var it poisons is one `sanitize` removes is what makes "the
/// poisoned set is a subset of the sanitized set" a fact about the code
/// rather than a claim in this comment.
///
/// # Graceful-skip protocol — delegated, not re-implemented
///
/// Returns `None`, with an explanatory `stderr` note, exactly when
/// `reify_test_support::run_orphan_audit` declines to hand back an envelope
/// for `scope`. That one call IS the protocol — the `python3`/`git` presence
/// probes, the script-on-disk check, the `repo_root`-is-a-git-work-tree probe
/// and the `EXCLUDE_CRATES` membership test. Do not re-implement any of it
/// here: its most fragile element is a git diagnostic string that probe keys
/// on, so a second copy drifts the moment either git's wording or production's
/// probe changes.
///
/// Every cause of that `None` empties BOTH halves below — without `python3`
/// the script exits 3 with no stdout either way; an `EXCLUDE_CRATES` scope
/// legitimately emits nothing, reachable by any future caller since this
/// helper is generic over `scope`. So a caller comparing the two halves would
/// fail its "sanitized is non-empty" assertion while passing its "poisoned is
/// empty" one: a spurious RED that says nothing about the hazard. Skipping is
/// the only honest answer.
///
/// Delegating also inherits the protocol's LOUD half. A `git rev-parse
/// --show-toplevel` that fails for a reason OTHER than "no repository here" —
/// a corrupt `.git`, dubious ownership under this project's shared
/// warm-lane/worktree topology — is a condition where a repository IS expected
/// to exist. Production panics on it, naming the probe's status and stderr;
/// the re-implementation here swallowed both and fell through, so the caller
/// blamed a broken `--scope` instead: the wrong diagnosis, with the right one
/// already measured and discarded.
///
/// Must NOT be called from inside a poisoned replay child: the gate call
/// would hit `run_orphan_audit`'s repo-root mismatch panic rather than
/// skipping. Asserted below rather than left to this comment plus the replay
/// filter's substring choice, so widening that filter — or adding a test here
/// whose name happens to match it — fails on the precondition instead of
/// three frames down inside `reify-test-support`.
///
/// Spawn failures are hard failures, matching `run_orphan_audit`. This helper
/// asserts nothing about either run itself; it reports stdout, status and
/// stderr on [`AuditRun`] and leaves every judgement to the caller, which
/// needs all three to tell "redirected into the empty decoy and ran to
/// completion" (exit 0) from "aborted before scanning" (non-zero).
#[allow(dead_code)]
pub fn audit_script_stdout_poisoned_and_sanitized(scope: &str) -> Option<(AuditRun, AuditRun)> {
    assert!(
        !in_replay_child(),
        "audit_script_stdout_poisoned_and_sanitized must not run inside the poisoned \
         replay child — its `run_orphan_audit` gate would hit the repo-root mismatch \
         panic instead of skipping. Narrow the replay filter so it does not select \
         this helper's caller."
    );

    // The ENTIRE graceful-skip protocol, in one delegated call — see this
    // function's doc for why it is delegated rather than re-implemented, and
    // for why every cause of a `None` makes the comparison below meaningless.
    if reify_test_support::run_orphan_audit(scope).is_none() {
        eprintln!(
            "reify_test_support::run_orphan_audit({scope:?}) produced no envelope \
             (an environment skip, or the scope is in EXCLUDE_CRATES); skipping the \
             hook-git-env audit probe, which would otherwise compare two empty runs"
        );
        return None;
    }

    // CARGO_MANIFEST_DIR is evaluated in THIS crate, which always sits at
    // <repo>/crates/reify-audit; two `.parent()` walks reach the repo root.
    //
    // A SECOND COPY of the `resolve_script_and_root` walk inside
    // `reify_test_support`, and of the argv `build_audit_command` builds a few
    // lines below — same shape, same depth. It is here only because both of
    // those are module-private, and the gate above hands back an envelope
    // rather than the paths it resolved, while the two spawns below need the
    // script path itself. The right fix is a public seam on
    // `reify_test_support::orphan_audit` so this copy can be deleted rather
    // than pinned; that file is outside the lock set of the task that owns
    // this one, so it is filed as follow-up work. Until then the two premise
    // checks below bound the damage.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let script = Path::new(manifest_dir)
        .parent()
        .expect("crates/reify-audit has a parent (crates/)")
        .parent()
        .expect("crates/ has a parent (repo root)")
        .join("scripts/audit-orphan-producers.sh");

    let repo_root = script
        .parent()
        .expect("scripts/ dir exists")
        .parent()
        .expect("repo root exists");

    // Premise checks on the walk directly above — NOT a second copy of the
    // skip protocol. The gate already ran the script to completion, so it
    // provably exists at the path `reify-test-support` resolved from its own
    // manifest dir; anything wrong here is a bug in this helper, never an
    // environmental condition, so both fail loudly rather than skipping.
    //
    // Check 1: the script is where this walk says it is.
    assert!(
        script.exists(),
        "reify_test_support::run_orphan_audit({scope:?}) just ran the audit script \
         successfully, but this crate's own CARGO_MANIFEST_DIR walk resolves it to \
         {script:?}, where nothing exists — the two `.parent()` walks disagree, so \
         this helper would spawn a different script (or none) than the one the skip \
         protocol vetted"
    );

    // Check 2: this root is one from which the OTHER walk reproduces this
    // same root. `reify_test_support`'s `resolve_script_and_root` walks two
    // `.parent()`s off ITS manifest dir, so if `crates/reify-test-support`
    // sits here, that walk lands back on `repo_root` by construction.
    //
    // Check 1 alone cannot see this: it only rejects a walk that resolves to
    // NOTHING. Two walks resolving to existing but DIFFERENT roots — a nested
    // checkout, a vendored copy, either crate moved out of `crates/` — pass it
    // silently while spawning a script the gate never vetted. That is the case
    // this check adds.
    //
    // Bounded, deliberately: it does not distinguish this repo from a byte
    // identical vendored copy laid out the same way. Closing that needs the
    // path itself rather than a reconstruction of it, which means a public
    // seam on `reify_test_support::orphan_audit` (its `resolve_script_and_root`
    // and `build_audit_command` are module-private) — filed as follow-up work,
    // out of scope for the task that owns this file.
    let sibling_manifest = repo_root.join("crates/reify-test-support");
    assert!(
        sibling_manifest.join("Cargo.toml").exists(),
        "this crate's CARGO_MANIFEST_DIR walk resolves the repo root to \
         {repo_root:?}, but {sibling_manifest:?} holds no Cargo.toml — so \
         `reify_test_support`'s own two-`.parent()` walk, which the skip protocol \
         above just ran through, cannot have landed on this same root. The two \
         walks resolve DIFFERENT roots and this helper is about to spawn a script \
         the gate never vetted"
    );

    let decoy = decoy_repo();

    // One closure, so the two spawns are provably identical apart from the
    // environment delta below.
    let build = || {
        let mut cmd = Command::new(&script);
        cmd.args(["--scope", scope, "--quiet", "--format", "json"])
            .current_dir(repo_root);
        cmd
    };

    let mut poisoned_cmd = build();
    poison_with_hook_git_env(&mut poisoned_cmd, &decoy);

    let mut sanitized_cmd = build();
    poison_with_hook_git_env(&mut sanitized_cmd, &decoy);
    // The canonical sanitizer, not a hand-rolled removal loop — see this
    // function's doc for why.
    reify_audit::git_env::sanitize(&mut sanitized_cmd);

    let run = |mut cmd: Command, label: &str| -> AuditRun {
        let out = cmd.output().unwrap_or_else(|e| {
            panic!("failed to invoke audit-orphan-producers.sh ({label}): {e}")
        });
        AuditRun {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            status: out.status,
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    };

    let poisoned = run(poisoned_cmd, "poisoned");
    let sanitized = run(sanitized_cmd, "sanitized");

    Some((poisoned, sanitized))
}

/// Re-run this test binary's `filters`-matching tests under a poisoned
/// *ambient* git environment, and assert they all still pass.
///
/// `filters` are libtest positional filters, OR-combined (measured: passing
/// two names lists exactly those two). A single `""` selects every test in the
/// binary.
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
    replay_with_mark(filters, expected_min, ReplayMark::Plain);
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
#[allow(dead_code)]
pub fn replay_self_under_hook_git_env_expecting_envelope(filters: &[&str], expected_min: usize) {
    replay_with_mark(filters, expected_min, ReplayMark::Envelope);
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
fn replay_with_mark(filters: &[&str], expected_min: usize, mark: ReplayMark) {
    // Re-entrancy guard: we ARE the replayed child. Do not recurse.
    if std::env::var_os(REPLAY_GUARD).is_some() {
        return;
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
/// skip path (measured: the child's stderr reads `python3 not on PATH;
/// skipping orphan audit for scope "crates/reify-audit/src"`). That is a
/// SUPPORTED environment, not a broken one.
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
