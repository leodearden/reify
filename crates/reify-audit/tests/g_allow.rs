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

use reify_test_support::run_orphan_audit;

mod common;

#[test]
fn reify_audit_pub_fns_are_g_allow_marked() {
    let audit = run_orphan_audit("crates/reify-audit/src");

    // Inside the poisoned replay child ONLY, `None` is a hard failure rather
    // than a skip. Outside it, `run_orphan_audit`'s documented graceful-skip
    // protocol is untouched — see `orphan_audit_survives_ambient_hook_git_env`
    // for why the scoping is the whole point.
    //
    // As of task 5698 this branch is unreachable via the poisoned path itself
    // for this scope: a broken sanitizer now panics earlier, inside
    // `run_orphan_audit_at` (see `orphan_audit_survives_ambient_hook_git_env`'s
    // doc for that chain), before `run_orphan_audit` gets a chance to return
    // `None`. Kept as defence-in-depth against `run_orphan_audit`'s public
    // contract, which still permits `None` via `EnvUnavailable` (missing
    // python3/git/script, or a `repo_root` outside any git work tree) — not
    // because reaching it is expected today.
    if audit.is_none() && common::git_env::in_replay_child() {
        panic!(
            "run_orphan_audit returned None inside the poisoned replay child \
             — unexpected as of task 5698 (see the comment above this check). \
             `python3`/`git`/script absent is implausible regardless: the \
             parent process just ran this same test successfully before \
             spawning this child."
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
/// Both branches were walked at the commit that introduced this test, by
/// temporarily rewriting `reify_test_support`'s `sanitize()` body to drop its
/// `cmd.env_remove(var)`: the child then printed the panic above, exited 101,
/// and this test failed with the harness's status assertion. Restoring the
/// line restored GREEN. That is the check to repeat if this test is ever
/// suspected of having gone vacuous — it takes one line and one `cargo test`.
///
/// Before task 5698, that replay-child-only `panic!` in
/// `reify_audit_pub_fns_are_g_allow_marked` was the entire reason this test
/// had teeth: without it, `None` took the graceful-skip `return`, which
/// libtest counts as PASSED — and both of the replay harness's non-vacuity
/// guards count a self-skip in `passed`, so NO value of `expected_min` could
/// have made the broken case RED. Task 5698 moved the teeth: the panic that
/// actually fires today lives inside `run_orphan_audit_at` itself (the
/// repo-root mismatch above), so the replay child now fails before
/// `run_orphan_audit` even gets a chance to return `None`. The
/// replay-child-only `panic!` survives as belt-and-braces against
/// `run_orphan_audit`'s public contract, which still permits `None` — not
/// because reaching it is expected for this scope today. The graceful skip
/// stays the behaviour everywhere outside the replay child, because
/// `run_orphan_audit`'s skip protocol is a contract with nine callers across
/// two crates covering environments where `python3`, `git` or the script is
/// genuinely absent.
///
/// # The RED observation, recorded because a clean checkout no longer shows it
///
/// Measured during the esc-5656-1 / esc-5656-2 `/unblock` triage, at the
/// then-main tip 7a21980c883d9147e4126d5ace1b99df6beb0c18 (quoted here as a
/// prior measurement, not re-derived at this commit): `git grep env_remove --
/// crates/reify-test-support/src/orphan_audit.rs` returned NO match, and the
/// spawn was a bare
/// `Command::new(&script).args(...).current_dir(repo_root).output()`. Under
/// the ambient poison that script produced exit 0, 0 bytes of stdout, and
/// stderr `audit-orphan-producers.sh: no source files matched`.
///
/// So: this test would have been RED on that commit. It is GREEN today only
/// because task 5605's `.env_remove()` calls landed — and, as recorded above,
/// deleting one of them locally still turns it RED. What no longer happens is
/// a checkout that shows it RED on its own, and CI will never delete such a
/// line, which is why
/// `hook_git_env_defeats_the_audit_script_and_stripping_it_cures_the_defeat`
/// exists: it pins the same hazard's potency synthetically, on every run, with
/// no dependency on this production call site at all. Deleting either test
/// leaves the other unable to notice.
///
/// # Why a test NAME rather than the empty filter
///
/// Only one test in this binary is exposed to the hazard. An empty filter
/// would also drag the synthetic witness into the child, where it poisons and
/// strips its OWN children's environments — the ambient poison is irrelevant
/// to it, so it would ride along as pure cost and dilute the floor's meaning.
/// Naming the target keeps the selection exact: neither other test name in
/// this binary contains the substring `reify_audit_pub_fns_are_g_allow_marked`
/// (measured: `--list` with this filter names exactly that one test), so the
/// replay cannot select itself. The helper's `REIFY_AUDIT_HOOK_ENV_REPLAY`
/// guard is the second line of defence.
///
/// The floor of 1 is therefore exact rather than a lower bound. It exists
/// because libtest exits 0 on a zero-match filter: without it, renaming the
/// target test or moving it to another binary would silently downgrade this
/// harness to a vacuous pass. Raising the floor would be a claim that MORE
/// than one test here is hazard-exposed — do that only alongside widening the
/// filter to actually select them.
#[test]
fn orphan_audit_survives_ambient_hook_git_env() {
    common::git_env::replay_self_under_hook_git_env(&["reify_audit_pub_fns_are_g_allow_marked"], 1);
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
/// site at all. The pair is the point — the replay test pins that production
/// sanitizes, this test pins that sanitizing is what makes the difference.
/// Neither can silently go vacuous while the other still holds.
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
/// - `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE` pointed at the decoy: empty
///   stdout, stderr `audit-orphan-producers.sh: no source files matched`.
/// - the same poison, then [`reify_audit::git_env::sanitize`]d: a JSON
///   envelope with a numeric `orphan_count`.
///
/// Exit status is 0 in both shapes above — the exit code carries no signal
/// here, which is why the assertions below read stdout (each run's status and
/// stderr are still carried on `common::git_env::AuditRun` for a failure
/// message to report, even though neither is asserted on directly).
///
/// The mechanism is `audit-orphan-producers.sh` line 66 —
/// `REPO_ROOT="$(git rev-parse --show-toplevel)"` followed by
/// `cd "$REPO_ROOT"`. An ambient `GIT_DIR`/`GIT_WORK_TREE` overrides both the
/// cwd and any `-C`, so the whole scan is redirected into the empty decoy
/// tree, matches no source files, and emits nothing.
///
/// BOTH halves are hard assertions. The poisoned one was briefly a soft
/// `eprintln!` check, on the reasoning that a script which hardened itself out
/// of this hazard should not be punished for it — but libtest swallows stderr
/// on a passing test, so nothing observable happened, and with that half soft
/// this test's remaining assertions merely restated what
/// `reify_audit_pub_fns_are_g_allow_marked` (same binary, same scope) and
/// `sanitize_makes_dash_c_authoritative_against_real_git`
/// (reify-test-support's `git_env.rs`, same decoy-and-poison construction)
/// already pin. A dropped `cmd.env(..)` in `poison_with_hook_git_env`, a decoy
/// that stopped being empty, or a refactor applying the poison to the wrong
/// `Command` would each have made the two halves identical with this test
/// still reporting PASS. The hardening case is real but rare and one-off: the
/// sanctioned response is to retire this test deliberately, together with what
/// it guards, rather than to leave it permanently self-disabled. The failure
/// message says so.
///
/// The sanitized-half assertions deliberately pin only "non-empty and parses
/// as an envelope with a numeric `orphan_count`" — never a byte count or a
/// scanned-fn count. Those track this crate's incidental corpus size and drift
/// with any unrelated `pub fn` addition (they already have, twice, since this
/// test was written); pinning them would make such an addition fail this test,
/// and that kind of churn is exactly how a signal gets weakened or deleted by
/// a later maintainer.
///
/// This test never runs inside the poisoned replay child: that replay's filter
/// selects `reify_audit_pub_fns_are_g_allow_marked` only, and this name does
/// not contain that substring. It would prove nothing there anyway — it
/// poisons and strips its OWN children's environments, so an ambient poison is
/// irrelevant to it.
#[test]
fn hook_git_env_defeats_the_audit_script_and_stripping_it_cures_the_defeat() {
    let Some((poisoned, sanitized)) =
        common::git_env::audit_script_stdout_poisoned_and_sanitized("crates/reify-audit/src")
    else {
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
    // unchanged — but it is a HARD assertion, because it is the only thing
    // this test uniquely contributes. Everything asserted above is already
    // pinned by `reify_audit_pub_fns_are_g_allow_marked` (same binary, same
    // scope); this line is what makes the pair a demonstration of the hazard
    // rather than a second copy of that test. Left soft, a regression in the
    // harness itself — a dropped `cmd.env(..)` in `poison_with_hook_git_env`,
    // a decoy that stopped being empty, a refactor poisoning the wrong
    // `Command` — makes both halves identical and this test still reports
    // PASS, with the explanatory `eprintln!` swallowed by libtest because the
    // test passed.
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
         replay-child panic and reify-test-support's `sanitize()` — rather than to \
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
