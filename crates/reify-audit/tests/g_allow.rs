//! Pin: every `pub fn` in `crates/reify-audit/src/` is either called by a
//! non-test caller or carries a `// G-allow:` marker.
//!
//! User-observable signal (per task description and design decisions):
//!   `cargo test -p reify-audit --test g_allow`
//!
//! The test shells out to `scripts/audit-orphan-producers.sh` — the
//! source-of-truth for orphan detection — with `--scope crates/reify-audit/src`
//! so it only checks this crate. Running it workspace-wide would fail for
//! reasons outside this task (422 pre-existing orphans captured in the
//! baseline report).
//!
//! Anti-gaming rationale: this pin defends against gaming the orphan-producer script
//! via boilerplate `// G-allow:` markers — the script's regex (`//\s*G-allow:\s*(.+)`)
//! only requires non-blank reason text. Semantic accuracy (the reason names a real
//! deferred consumer or tracked task) is enforced by reviewer review; this test
//! surfaces surface-level approvals to reviewers. See esc-3667-113 triage.
//!
//! Graceful skip: if `python3` or `git` are absent from PATH, the test prints
//! a note to stderr and returns. Mirrors
//! `crates/reify-kernel-gmsh/tests/rpath_smoke.rs`.
//! The shared helper is in `reify_test_support::run_orphan_audit`.
//!
//! That skip has exactly one exception, `common::git_env`'s
//! `replay_child_expects_envelope` — see the earned-mark rule, below.
//!
//! # The hook-git-env trio
//!
//! The three hook-git-env tests below are one invariant in three parts, and
//! this is its ONLY home — each points here instead of restating it:
//!
//! - `orphan_audit_survives_ambient_hook_git_env` — production sanitizes.
//! - `hook_git_env_defeats_the_audit_script_and_stripping_it_cures_the_defeat`
//!   — sanitizing is what makes the difference, demonstrated synthetically so
//!   the hazard stays visible from a clean checkout.
//! - `replay_child_hard_fails_only_when_the_parent_verified_an_envelope` —
//!   neither tightening may fire where the audit was never shown able to run,
//!   so the first cannot buy its teeth by reddening supported environments.
//!
//! None can notice another going vacuous. Retire them together or not at all.
//!
//! # The earned-mark rule
//!
//! A separate rule with a separate home: a replay child's hard failure needs
//! an EARNED mark, not mere child-ness. That home is the third test's own doc.
//! Every other mention — here, and throughout `common::git_env` — is a bare
//! pointer to it and re-derives nothing, so editing that one doc really is
//! editing every statement of the rule.

use std::path::Path;
use std::process::{Command, ExitStatus};

use reify_test_support::run_orphan_audit;

mod common;

/// The audit scope every test in this file uses.
///
/// One home because several docs below rest on the premise that all the runs
/// are of the SAME scope — e.g. the synthetic witness's sanitized-half failure
/// message says `run_orphan_audit` "ran this same script against this same
/// scope moments ago". Nothing compares the literals at runtime, so spelling
/// them separately would let one drift and silently falsify that premise while
/// every test stayed green.
const SCOPE: &str = "crates/reify-audit/src";

/// The test the two replay paths select, by libtest positional filter.
///
/// One home for the same reason as [`SCOPE`], plus one specific to a filter:
/// the replay must not select the test that spawns it, and "no other test name
/// in this binary contains this substring" is a property of ONE string. Two
/// copies make it a property nothing states about either.
const TARGET_TEST: &str = "reify_audit_pub_fns_are_g_allow_marked";

#[test]
fn reify_audit_pub_fns_are_g_allow_marked() {
    // Unconditional and first, so the breadcrumb is out before anything below
    // can skip or panic. See `common::git_env::announce_replay_mark`.
    common::git_env::announce_replay_mark();

    let audit = run_orphan_audit(SCOPE);

    // Defence-in-depth against `run_orphan_audit`'s public contract, which
    // still permits `None` (see its doc for the causes), scoped by the
    // earned-mark rule to the one child where a skip has no innocent reading.
    // The graceful-skip path below is untouched everywhere else, including in
    // a plain-marked child.
    //
    // What this catches that the replay's child-exit-status assertion cannot:
    // a skip is a `return`, and libtest has no skipped state, so a skipping
    // child exits 0 reporting `1 passed` — indistinguishable from a real run,
    // and `replay_with_mark`'s status, `passed + ignored == listed` and floor
    // checks ALL hold on a replay that exercised nothing. Only the child can
    // tell the two apart, by refusing to skip. Not hypothetical: it is what
    // `run_orphan_audit` did on a wrong-tree redirect until task 5698 made
    // that case a panic. That panic — one probe in another crate, not this
    // function's contract — is the only thing making this branch unreachable.
    if audit.is_none() && common::git_env::replay_child_expects_envelope() {
        panic!(
            "run_orphan_audit returned None inside a replay child stamped with \
             the envelope mark. That mark is stamped only by a parent that ran \
             this same audit, for this same scope, in this same environment \
             moments earlier and GOT an envelope — and that parent returns \
             without spawning any child when it did not. So `python3`/`git`/the \
             script being absent is not a plausible reading here: what changed \
             between that run and this one is the ambient hook git environment \
             this child carries."
        );
    }

    let Some(result) = audit else {
        return;
    };

    let orphan_count = result["orphan_count"]
        .as_u64()
        .expect("orphan_count field present in JSON output");

    assert_eq!(
        orphan_count,
        0,
        "reify-audit has {orphan_count} unmarked orphan pub fn(s); \
         each needs a `// G-allow: ...` comment on the line immediately \
         above the `pub fn` declaration.\nOrphans:\n{:#}",
        result["orphans"]
    );
}

/// `reify_audit_pub_fns_are_g_allow_marked` must survive a real *ambient* hook
/// git environment, not just a per-child one.
///
/// It spawns `scripts/audit-orphan-producers.sh`, and under
/// `hooks/pre-commit` -> `hooks/project-checks` -> `scripts/verify.sh` the
/// whole process tree carries `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE`. So
/// re-run it in a child that has that poison ambient, which is the real hook
/// condition rather than a simulation of it.
///
/// # The discrimination this test buys
///
/// - Unsanitized spawn -> `run_orphan_audit_at`'s repo-root premise probe
///   (itself routed through the same, now-broken sanitizer) resolves the
///   ambient GIT_DIR/GIT_WORK_TREE/GIT_INDEX_FILE redirect to the harness's
///   decoy tree instead of the real repo root -> the probe's mismatch check
///   fires -> `panic!("...would run against a DIFFERENT repository than
///   requested...")`, inside `crates/reify-test-support/src/orphan_audit.rs`
///   and before the script is even spawned -> the child exits non-zero ->
///   `replay_self_under_hook_git_env`'s status assertion fails -> RED.
/// - Sanitized spawn -> the probe agrees with the real repo root -> the
///   script actually runs -> a JSON envelope -> `orphan_count == 0` -> the
///   child is green -> GREEN.
///
/// To check this test has not gone vacuous: temporarily drop the
/// `cmd.env_remove(var)` from `reify_test_support`'s `sanitize()`. The child
/// panics, exits 101, and this test fails on the harness's status assertion;
/// restoring the line restores GREEN. One line and one `cargo test`.
///
/// That check is the only way to see it RED. Task 5605's `.env_remove()`
/// calls have landed, so no clean checkout reproduces the original failure and
/// CI will never delete such a line — which is why this test is one leg of a
/// trio rather than a lone guard. The rule lives in this module's doc, under
/// "The hook-git-env trio"; do not restate it here.
///
/// This test probes first and spawns no child at all when the probe skips —
/// see the earned-mark rule (module doc, and the test it names).
///
/// (The original RED measurement, and the task-5605/5698 history of where the
/// child dies, are in project memory — `search(project_id="reify", query="
/// run_orphan_audit replay child envelope mark skip 5698 repo-root premise
/// probe panic")`, measured to return all three records. They are deliberately
/// not restated here, where nothing checks them and they would rot.)
///
/// # Why a test NAME rather than the empty filter
///
/// Only one test in this binary is exposed to the hazard. An empty filter
/// would also drag the synthetic witness into the child, where it poisons and
/// strips its OWN children's environments — pure cost that dilutes the floor's
/// meaning, and now a hard failure on that helper's replay-child
/// precondition. Naming the target also keeps the selection exact: no other
/// test name in this binary contains [`TARGET_TEST`] as a substring, so the
/// replay cannot select
/// itself. The helper's `REIFY_AUDIT_HOOK_ENV_REPLAY` guard is the second line
/// of defence.
///
/// The floor of 1 is therefore exact rather than a lower bound. It exists
/// because libtest exits 0 on a zero-match filter: without it, renaming the
/// target test or moving it to another binary would silently downgrade this
/// harness to a vacuous pass. Raising the floor would be a claim that MORE
/// than one test here is hazard-exposed — do that only alongside widening the
/// filter to actually select them.
#[test]
fn orphan_audit_survives_ambient_hook_git_env() {
    // EARN the mark before spawning anything — this call IS the earning, and
    // the earned-mark rule is why it must precede the spawn. The whole skip
    // protocol is delegated to `run_orphan_audit` rather than re-probed here,
    // for the reason stated in `audit_script_stdout_poisoned_and_sanitized`'s
    // doc. Cost: one extra scoped script run in the parent.
    if run_orphan_audit(SCOPE).is_none() {
        eprintln!(
            "run_orphan_audit({SCOPE:?}) produced no envelope in this environment, \
             so there is nothing for a replay child to preserve; skipping the \
             ambient-hook-git-env replay rather than spawning a child that could \
             only reproduce the same skip"
        );
        return;
    }

    common::git_env::replay_self_under_hook_git_env_expecting_envelope(&[TARGET_TEST], 1);
}

/// The replay child's hard failure must fire ONLY where the parent has
/// established that this environment can produce an audit envelope — never on
/// a mere "am I a replay child?".
///
/// This doc is that rule's ONLY home: `common::git_env` (module doc, both
/// predicates, both spawn entry points, the mark enum and its two constants),
/// this module's own doc, and both other trio members carry a pointer to it
/// and no re-derivation — as this doc points at "The hook-git-env trio" above
/// rather than restating that. Editing here is editing the rule everywhere.
///
/// # The regression this pins
///
/// A tightening keyed on mere child-ness reddens any environment lacking the
/// audit's prerequisites: the child skips as designed, then panics claiming
/// its parent had just run the audit successfully — a diagnosis its own
/// stderr contradicts one line earlier, on an environment the skip protocol
/// exists to support.
///
/// The premise was false, not merely unlucky.
/// `replay_self_under_hook_git_env` only `--list`s the selection in the parent
/// — it never runs the target, so it never learns whether the parent's own run
/// produced an envelope — and libtest guarantees no ordering between the two
/// tests.
///
/// (The command that measured it and its verbatim output are in project
/// memory, not here where nothing re-measures them — same search as the
/// pointer in `orphan_audit_survives_ambient_hook_git_env` above.)
///
/// # What is and is not covered
///
/// One skip cause, the PATH-deprived one. `run_orphan_audit` has others (a
/// `repo_root` outside any git work tree, the script absent from disk) and
/// neither half induces them — a bounded claim rather than a hole, since the
/// tightening branches on the MARK and never on the cause. The work-tree cause
/// is in particular NOT reachable by pointing the child's `current_dir` at a
/// non-git tempdir: `run_orphan_audit` resolves `repo_root` at compile time
/// from `env!("CARGO_MANIFEST_DIR")` and probes with `.current_dir(repo_root)`,
/// so the child's own cwd never enters it.
///
/// # The two halves
///
/// Both spawn the SAME target test in the SAME deprived environment (an empty
/// `PATH`, so `run_orphan_audit`'s python3 probe takes its skip path) via
/// [`common::git_env::spawn_replay_child_lacking_audit_prereqs`], and differ
/// ONLY in the value stamped into the replay guard:
///
/// - PLAIN mark — what a caller that verified nothing stamps. The graceful
///   skip must SURVIVE: child exits 0, summary reports 1 passed.
/// - ENVELOPE mark — what a caller stamps only after seeing an envelope in
///   this same environment. The tightening must keep its teeth: child exits
///   non-zero, summary reports 1 failed.
///
/// So this test pins a discrimination, not a direction: it fails both if the
/// tightening over-fires on a supported environment and if it is loosened into
/// never firing at all.
///
/// Each half asserts FIRST that the child's stderr carries `skipping orphan
/// audit`. That marker string lives in this repo
/// (`crates/reify-test-support/src/orphan_audit.rs`), so keying on it adds no
/// cross-tool coupling, and it is what ATTRIBUTES each half to the deprived
/// fixture: without it, half A could pass green-for-the-wrong-reason on a
/// machine where the audit genuinely ran and succeeded.
///
/// It is the wording the PREREQUISITE-PROBE skip notes share, not every skip
/// note in that file — so keying on the family rather than on the python3
/// probe's own wording buys one thing: reordering those probes among
/// themselves (an empty `PATH` hides `git` too) moves this test onto another
/// of them instead of reddening it. Reaching a skip note that does NOT carry
/// the marker fails BOTH halves here, loudly and on a stated premise — but an
/// empty `PATH` cannot reach one, since it leaves the script on disk and never
/// gets far enough to run it.
///
/// Deliberately NOT an enumerated census of which notes carry it: nothing
/// would check such a list, and it would read as authoritative while rotting
/// on the next probe added or reworded. The single-source fix is a `pub const`
/// in `reify-test-support` that the `eprintln!`s and this test both read,
/// which needs an edit to `crates/reify-test-support/src/orphan_audit.rs` —
/// outside this task's lock set, filed separately.
///
/// The counts come from libtest's summary rather than from the tightening
/// panic's prose, so rewording that panic does not fail this test.
#[test]
fn replay_child_hard_fails_only_when_the_parent_verified_an_envelope() {
    use common::git_env::ReplayMark;

    // The wording `run_orphan_audit`'s prerequisite-probe skip notes share —
    // NOT every skip note in it. A copy of a string that lives in another
    // crate's `eprintln!`s, so a reword there reddens both halves below
    // instead of silently un-attributing them; see the doc above.
    const SKIP_MARKER: &str = "skipping orphan audit";

    // Literally the same filter the replay harness uses — the module-level
    // const, not a second spelling of it — so this fixture cannot pin the
    // behaviour of a test the real replay no longer selects.
    const TARGET: [&str; 1] = [TARGET_TEST];

    // --- Half A: an unverified mark must leave the graceful skip intact ---
    let plain =
        common::git_env::spawn_replay_child_lacking_audit_prereqs(&TARGET, ReplayMark::Plain);
    let plain_stdout = String::from_utf8_lossy(&plain.stdout);
    let plain_stderr = String::from_utf8_lossy(&plain.stderr);

    assert!(
        plain_stderr.contains(SKIP_MARKER),
        "the deprived child did not report {SKIP_MARKER:?}, so this half is not \
         exercising the environment it claims: either the fixture's empty PATH \
         no longer reaches any of `run_orphan_audit`'s prerequisite probes, or \
         the wording they share was changed (update SKIP_MARKER). Whatever this \
         child did assert below, it was not about an environment that cannot \
         run the audit.\n\
         --- child stderr (truncated) ---\n{:.600}",
        plain_stderr,
    );
    assert!(
        plain.status.success(),
        "a replay child stamped with {:?} FAILED (exit \
         {:?}) in an environment that simply lacks the audit's prerequisites. \
         Nothing established \
         that this environment can run the audit, so `run_orphan_audit`'s \
         graceful skip is the contract — a tightening that fires here turns a \
         supported environment into a red build and hands the operator a \
         diagnosis its own stderr contradicts.\n\
         --- child stdout (truncated) ---\n{:.800}\n\
         --- child stderr (truncated) ---\n{:.800}",
        ReplayMark::Plain,
        plain.status.code(),
        plain_stdout,
        plain_stderr,
    );
    assert_eq!(
        common::git_env::libtest_summary_count(&plain_stdout, "passed"),
        Some(1),
        "the PLAIN-mark child exited 0, but its libtest summary does not report \
         exactly 1 passing test — so the skip was not what made it green. \
         libtest exits 0 on a zero-match filter, so a renamed or relocated \
         {:?} would look identical here.\n\
         --- child stdout (truncated) ---\n{:.800}",
        TARGET,
        plain_stdout,
    );

    // --- Half B: the earned mark must keep the tightening's teeth ---
    let envelope =
        common::git_env::spawn_replay_child_lacking_audit_prereqs(&TARGET, ReplayMark::Envelope);
    let envelope_stdout = String::from_utf8_lossy(&envelope.stdout);
    let envelope_stderr = String::from_utf8_lossy(&envelope.stderr);

    assert!(
        envelope_stderr.contains(SKIP_MARKER),
        "the deprived child did not report {SKIP_MARKER:?} — same diagnosis as \
         half A: this half is not exercising an environment that cannot run \
         the audit, so whatever it proves is not what it claims.\n\
         --- child stderr (truncated) ---\n{:.600}",
        envelope_stderr,
    );
    assert!(
        !envelope.status.success(),
        "a replay child stamped with {:?} exited \
         0 despite skipping the audit. That mark is stamped only after the \
         parent has SEEN an envelope for this scope in this environment, so a \
         skip here means the environment changed underfoot or the sanitizer \
         stopped working — exactly the condition \
         `reify_audit_pub_fns_are_g_allow_marked`'s tightening exists to \
         catch. \
         It has lost its teeth: half A's fix has been over-applied.\n\
         --- child stdout (truncated) ---\n{:.800}\n\
         --- child stderr (truncated) ---\n{:.800}",
        ReplayMark::Envelope,
        envelope_stdout,
        envelope_stderr,
    );
    assert_eq!(
        common::git_env::libtest_summary_count(&envelope_stdout, "failed"),
        Some(1),
        "the ENVELOPE-mark child exited non-zero, but its libtest summary does \
         not report exactly 1 FAILING test — so the tightening is not what made \
         it red. A child that died before libtest ran (a bad spawn, an aborted \
         process) exits non-zero too, and would prove nothing about the mark.\n\
         --- child stdout (truncated) ---\n{:.800}\n\
         --- child stderr (truncated) ---\n{:.800}",
        envelope_stdout,
        envelope_stderr,
    );
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
struct AuditRun {
    stdout: String,
    status: ExitStatus,
    stderr: String,
}

/// Run `scripts/audit-orphan-producers.sh --scope <scope> --quiet --format
/// json` TWICE against one shared [`common::git_env::decoy_repo`]: once with
/// the hook poison ambient in the child, once with
/// [`reify_audit::git_env::sanitize`] applied
/// — the same baseline `reify_test_support::sanitize` strips in production,
/// not just the three vars this helper poisons. Returns `(poisoned,
/// sanitized)` as [`AuditRun`]s.
///
/// Both commands come from ONE closure against ONE decoy, so the environment
/// is provably the only delta. `sanitize` is the canonical one production
/// uses, not a hand-rolled removal loop over `REPO_REDIRECT_VARS`, so
/// "sanitized" here means what production means by it; and
/// `common::git_env`'s own assertion that every var it poisons is one
/// `sanitize` removes makes "the poisoned set is a subset of the sanitized
/// set" a fact about the code rather than a claim in this comment.
///
/// # Graceful-skip protocol — delegated, not re-implemented
///
/// Returns `None`, with an explanatory `stderr` note, exactly when
/// `reify_test_support::run_orphan_audit` declines an envelope for `scope`.
/// That one call IS the protocol — `python3`/`git` presence, script-on-disk,
/// `repo_root`-is-a-git-work-tree, `EXCLUDE_CRATES` membership. Do not
/// re-implement any of it: its most fragile element is a git diagnostic string
/// the work-tree probe keys on, so a second copy drifts the moment either
/// git's wording or that probe changes. Delegating also inherits the
/// protocol's LOUD half — a `git rev-parse --show-toplevel` failing for a
/// reason OTHER than "no repository here" (a corrupt `.git`, dubious ownership
/// under this project's shared worktree topology) panics naming the probe's
/// status and stderr, where the re-implementation swallowed both and left the
/// caller blaming a broken `--scope`.
///
/// SKIPPING, rather than comparing, is the only honest answer, because every
/// cause of that `None` empties BOTH halves: without `python3` the script
/// exits 3 with no stdout either way, and an `EXCLUDE_CRATES` scope
/// legitimately emits nothing (reachable by any future caller, since this
/// helper is generic over `scope`). A caller comparing them would fail its
/// "sanitized is non-empty" assertion while passing its "poisoned is empty"
/// one — a spurious RED saying nothing about the hazard.
///
/// The gate is a THIRD script run, duplicating the sanitized spawn's work and
/// discarding its envelope. Deriving the skip from that run instead would mean
/// re-deciding "was this a skip?" from stdout and stderr here — the
/// re-implementation ruled out above, and it would lose the loud half. The
/// redundant spawn is what that costs.
///
/// Must NOT be called from inside a replay child: the gate would hit
/// `run_orphan_audit`'s repo-root mismatch panic rather than skipping. Refused
/// below via [`common::git_env::assert_not_in_replay_child`] rather than left
/// to this comment plus the replay filter's substring choice, so widening that
/// filter — or adding a test here whose name happens to match it — fails on
/// the precondition instead of three frames down inside `reify-test-support`.
///
/// Asserts nothing about either run: it reports stdout, status and stderr on
/// [`AuditRun`] and leaves every judgement to the caller, which needs all
/// three to tell "redirected into the empty decoy and ran to completion"
/// (exit 0) from "aborted before scanning" (non-zero). Spawn failures are hard
/// failures, matching `run_orphan_audit`.
fn audit_script_stdout_poisoned_and_sanitized(scope: &str) -> Option<(AuditRun, AuditRun)> {
    common::git_env::assert_not_in_replay_child(
        "audit_script_stdout_poisoned_and_sanitized",
        "its `run_orphan_audit` gate would hit the repo-root mismatch panic instead \
         of skipping",
    );

    // The ENTIRE graceful-skip protocol, in one delegated call — see this
    // function's doc for why it is delegated rather than re-implemented, and
    // for why every cause of a `None` makes the comparison below meaningless.
    if run_orphan_audit(scope).is_none() {
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
    // script path itself.
    //
    // TODO(#6153): delete this walk and the argv below in favour of a public
    // seam on `reify_test_support::orphan_audit`, and drop the two premise
    // checks that exist only to bound them. That file is outside the lock set
    // of the task that owns this one, which is why the copy is here at all.
    // (This crate is on the ptodo detector's own allowlist — `reify-audit` is
    // the tool — so this cite documents rather than enrols; the task is the
    // record either way.)
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
    // path itself rather than a reconstruction of it — the same public seam
    // task #6153 tracks above, not a second piece of work.
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

    let decoy = common::git_env::decoy_repo();

    // One closure, so the two spawns are provably identical apart from the
    // environment delta below.
    let build = || {
        let mut cmd = Command::new(&script);
        cmd.args(["--scope", scope, "--quiet", "--format", "json"])
            .current_dir(repo_root);
        cmd
    };

    let mut poisoned_cmd = build();
    common::git_env::poison_with_hook_git_env(&mut poisoned_cmd, &decoy);

    let mut sanitized_cmd = build();
    common::git_env::poison_with_hook_git_env(&mut sanitized_cmd, &decoy);
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

/// An ambient hook git environment really does defeat
/// `scripts/audit-orphan-producers.sh`, and stripping exactly those variables
/// really does cure it.
///
/// # Why this test exists
///
/// `orphan_audit_survives_ambient_hook_git_env` is the test that guards the
/// PRODUCTION call site. But task 5605 already landed the `.env_remove()`
/// calls that test depends on, so from a clean checkout there is no longer any
/// way to watch it go RED — doing so would mean deleting an `env_remove` line
/// from `crates/reify-test-support/src/orphan_audit.rs`, which CI never does.
/// A guard whose teeth can never be demonstrated decays into a guard nobody
/// trusts.
///
/// So this test re-demonstrates the hazard's potency directly and
/// synthetically: it spawns the audit script twice, differing ONLY in whether
/// the hook variables are stripped, with no dependency on the production call
/// site at all. It is one leg of a trio; the rule lives in this module's doc,
/// under "The hook-git-env trio", and is deliberately not restated here.
///
/// # What each half demonstrates
///
/// `audit-orphan-producers.sh --scope crates/reify-audit/src --quiet --format
/// json`, two ways — poisoned, then sanitized — against a `git init`ed
/// tempdir carrying a planted `.git/index.lock`, exactly what
/// [`common::git_env::decoy_repo`] builds. There is no third, clean run: the
/// sanitized half IS the clean baseline, since stripping the poison is what
/// restores the unpoisoned environment.
///
/// - `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE` pointed at the decoy: exit 0,
///   empty stdout, stderr `audit-orphan-producers.sh: no source files matched`.
/// - the same poison, then [`reify_audit::git_env::sanitize`]d: exit 0, a JSON
///   envelope with a numeric `orphan_count`.
///
/// The mechanism is the script's `REPO_ROOT="$(git rev-parse
/// --show-toplevel)"` followed by `cd "$REPO_ROOT"` (cited by content, not
/// line number — that line has already moved once). An ambient
/// `GIT_DIR`/`GIT_WORK_TREE` overrides both the cwd and any `-C`, so the whole
/// scan is redirected into the empty decoy tree, matches no source files, and
/// emits nothing.
///
/// Why the poisoned half asserts that exit status and that stderr marker
/// rather than emptiness alone is at the assertions themselves, where a reader
/// meeting a failure will be.
///
/// BOTH halves are hard assertions, deliberately. Softening the poisoned half
/// to an `eprintln!` would spare a script that hardened itself out of the
/// hazard — but libtest swallows stderr on a passing test, so nothing
/// observable would happen, and a harness regression that made the two halves
/// identical would still report PASS. The hardening case is real but one-off:
/// the sanctioned response is to retire this test deliberately, together with
/// what it guards, not to leave it permanently self-disabled. The failure
/// message says so.
///
/// The sanitized-half assertions deliberately pin only "non-empty and parses
/// as an envelope with a numeric `orphan_count`" — never a byte count or a
/// scanned-fn count. Those track this crate's incidental corpus size and drift
/// with any unrelated `pub fn` addition; pinning them would make such an
/// addition fail this test, which is how a signal gets weakened or deleted by
/// a later maintainer.
#[test]
fn hook_git_env_defeats_the_audit_script_and_stripping_it_cures_the_defeat() {
    let Some((poisoned, sanitized)) = audit_script_stdout_poisoned_and_sanitized(SCOPE) else {
        // The helper gates on `reify_test_support::run_orphan_audit`, so this
        // is that function's own graceful-skip protocol verbatim — python3 /
        // git / the script absent, `repo_root` outside any git work tree, or a
        // scope in EXCLUDE_CRATES. Each of those empties BOTH halves, so the
        // comparison below would prove nothing.
        return;
    };

    // `{:.400}` is a Display precision, i.e. a truncating max width: enough of
    // the offending stdout/stderr to diagnose a failure without dumping ~9 KiB.
    //
    // The sanitized half is asserted FIRST. Both halves are hard assertions
    // now, so ordering no longer decides whether a check runs at all — but it
    // still decides which diagnosis a reader meets first, and "the script
    // cannot produce output even unpoisoned" is the more fundamental failure:
    // it explains an empty poisoned half too, whereas the reverse is not true.
    assert!(
        !sanitized.stdout.trim().is_empty(),
        "stripping GIT_DIR/GIT_WORK_TREE/GIT_INDEX_FILE did NOT restore the \
         audit script's output — it emitted nothing (exit {:?}). Stripping \
         those vars is supposed to be the whole cure, and the environment is \
         already vetted: `reify_test_support::run_orphan_audit` ran this same \
         script against this same scope moments ago and got an envelope, or \
         this test would have skipped. So the difference is something this \
         test's own two spawns introduce — a mis-sanitized command, or a \
         `.parent()` walk resolving a different script than the gate vetted. \
         The stderr below is the script's own account.\n\
         --- sanitized stdout (truncated) ---\n{:.400}\n\
         --- sanitized stderr (truncated) ---\n{:.400}",
        sanitized.status.code(),
        sanitized.stdout,
        sanitized.stderr,
    );

    let envelope: serde_json::Value = serde_json::from_str(&sanitized.stdout).unwrap_or_else(|e| {
        panic!(
            "stripping the hook git environment produced non-empty output that \
                 is not valid JSON: {e}\n--- sanitized stdout (truncated) \
                 ---\n{:.400}",
            sanitized.stdout,
        )
    });

    assert!(
        envelope["orphan_count"].as_u64().is_some(),
        "the sanitized run parsed as JSON but carries no numeric \
         `orphan_count`, so it is not the audit envelope this test claims \
         stripping restores.\n--- parsed value (truncated) ---\n{:.400}",
        envelope.to_string(),
    );

    // The "poisoned" half runs LAST — the ordering above is load-bearing and
    // unchanged — but these are HARD assertions, because they are the only
    // thing this test uniquely contributes. Everything asserted above is
    // already pinned by `reify_audit_pub_fns_are_g_allow_marked` (same binary,
    // same scope).
    //
    // Status and stderr are asserted BEFORE the emptiness they explain. Empty
    // stdout is produced both by the hazard (scan redirected into the empty
    // decoy, script runs to completion, exit 0) and by the script dying before
    // it scanned anything (exit non-zero) — so the emptiness check alone
    // cannot attribute the silence to the GIT_DIR/GIT_WORK_TREE redirect, and
    // a broken fixture would report as a green demonstration of the hazard.
    assert!(
        poisoned.status.success(),
        "the audit script did not RUN under the hook git environment — it \
         exited {:?} rather than 0. This test's claim is that the poison \
         redirects the scan into an empty decoy tree which the script then \
         reports on normally, NOT that the poison kills the script. A non-zero \
         exit means the decoy is malformed rather than merely empty (a tempdir \
         cleaned early, a `git init` that left no usable object store), so the \
         script's own `git rev-parse --show-toplevel` failed under `set -euo \
         pipefail` and it aborted before scanning. Fix the fixture — do not \
         relax this assertion, or the empty stdout below stops meaning \
         anything.\n\
         --- poisoned stderr (truncated) ---\n{:.400}",
        poisoned.status.code(),
        poisoned.stderr,
    );

    // The script's OWN marker, not git's `fatal:` wording: this string lives
    // in `scripts/audit-orphan-producers.sh`, so keying on it couples this
    // test to this repo rather than to git's diagnostics.
    const NO_SOURCES_MARKER: &str = "audit-orphan-producers.sh: no source files matched";
    assert!(
        poisoned.stderr.contains(NO_SOURCES_MARKER),
        "the audit script exited 0 under the hook git environment but did not \
         report {NO_SOURCES_MARKER:?} — so it did not take the \
         scanned-an-empty-tree path this test claims the poison forces it \
         down, and the empty stdout below has some other cause. Either the \
         script's no-match reporting changed (update this marker) or the \
         poison is no longer redirecting the scan (see the two causes in the \
         next assertion).\n\
         --- poisoned stderr (truncated) ---\n{:.400}",
        poisoned.stderr,
    );

    assert!(
        poisoned.stdout.trim().is_empty(),
        "the hook git environment no longer defeats the audit script: with \
         GIT_DIR/GIT_WORK_TREE/GIT_INDEX_FILE pointed at an empty decoy repo, the \
         script still emitted {} byte(s) on stdout (exit {:?}), the same shape the \
         sanitized run produced. Two possible causes, and they need opposite \
         responses. (1) This harness regressed and the poison never reached the \
         child — check that `poison_with_hook_git_env` still applies all three vars \
         to the POISONED command, and that `decoy_repo` still yields an empty tree; \
         fix it. (2) The script deliberately hardened itself, e.g. it no longer \
         resolves its repo root via `git rev-parse --show-toplevel`; then this \
         hazard is genuinely dead, and the right move is to retire this test \
         together with what it guards — `reify_audit_pub_fns_are_g_allow_marked`'s \
         envelope-marked replay-child panic, the \
         `replay_child_hard_fails_only_when_the_parent_verified_an_envelope` test \
         that bounds when that panic may fire, and reify-test-support's \
         `sanitize()` — rather than to \
         weaken this assertion back into a log line that no passing run ever \
         shows.\n\
         --- poisoned stdout (truncated) ---\n{:.400}\n\
         --- poisoned stderr (truncated) ---\n{:.400}",
        poisoned.stdout.len(),
        poisoned.status.code(),
        poisoned.stdout,
        poisoned.stderr,
    );
}

/// [`common::git_env::libtest_summary_count`] is the single parser both
/// `replay_with_mark`'s non-vacuity checks and
/// `replay_child_hard_fails_only_when_the_parent_verified_an_envelope`'s two
/// count assertions read child summaries through, so its stated properties
/// need pinning here rather than inferring from those callers — a parser bug
/// there surfaces as a confusing count mismatch attributed to the child.
///
/// Driven over literal libtest summary lines rather than a spawned child:
/// these are pure-function properties, and a spawned child could only produce
/// whichever shapes today's tests happen to reach.
#[test]
fn libtest_summary_count_reads_the_field_it_was_asked_for() {
    use common::git_env::libtest_summary_count;

    const OK: &str = "test result: ok. 5 passed; 0 failed; 2 ignored; 0 measured; 30 filtered out";
    const FAILED: &str =
        "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out";

    // Both summary verdicts parse — the `FAILED.` shape is what half B of the
    // discrimination test reads, and it differs from `ok.` before the counts.
    assert_eq!(libtest_summary_count(OK, "passed"), Some(5));
    assert_eq!(libtest_summary_count(OK, "failed"), Some(0));
    assert_eq!(libtest_summary_count(OK, "ignored"), Some(2));
    assert_eq!(libtest_summary_count(FAILED, "passed"), Some(0));
    assert_eq!(libtest_summary_count(FAILED, "failed"), Some(1));

    // The property the doc claims: the count is parsed as a NUMBER, so a
    // 21-passing run is not read as the 1 that both `Some(1)` call sites
    // compare against. A substring match on `"1 passed"` would return `Some(1)`
    // here and silently turn each of those assertions into a green.
    const TWENTY_ONE: &str =
        "test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out";
    assert_eq!(libtest_summary_count(TWENTY_ONE, "passed"), Some(21));

    // A field this parser knows nothing about yields `None`, NOT a count. Both
    // call sites compare against `Some(1)`, so a misspelled field fails the
    // assertion rather than reading some neighbouring number — the failure is
    // confusing, but it is a failure, and this pins that it stays one.
    assert_eq!(libtest_summary_count(OK, "pased"), None);
    assert_eq!(libtest_summary_count(OK, "measured"), Some(0));

    // No summary at all: `None`, which is what `replay_with_mark` turns into
    // its "could not find libtest's `test result:` summary" panic.
    assert_eq!(libtest_summary_count("", "passed"), None);
    assert_eq!(
        libtest_summary_count("running 1 test\ntest foo ... ok\n", "passed"),
        None
    );
}

/// The LAST `test result:` line wins — the other property
/// [`common::git_env::libtest_summary_count`]'s doc claims, and the one that
/// matters in practice: every replay child is spawned with `--nocapture`, so a
/// test's own stdout is interleaved with libtest's and can carry the same
/// prefix.
///
/// Not hypothetical for this binary: the child runs
/// `reify_audit_pub_fns_are_g_allow_marked`, whose failure message embeds the
/// audit's JSON, and this file's own assertion messages embed truncated child
/// stdout — which is a real summary line, verbatim, one nesting level down.
#[test]
fn libtest_summary_count_takes_the_last_summary_line() {
    use common::git_env::libtest_summary_count;

    // A decoy `test result:` line emitted by the test's OWN output under
    // `--nocapture`, ahead of the real summary. Reading the first match would
    // report the decoy's 99.
    let interleaved = concat!(
        "running 1 test
",
        "some test echoed a captured child summary:
",
        "test result: ok. 99 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
",
        "test reify_audit_pub_fns_are_g_allow_marked ... ok
",
        "
",
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out
",
    );
    assert_eq!(libtest_summary_count(interleaved, "passed"), Some(1));

    // libtest indents nothing, but a nested child's summary reaching the
    // parent through a `--- child stdout ---` block may arrive indented. The
    // parser trims before matching the prefix, so such a line is still a
    // candidate — and being LAST is what decides, not indentation.
    let indented = concat!(
        "test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
",
        "    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
",
    );
    assert_eq!(libtest_summary_count(indented, "passed"), Some(2));
}
