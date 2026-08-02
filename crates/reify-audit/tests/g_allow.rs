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
    if audit.is_none() && common::git_env::in_replay_child() {
        panic!(
            "run_orphan_audit returned None inside the poisoned replay child. \
             The audit script resolves its repo root with `git rev-parse \
             --show-toplevel` (audit-orphan-producers.sh line 66), so the \
             ambient GIT_DIR/GIT_WORK_TREE/GIT_INDEX_FILE this child was \
             spawned with redirected the entire scan into the harness's empty \
             decoy tree; it matched no source files, emitted empty stdout, and \
             run_orphan_audit turned that into None. This is exactly the \
             regression the `.env_remove()` calls in \
             crates/reify-test-support/src/orphan_audit.rs (task 5605) exist \
             to prevent — check that `sanitize()` is still called on the \
             command `build_audit_command` returns. Note that `python3 / git / \
             script absent` is NOT a plausible explanation here: the parent \
             process just ran this same test successfully before spawning this \
             child."
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
/// - Unsanitized spawn -> the script's `git rev-parse --show-toplevel` resolves
///   into the harness's empty decoy tree -> no source files matched -> empty
///   stdout -> `run_orphan_audit` returns `None` -> the replay-child-only
///   `panic!` fires -> the child exits non-zero ->
///   `replay_self_under_hook_git_env`'s status assertion fails -> RED.
/// - Sanitized spawn -> the real repo root -> a JSON envelope ->
///   `orphan_count == 0` -> the child is green -> GREEN.
///
/// That `panic!` is the entire reason this test has teeth. Without it `None`
/// takes the graceful-skip `return`, which libtest counts as PASSED — and both
/// of the replay harness's non-vacuity guards count a self-skip in `passed`,
/// so NO value of `expected_min` could make the broken case RED. The skip
/// stays the behaviour everywhere except inside the replay child, because
/// `run_orphan_audit`'s skip protocol is a contract with eight callers across
/// three crates covering environments where `python3`, `git` or the script is
/// genuinely absent.
///
/// # The RED observation, recorded because it is no longer reproducible
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
/// because task 5605's `.env_remove()` calls landed. From a clean checkout
/// there is now no way to watch it fail without deleting one of those lines,
/// which is why `hook_git_env_defeats_the_audit_script_and_stripping_it_cures_the_defeat`
/// exists — it pins the same hazard's potency synthetically, so this test's
/// teeth stay demonstrable even though its RED no longer is.
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
/// # Measured (HEAD b2a758678a5409099ffd108459eb97a430c5c41b)
///
/// `audit-orphan-producers.sh --scope crates/reify-audit/src --quiet --format
/// json`, three ways, against a `git init`ed tempdir carrying a planted
/// `.git/index.lock` — exactly what [`common::git_env::decoy_repo`] builds:
///
/// - clean: exit 0, stdout 7417 bytes, `total_pub_fns_scanned` 46,
///   `orphan_count` 0.
/// - `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE` pointed at the decoy: exit 0,
///   stdout **0 bytes**, stderr `audit-orphan-producers.sh: no source files
///   matched`.
/// - the same poison, then those three vars `env_remove`d: exit 0, stdout 7417
///   bytes, `orphan_count` 0.
///
/// The mechanism is `audit-orphan-producers.sh` line 66 —
/// `REPO_ROOT="$(git rev-parse --show-toplevel)"` followed by
/// `cd "$REPO_ROOT"`. An ambient `GIT_DIR`/`GIT_WORK_TREE` overrides both the
/// cwd and any `-C`, so the whole scan is redirected into the empty decoy
/// tree, matches no source files, and emits nothing. Note that exit status is
/// 0 in every one of the three runs: the exit code carries no signal here,
/// which is why the assertions below read stdout.
///
/// Those assertions deliberately pin only "empty" vs "non-empty and parses as
/// an envelope". The 7417 bytes and the 46 scanned fns are today's incidental
/// corpus size for this crate; pinning them would make an unrelated `pub fn`
/// addition fail this test, and that churn is how a signal gets weakened or
/// deleted.
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
        // python3 / git / the script itself is absent — same graceful-skip
        // protocol every `run_orphan_audit` caller follows.
        return;
    };

    // `{:.400}` is a Display precision, i.e. a truncating max width: enough of
    // the offending stdout to diagnose a failure without dumping 7 KiB.
    assert!(
        poisoned.trim().is_empty(),
        "the hook git environment no longer defeats the audit script: with \
         GIT_DIR/GIT_WORK_TREE/GIT_INDEX_FILE pointed at an empty decoy repo, \
         the script still emitted {} byte(s) on stdout. Either the script \
         stopped resolving its repo root through `git rev-parse \
         --show-toplevel`, or the decoy stopped being empty. If the hazard is \
         genuinely gone, `reify_audit_pub_fns_are_g_allow_marked`'s \
         replay-child panic and reify-test-support's `sanitize()` are both \
         dead weight and should be retired together — do not just delete this \
         assertion.\n--- poisoned stdout (truncated) ---\n{:.400}",
        poisoned.len(),
        poisoned,
    );

    assert!(
        !sanitized.trim().is_empty(),
        "stripping GIT_DIR/GIT_WORK_TREE/GIT_INDEX_FILE did NOT restore the \
         audit script's output — it emitted nothing. Stripping those vars is \
         supposed to be the whole cure, so this says the script now fails for \
         some other reason (a broken --scope, a missing tool that the skip \
         probes did not catch). Diagnose it by running the script by hand \
         before touching this test.\n--- sanitized stdout (truncated) \
         ---\n{:.400}",
        sanitized,
    );

    let envelope: serde_json::Value = serde_json::from_str(&sanitized).unwrap_or_else(|e| {
        panic!(
            "stripping the hook git environment produced non-empty output that \
             is not valid JSON: {e}\n--- sanitized stdout (truncated) \
             ---\n{sanitized:.400}"
        )
    });

    assert!(
        envelope["orphan_count"].as_u64().is_some(),
        "the sanitized run parsed as JSON but carries no numeric \
         `orphan_count`, so it is not the audit envelope this test claims \
         stripping restores.\n--- parsed value (truncated) ---\n{:.400}",
        envelope.to_string(),
    );
}
