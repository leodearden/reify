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
//! `crates/reify-kernel-gmsh/tests/rpath_smoke.rs`. The shared helper is
//! `reify_test_support::run_orphan_audit`. That skip has exactly one
//! exception, gated on `common::git_env::replay_child_expects_envelope`.
//!
//! # The hook-git-env trio
//!
//! The three hook-git-env tests below are one invariant in three parts:
//!
//! - `orphan_audit_survives_ambient_hook_git_env` — production sanitizes.
//! - `hook_git_env_defeats_the_audit_script_and_stripping_it_cures_the_defeat`
//!   — sanitizing is what makes the difference, demonstrated synthetically so
//!   the hazard stays visible from a clean checkout.
//! - `replay_child_hard_fails_only_when_the_parent_verified_an_envelope` —
//!   neither tightening may fire where the audit was never shown able to run,
//!   so the first cannot buy its teeth by reddening supported environments.
//!
//! Why the synthetic leg exists: 5605 landed the `.env_remove()` calls the
//! first leg depends on, so no clean checkout can watch that one go RED any
//! more. The original RED measurement and the 5605/5698 history are in project
//! memory: `search(project_id="reify", query="run_orphan_audit replay child
//! envelope mark skip 5698 repo-root premise probe panic")`.
//!
//! None can notice another going vacuous. Retire them together or not at all.

use std::path::Path;
use std::process::{Command, ExitStatus, Output};

use reify_test_support::run_orphan_audit;

mod common;

use common::git_env::ReplayMark;

/// The audit scope every test in this file uses.
///
/// One home because several assertions below rest on all the runs being of the
/// SAME scope, and nothing compares the literals at runtime — a second spelling
/// could drift and silently falsify that premise while every test stayed green.
const SCOPE: &str = "crates/reify-audit/src";

/// The test the replay and the deprived-`PATH` fixture both select, by libtest
/// positional filter.
///
/// One home for the same reason as [`SCOPE`], plus one specific to a filter:
/// "no other test name in this binary contains this substring" — what keeps the
/// replay from selecting itself — is a property of ONE string.
const TARGET_TEST: &str = "reify_audit_pub_fns_are_g_allow_marked";

#[test]
fn reify_audit_pub_fns_are_g_allow_marked() {
    // FIRST statement, above everything that can skip or panic, so the
    // breadcrumb is out regardless of what this run then does. A no-op outside
    // an envelope-marked replay child; inside one it is the child's half of the
    // round trip `orphan_audit_survives_ambient_hook_git_env` relies on.
    common::git_env::announce_replay_mark();

    let audit = run_orphan_audit(SCOPE);

    // Defence-in-depth against `run_orphan_audit`'s public contract, which
    // still permits `None`, scoped to the one child where a skip has no
    // innocent reading; the graceful-skip path below is untouched everywhere
    // else. What this catches and the replay's own assertions cannot: a skip is
    // a `return` and libtest has no skipped state, so a skipping child exits 0
    // reporting `1 passed` — status, `passed + ignored == listed` and floor
    // checks ALL hold on a run that exercised nothing. Only the child can tell
    // the two apart, by refusing to skip.
    if audit.is_none() && common::git_env::replay_child_expects_envelope() {
        panic!(
            "run_orphan_audit returned None inside a replay child stamped with \
             the envelope mark — stamped only by a parent that got an envelope \
             for this same scope, in this same environment, moments earlier. So \
             a missing `python3`/`git`/script is not a plausible reading here: \
             what changed between that run and this one is the ambient hook git \
             environment this child carries."
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
/// git environment, not just a per-child one: it spawns
/// `scripts/audit-orphan-producers.sh`, and under `hooks/pre-commit` ->
/// `hooks/project-checks` -> `scripts/verify.sh` the whole process tree carries
/// `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE`.
///
/// The discrimination: an unsanitized spawn lets that ambient redirect reach
/// `run_orphan_audit_at`'s repo-root premise probe, which then resolves the
/// harness's decoy tree instead of the real root and panics inside
/// `crates/reify-test-support/src/orphan_audit.rs` before the script is even
/// spawned — the child exits non-zero and the replay's status assertion fails.
/// A sanitized spawn resolves the real root, gets a JSON envelope, and is
/// green.
///
/// That discrimination is armed by the envelope mark, so the spawn goes through
/// the entry point whose whole identity IS that mark rather than one this call
/// site picks. Stamping [`ReplayMark::Plain`] here instead leaves this binary
/// byte-identical to the unmutated tree — in a real replay child production
/// sanitizes, so the mark-gated branch is never evaluated — which is why that
/// entry point also OBSERVES the mark its own child reported carrying, via the
/// breadcrumb `reify_audit_pub_fns_are_g_allow_marked` emits first.
///
/// To check this has not gone vacuous: drop an `env_remove` from
/// `reify_test_support`'s `sanitize()` and the child exits 101; restoring it
/// restores GREEN. That mutation is the only way to see this leg RED — see the
/// module doc.
///
/// Filters on one test NAME rather than `""`: only that test is hazard-exposed,
/// and an empty filter would also drag this file's other spawning tests into
/// the child, where their replay-child preconditions refuse. No other test name
/// here contains [`TARGET_TEST`] as a substring, so the replay cannot select
/// itself and the floor of 1 is exact — libtest exits 0 on a zero-match filter,
/// so raise that floor only alongside widening the filter.
#[test]
fn orphan_audit_survives_ambient_hook_git_env() {
    // EARN the mark before spawning anything: `ReplayMark::Envelope`'s whole
    // content is that this process saw an envelope here moments ago, so the
    // probe must precede the spawn. The skip protocol is delegated to
    // `run_orphan_audit` rather than re-probed here, for the reason in
    // `audit_script_stdout_poisoned_and_sanitized`'s doc. Cost: one extra
    // scoped script run in the parent.
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

/// Spawn ONE replay child in an environment that genuinely CANNOT run the
/// orphan audit, stamping `mark` as the replay guard's value.
///
/// The fixture behind
/// [`replay_child_hard_fails_only_when_the_parent_verified_an_envelope`]: it
/// holds everything fixed except `mark`, so that test's two children differ
/// only in what the mark claims. The command body is shared with the real
/// replay via [`common::git_env::replay_child_command`].
///
/// An EMPTY `PATH` makes `reify_test_support::run_orphan_audit`'s prerequisite
/// probes fail with `NotFound`, so it takes its documented skip path — a
/// SUPPORTED environment, not a broken one, and the caller asserts on the skip
/// note the child actually emits. Deliberately NOT poisoned with the
/// hook git environment: the child skips long before it reaches the audit
/// script, so a decoy would add no signal — what this fixture buys is the
/// MARK's meaning, not the poison's. Spawn failures are hard failures, since
/// `current_exe()` is this very binary.
fn spawn_child_lacking_audit_prereqs(filters: &[&str], mark: ReplayMark) -> Output {
    // Held until after `output()` returns, so the child sees a PATH that exists
    // and is empty rather than one pointing at a deleted directory.
    let empty_path = tempfile::tempdir().expect("create empty PATH dir for the deprived child");

    common::git_env::replay_child_command(filters, mark)
        .env("PATH", empty_path.path())
        .output()
        .expect("re-exec self with the orphan audit's prerequisites removed")
}

/// The replay child's hard failure must fire ONLY where the parent has
/// established that this environment can produce an audit envelope — never on a
/// mere "am I a replay child?".
///
/// The regression this pins: a tightening keyed on mere child-ness reddens any
/// environment lacking the audit's prerequisites — the child skips as designed,
/// then panics claiming its parent had just run the audit successfully, a
/// diagnosis its own stderr contradicts one line earlier. That premise is false
/// rather than unlucky, since the replay only `--list`s the selection in the
/// parent and so never learns what the parent's own run produced.
///
/// Both halves spawn the SAME target test in the SAME deprived environment via
/// [`spawn_child_lacking_audit_prereqs`], and differ ONLY in the mark:
///
/// - PLAIN — what a caller that verified nothing stamps. The graceful skip must
///   SURVIVE: child exits 0, summary reports 1 passed.
/// - ENVELOPE — what a caller stamps only after seeing an envelope in this same
///   environment. The tightening must keep its teeth: child exits non-zero,
///   summary reports 1 failed.
///
/// So this pins a discrimination, not a direction: it fails both if the
/// tightening over-fires on a supported environment and if it is loosened into
/// never firing at all. Each half asserts FIRST that the child's stderr carries
/// `skipping orphan audit`, which is what ATTRIBUTES it to the deprived
/// fixture; the counts then come from libtest's summary rather than from the
/// tightening panic's prose, so rewording that panic does not fail this test.
///
/// Covers ONE of `run_orphan_audit`'s skip causes, the PATH-deprived one — a
/// bounded claim rather than a hole, since the tightening branches on the MARK
/// and never on the cause. The work-tree cause is in particular NOT reachable
/// by pointing the child's `current_dir` at a non-git tempdir:
/// `run_orphan_audit` resolves `repo_root` at compile time from
/// `env!("CARGO_MANIFEST_DIR")`, so the child's own cwd never enters it.
#[test]
fn replay_child_hard_fails_only_when_the_parent_verified_an_envelope() {
    // Same refusal, and for the same reason, as the sibling fixture below: this
    // test spawns children of its own, so widening the replay filter to select
    // it must fail on the precondition rather than quietly nest a generation.
    common::git_env::assert_not_in_replay_child(
        "replay_child_hard_fails_only_when_the_parent_verified_an_envelope",
        "it would spawn grandchildren inside an already-nested child for no \
         signal the outer run does not have",
    );

    // The wording `run_orphan_audit`'s prerequisite-probe skip notes share, so
    // reordering those probes among themselves (an empty PATH hides `git` too)
    // moves this test onto another of them instead of reddening it. A copy of a
    // string living in another crate's `eprintln!`s: a reword there reddens
    // both halves below rather than silently un-attributing them. The
    // single-source fix — a `pub const` in `reify-test-support` that its
    // `eprintln!`s and this test both read — needs an edit outside this task's
    // lock set and is filed as #7070.
    const SKIP_MARKER: &str = "skipping orphan audit";

    // Literally the same filter the replay harness uses — the module-level
    // const, not a second spelling of it — so this fixture cannot pin the
    // behaviour of a test the real replay no longer selects.
    const TARGET: [&str; 1] = [TARGET_TEST];

    // --- Half A: an unverified mark must leave the graceful skip intact ---
    let plain = spawn_child_lacking_audit_prereqs(&TARGET, ReplayMark::Plain);
    let plain_stdout = String::from_utf8_lossy(&plain.stdout);
    let plain_stderr = String::from_utf8_lossy(&plain.stderr);

    assert!(
        plain_stderr.contains(SKIP_MARKER),
        "the deprived child did not report {SKIP_MARKER:?}, so this half is not \
         exercising an environment that cannot run the audit: either its empty \
         PATH no longer reaches any of `run_orphan_audit`'s prerequisite \
         probes, or the wording they share changed (update SKIP_MARKER).\n\
         --- child stderr (truncated) ---\n{:.600}",
        plain_stderr,
    );
    assert!(
        plain.status.success(),
        "a replay child stamped with {:?} FAILED (exit {:?}) in an environment \
         that simply lacks the audit's prerequisites. Nothing established that \
         this environment can run the audit, so `run_orphan_audit`'s graceful \
         skip is the contract and a tightening firing here reddens a supported \
         environment.\n\
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
    let envelope = spawn_child_lacking_audit_prereqs(&TARGET, ReplayMark::Envelope);
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
        "a replay child stamped with {:?} exited 0 despite skipping the audit — \
         exactly the condition the tightening exists to catch, since that mark \
         is stamped only after the parent SAW an envelope for this scope here. \
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
/// empty stdout BOTH when the hook environment redirects its scan into an empty
/// tree (exit 0, stderr `no source files matched`) and when it aborts before
/// scanning at all (non-zero, git's own `fatal:` line). A caller that reads only
/// `stdout` cannot tell a demonstrated hazard from a broken fixture.
struct AuditRun {
    stdout: String,
    status: ExitStatus,
    stderr: String,
}

/// Run `scripts/audit-orphan-producers.sh --scope <scope> --quiet --format
/// json` TWICE against one shared [`common::git_env::decoy_repo`]: once with
/// the hook poison ambient in the child, once with
/// [`reify_audit::git_env::sanitize`] applied — the canonical sanitizer
/// production uses, not a hand-rolled removal of the three vars poisoned here.
/// Both commands come from ONE closure against ONE decoy, so the environment is
/// provably the only delta.
///
/// Returns `None`, with an explanatory `stderr` note, exactly when
/// `reify_test_support::run_orphan_audit` declines an envelope for `scope`.
/// That one call IS the graceful-skip protocol — `python3`/`git` presence,
/// script-on-disk, `repo_root`-is-a-git-work-tree, `EXCLUDE_CRATES` membership
/// — and delegating inherits its LOUD half too, where a `git rev-parse` failing
/// for a reason OTHER than "no repository here" panics rather than skips.
/// Skipping rather than comparing is the only honest answer: every cause of
/// that `None` empties BOTH halves, so a caller comparing them would fail
/// "sanitized is non-empty" while passing "poisoned is empty". The cost is a
/// THIRD script run whose envelope is discarded.
///
/// Asserts nothing: it reports stdout, status and stderr and leaves every
/// judgement to the caller. Refuses to run inside a replay child, where all
/// three runs would repeat for no signal the outer run does not have.
fn audit_script_stdout_poisoned_and_sanitized(scope: &str) -> Option<(AuditRun, AuditRun)> {
    common::git_env::assert_not_in_replay_child(
        "audit_script_stdout_poisoned_and_sanitized",
        "it would re-run the audit script three more times inside an \
         already-nested child, for no signal the outer run does not have",
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
    // TODO(#6153): delete this walk and the argv below in favour of a public
    // seam on `reify_test_support::orphan_audit`, and drop the two premise
    // checks that exist only to bound them. Both the walk and the argv are a
    // second copy of module-private code in that crate — outside the lock set
    // of the task that owns this file, which is why the copy is here at all.
    // (This crate is on the ptodo detector's own allowlist, so this cite
    // documents rather than enrols; the task is the record either way.)
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

    // Premise checks on the walk directly above — NOT a second copy of the skip
    // protocol. The gate already ran the script to completion, so anything
    // wrong here is a bug in this helper rather than an environmental
    // condition, and both fail loudly instead of skipping.
    assert!(
        script.exists(),
        "reify_test_support::run_orphan_audit({scope:?}) just ran the audit script \
         successfully, but this crate's own CARGO_MANIFEST_DIR walk resolves it to \
         {script:?}, where nothing exists — the two `.parent()` walks disagree, so \
         this helper would spawn a different script (or none) than the one the skip \
         protocol vetted"
    );

    // Two walks resolving to existing but DIFFERENT roots — a nested checkout,
    // a vendored copy, either crate moved out of `crates/` — pass the check
    // above silently while spawning a script the gate never vetted. Since
    // `reify_test_support`'s own walk is two `.parent()`s off ITS manifest dir,
    // finding that crate here proves the other walk lands back on this root.
    // Bounded: it does not distinguish this repo from a byte-identical vendored
    // copy laid out the same way. Closing that needs the path itself rather
    // than a reconstruction of it — the same seam #6153 tracks above.
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
/// `orphan_audit_survives_ambient_hook_git_env` guards the PRODUCTION call
/// site, but no clean checkout can watch it go RED (see the module doc), and a
/// guard whose teeth can never be demonstrated decays into one nobody trusts.
/// This test re-demonstrates the hazard's potency synthetically, with no
/// dependency on the production call site at all.
///
/// It runs `audit-orphan-producers.sh --scope <SCOPE> --quiet --format json`
/// two ways against the `git init`ed tempdir [`common::git_env::decoy_repo`]
/// builds. There is no third, clean run: the sanitized half IS the clean
/// baseline, since stripping the poison is what restores the unpoisoned
/// environment.
///
/// - poison pointed at the decoy: exit 0, empty stdout, stderr
///   `audit-orphan-producers.sh: no source files matched`.
/// - the same poison, then [`reify_audit::git_env::sanitize`]d: exit 0, a JSON
///   envelope with a numeric `orphan_count`.
///
/// The mechanism is the script's `REPO_ROOT="$(git rev-parse --show-toplevel)"`
/// followed by `cd "$REPO_ROOT"` (cited by content, not line number — that line
/// has already moved once). An ambient `GIT_DIR`/`GIT_WORK_TREE` overrides both
/// the cwd and any `-C`, so the whole scan is redirected into the empty decoy
/// tree, matches no source files, and emits nothing.
///
/// BOTH halves are hard assertions: softening the poisoned half to an
/// `eprintln!` would spare a script that hardened itself out of the hazard, but
/// libtest swallows stderr on a passing test, so a harness regression that made
/// the two halves identical would still report PASS. If the script really does
/// harden itself, retire this test deliberately together with what it guards.
///
/// The sanitized half pins only "non-empty and parses as an envelope with a
/// numeric `orphan_count`" — never a byte or scanned-fn count, which track this
/// crate's incidental corpus size and would make any unrelated `pub fn`
/// addition fail here.
#[test]
fn hook_git_env_defeats_the_audit_script_and_stripping_it_cures_the_defeat() {
    let Some((poisoned, sanitized)) = audit_script_stdout_poisoned_and_sanitized(SCOPE) else {
        // `run_orphan_audit`'s own graceful-skip protocol, verbatim — and each
        // of its causes empties BOTH halves, so the comparison below would
        // prove nothing.
        return;
    };

    // `{:.400}` is a Display precision, i.e. a truncating max width: enough of
    // the offending output to diagnose a failure without dumping ~9 KiB.
    //
    // The sanitized half is asserted FIRST because it decides which diagnosis a
    // reader meets first, and "the script cannot produce output even
    // unpoisoned" is the more fundamental failure: it explains an empty
    // poisoned half too, whereas the reverse is not true.
    assert!(
        !sanitized.stdout.trim().is_empty(),
        "stripping GIT_DIR/GIT_WORK_TREE/GIT_INDEX_FILE did NOT restore the \
         audit script's output — it emitted nothing (exit {:?}). The \
         environment is already vetted: `run_orphan_audit` ran this same script \
         against this same scope moments ago and got an envelope, or this test \
         would have skipped. So the difference is something this test's own two \
         spawns introduce — a mis-sanitized command, or a `.parent()` walk \
         resolving a different script than the gate vetted.\n\
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

    // Status and stderr are asserted BEFORE the emptiness they explain. Empty
    // stdout is produced both by the hazard (scan redirected into the empty
    // decoy, script runs to completion, exit 0) and by the script dying before
    // it scanned anything (exit non-zero), so the emptiness check alone would
    // report a broken fixture as a green demonstration of the hazard.
    assert!(
        poisoned.status.success(),
        "the audit script did not RUN under the hook git environment — it \
         exited {:?} rather than 0. The claim here is that the poison redirects \
         the scan into an empty decoy tree the script then reports on normally, \
         NOT that the poison kills it. A non-zero exit means the decoy is \
         malformed rather than merely empty (a tempdir cleaned early, a `git \
         init` that left no usable object store), so the script aborted before \
         scanning. Fix the fixture — relaxing this assertion makes the empty \
         stdout below stop meaning anything.\n\
         --- poisoned stderr (truncated) ---\n{:.400}",
        poisoned.status.code(),
        poisoned.stderr,
    );

    // The script's OWN marker, not git's `fatal:` wording: this string lives in
    // `scripts/audit-orphan-producers.sh`, so keying on it couples this test to
    // this repo rather than to git's diagnostics.
    const NO_SOURCES_MARKER: &str = "audit-orphan-producers.sh: no source files matched";
    assert!(
        poisoned.stderr.contains(NO_SOURCES_MARKER),
        "the audit script exited 0 under the hook git environment but did not \
         report {NO_SOURCES_MARKER:?}, so it did not take the \
         scanned-an-empty-tree path the poison is supposed to force and the \
         empty stdout below has some other cause. Either the script's no-match \
         reporting changed (update this marker) or the poison is no longer \
         redirecting the scan (see the next assertion's two causes).\n\
         --- poisoned stderr (truncated) ---\n{:.400}",
        poisoned.stderr,
    );

    assert!(
        poisoned.stdout.trim().is_empty(),
        "the hook git environment no longer defeats the audit script: with \
         GIT_DIR/GIT_WORK_TREE/GIT_INDEX_FILE pointed at an empty decoy repo, the \
         script still emitted {} byte(s) on stdout (exit {:?}). Two causes needing \
         opposite responses. (1) This harness regressed and the poison never \
         reached the child — check that `poison_with_hook_git_env` still applies \
         all three vars to the POISONED command and that `decoy_repo` still yields \
         an empty tree; fix it. (2) The script hardened itself, e.g. it no longer \
         resolves its repo root via `git rev-parse --show-toplevel`; then the \
         hazard is dead and the remedy is to retire this whole trio (module doc) \
         together with reify-test-support's `sanitize()`, never to weaken this \
         assertion into a log line no passing run shows.\n\
         --- poisoned stdout (truncated) ---\n{:.400}\n\
         --- poisoned stderr (truncated) ---\n{:.400}",
        poisoned.stdout.len(),
        poisoned.status.code(),
        poisoned.stdout,
        poisoned.stderr,
    );
}
