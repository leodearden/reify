#!/usr/bin/env bash
# tests/infra/test_reify_audit_pdiag.sh
#
# Infra hard gate for the PDIAG detector (task #5405, INV-SF-6 "codes are
# mandatory on emitted diagnostics"):
#
#   (a) RATCHET    — `reify-audit --pattern PDIAG --project-root $REPO_ROOT`
#                    exits 0 against the committed
#                    crates/reify-audit/pdiag-baseline.txt.  This is the live
#                    ratchet: any file whose code-less
#                    Diagnostic::error/warning count rose above its baseline
#                    row (or that is new to the baseline entirely) is a High
#                    finding, and reify-audit's exit code IS the High count.
#
#   (b) HARD GATE  — a hermetic git-init'd fixture repo carrying ONE code-less
#                    diagnostic constructor at a swept path, with NO baseline
#                    manifest, exits NON-ZERO and its stderr names
#                    docs/notes/diagnostic-severity-policy.md.  This is the
#                    fail-LOUD direction: an absent manifest is an EMPTY
#                    baseline, never "nothing to check" (pdiag.rs::check).
#
#   (c) ESCAPE     — the SAME fixture with a trailing reviewed opt-out comment
#                    appended to the anchor line exits 0.  (b) and (c) differ
#                    by exactly that one token, so the pair pins the escape
#                    hatch as the thing doing the suppressing rather than some
#                    incidental property of the fixture.
#
# Design invariant (PRD §6.6): the per-file code-less census lives ONLY in
# pdiag::live_counts, which BOTH the ratchet (pdiag::check) and the generator
# (src/bin/pdiag-baseline-gen.rs) call.  No count is re-derived in this bash
# file — every scenario asserts on the binary's EXIT CODE, which is the same
# High-severity count the merge gate consumes.
#
# Budget-safe partition (mirrors test_reify_audit_ptodo.sh, incident
# 2026-06-22/23): scenario (a) is precision-sensitive — it compares a LIVE scan
# against a manifest committed alongside the current scanner, so a stale binary
# can disagree for reasons that are not a real ratchet regression.  It is
# therefore wrapped in the RATCHET_SKIP guard.  Scenarios (b)+(c) are STABLE
# across the warm-lane staleness window (a present-but-stale binary still emits
# a High for a code-less site and still honours the escape), so they run
# whenever REIFY_AUDIT_BIN is executable, regardless of RATCHET_SKIP.  The rc-75
# skip must never take the whole file down — that is precisely the bug that let
# the PTODO hard gate be silently bypassed.
#
# NO-SILENT-GREEN FLOOR (esc-5405-7, corrected esc-5405-9).  The rc-75
# partition above is only sound while the binary is PRESENT-but-stale.  The
# same guard also returns 75 for an ABSENT binary, and returns 125 when the
# rebuild path ran and the binary is STILL judged stale.  Neither rc implies
# the binary is unusable: 125 in particular covers a FAILED rebuild that left
# an older $REIFY_AUDIT_BIN fully executable, or an override $REIFY_AUDIT_BIN
# that is not cargo's own artifact (see the `return 125` site in
# scripts/reify-audit-freshness.sh; a SUCCESSFUL no-op build of cargo's own
# artifact is fresh there since #7691).  So BOTH rcs are split on detector
# USABILITY, never on the rc alone:
#   PRESENT-but-stale → RATCHET_SKIP=1.  Only the precision-sensitive (a) is
#     skipped; the staleness-stable (b)+(c) hard gate still runs, so the run
#     does assert something.
#   ABSENT → exit 1.  Every scenario is guarded on `[ -x "$REIFY_AUDIT_BIN" ]`,
#     so NONE execute, and an unguarded `test_summary` would print
#     "0 passed, 0 failed" and exit 0.  run_all.sh grades on exit code alone,
#     so that is a hard gate reporting green having asserted nothing — the
#     exact failure mode this file's partition was written to prevent.
# The $RAN tracker is the backstop under both: it refuses to exit 0 unless at
# least one scenario actually executed.
#
# SELF-MATCH SAFETY: this file must not contain a literal `Diagnostic::error(`
# anchor or a literal opt-out token.  Both are assembled from shell variables at
# runtime, so the written fixture carries the real tokens while this .sh source
# stays clean.  PDIAG only sweeps `crates/*/src/**.rs` + `gui/src-tauri/src/**.rs`
# (pdiag.rs::is_swept_path), so a .sh file is out of scope by construction —
# the discipline is kept anyway, exactly as test_reify_audit_ptodo.sh keeps it,
# because a detector whose own gate seeds its corpus is one scope change away
# from ratcheting against itself.
#
# No sqlite3 is used here: PDIAG is a purely STRUCTURAL lane (git ls-files plus
# working-tree reads, no task DB, no jcodemunch), so the esc-4581-87
# LD_LIBRARY_PATH="" sqlite3 routing that scenarios (d)/(e) of the PTODO gate
# need has no analogue.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob; bucketed in
# tests/infra/run-all-classification.manifest as intra-run-serial (the freshness
# guard may fork cargo and mutate the lane-shared CoW target/).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

# Graceful skip when required tools are absent.
for _tool in git cargo; do
    if ! command -v "$_tool" >/dev/null 2>&1; then
        echo "test_reify_audit_pdiag.sh: $_tool not on PATH — skipping" >&2
        exit 0
    fi
done

echo "=== PDIAG detector infra gate ==="

# The remediation doc every High PDIAG summary cites.  Held once here because
# three consumers must agree on the string: pdiag.rs::SEVERITY_POLICY_DOC, the
# doc itself, and this grep.
POLICY_DOC="docs/notes/diagnostic-severity-policy.md"
BASELINE="$REPO_ROOT/crates/reify-audit/pdiag-baseline.txt"

# -----------------------------------------------------------------------
# Actionability helpers (the 4636 lesson, esc-4959-57): a failing assert must
# land the offending detector output in assert()'s on-FAIL captured-output
# dump, co-located with the failing assertion.  Both helpers stay
# byte-for-byte SILENT and return 0 on the passing path, so an all-green run
# is unchanged.
# -----------------------------------------------------------------------

# Scenario (a): non-zero exit against the committed baseline is a ratchet
# regression.  Name the exit code and replay the detector's own stderr (the
# JSON findings), which already carries the offending path and both
# remediations.
_ratchet_exit_ok() {
    local _rc="$1" _errfile="$2"
    if [ "$_rc" -ne 0 ]; then
        printf 'PDIAG RATCHET REGRESSION — reify-audit --pattern PDIAG exited %s against the committed baseline.\n' \
            "$_rc" >&2
        printf 'Remediate per %s: attach a DiagnosticCode, take the reviewed opt-out, or — only when a\n' \
            "$POLICY_DOC" >&2
        printf 'file genuinely went DOWN — regenerate with `cargo run -p reify-audit --bin pdiag-baseline-gen`.\n' >&2
        if [ -s "$_errfile" ]; then
            printf -- '---- reify-audit stderr (tail -40) ----\n' >&2
            tail -n 40 "$_errfile" >&2
        fi
        return 1
    fi
    return 0
}

# Scenario (b): the hard gate is only useful if the reader is routed somewhere.
# A finding that fires but cites nothing is a dead end.
_stderr_cites_policy_doc() {
    local _errfile="$1"
    if grep -qF "$POLICY_DOC" "$_errfile"; then
        return 0
    fi
    printf 'PDIAG hard-gate finding did NOT cite %s — the remediation is unreachable from the failure.\n' \
        "$POLICY_DOC" >&2
    printf -- '---- reify-audit stderr (tail -40) ----\n' >&2
    tail -n 40 "$_errfile" >&2
    return 1
}

# -----------------------------------------------------------------------
# Resolve the reify-audit binary and ride the shared freshness guard.
#
# Testability seam (task #4624 precedent): REIFY_AUDIT_BIN can be overridden by
# environment for hermetic meta-tests that exercise the budget-safe skip path
# without a real binary on disk.
# -----------------------------------------------------------------------
REIFY_AUDIT_BIN="${REIFY_AUDIT_BIN:-$REPO_ROOT/target/release/reify-audit}"

source "$REPO_ROOT/scripts/reify-audit-freshness.sh"

# rebuild-budget-safe: under REIFY_AUDIT_NO_COLD_BUILD=1 an absent/stale binary
# yields 75 (EX_TEMPFAIL) instead of a cargo build inside run_all.sh's wall.
# Map 75 → RATCHET_SKIP=1, NOT exit 0 — (b)+(c) must still run.
RATCHET_SKIP=0

# Did ANY scenario actually execute?  Consulted after test_summary; a run that
# asserted nothing must not exit 0.  See NO-SILENT-GREEN FLOOR in the header.
RAN=0

set +e
reify_audit_guard "$REIFY_AUDIT_BIN" rebuild-budget-safe "$REPO_ROOT" 2>&1
_guard_rc=$?
set -e

if [ "$_guard_rc" -eq 75 ]; then
    echo "test_reify_audit_pdiag.sh: reify-audit binary absent/stale and REIFY_AUDIT_NO_COLD_BUILD=1 — (a) SKIP (budget-safe)" >&2
    RATCHET_SKIP=1
elif [ "$_guard_rc" -ne 0 ]; then
    # Any other nonzero rc — 125 from reify_audit_guard means the binary is
    # STILL judged stale after the rebuild path ran.  That covers two very
    # different worlds: a failed `cargo build -p reify-audit` with no usable
    # detector at all, and a present binary the guard could not vouch for — a
    # failed build that left an older binary behind, or an override
    # REIFY_AUDIT_BIN that is not cargo's own artifact.
    #
    # So split on detector USABILITY, exactly as the rc-75 partition above
    # does and exactly as tests/infra/test_reify_audit_ptodo.sh does (#5962
    # review, esc-5405-9).  Collapsing both worlds into one unconditional
    # `exit 1` turns every such run into a spurious hard RED
    # while emitting a diagnostic ("could not be made usable") that is
    # factually wrong about an executable binary.
    if [ -x "$REIFY_AUDIT_BIN" ]; then
        # PRESENT: the detector runs, so the staleness-stable (b)+(c) hard gate
        # must still execute.  Only the precision-sensitive ratchet (a) is
        # skipped.  This is not a silent green: (b)+(c) set $RAN, and the floor
        # after test_summary still refuses a run that executed none of them.
        echo "test_reify_audit_pdiag.sh: reify-audit freshness guard failed (rc=$_guard_rc) but '$REIFY_AUDIT_BIN' is executable — skipping the precision-sensitive ratchet (a); the (b)+(c) hard gate still runs against the stale binary" >&2
        RATCHET_SKIP=1
    else
        # ABSENT: nothing can run.  Leaving RATCHET_SKIP=0 here would look like
        # "ratchet enabled" while the `-x` guards below silently skip every
        # scenario.  That is not a budget-safe skip; it is a broken toolchain,
        # and it must be loud.
        echo "test_reify_audit_pdiag.sh: reify-audit freshness guard failed (rc=$_guard_rc) — the detector could not be made usable and no budget-safe skip was requested; refusing to report green" >&2
        exit 1
    fi
fi

# -----------------------------------------------------------------------
# Exactly ONE EXIT trap covers every temp path.  Registering a second `trap
# ... EXIT` would silently REPLACE this one and leak the earlier temps.
# -----------------------------------------------------------------------
AUX=""       # tasks-file + runs-db holder, never inside a fixture repo
FIX_B=""     # scenario (b): code-less anchor, no baseline
FIX_C=""     # scenario (c): the same fixture plus the reviewed opt-out
_err_tmp=""  # stderr capture for run_audit
cleanup_all() {
    # `|| true` on each line: `[ -n "" ] && rm` short-circuits to rc 1, which
    # would become the trap's exit code and override the script's real status.
    [ -n "$AUX"      ] && rm -rf "$AUX"      || true
    [ -n "$FIX_B"    ] && rm -rf "$FIX_B"    || true
    [ -n "$FIX_C"    ] && rm -rf "$FIX_C"    || true
    [ -n "$_err_tmp" ] && rm -f  "$_err_tmp" || true
}
trap cleanup_all EXIT

# -----------------------------------------------------------------------
# run_audit — retry + visibility wrapper (task #4800 precedent).
#
#   (i)  Captures stderr to $_err_tmp so the detector's JSON findings and any
#        "git ls-files failed" breadcrumb are inspectable by the scenarios and
#        replayable into a failing assert's dump — closing the 2>/dev/null
#        blind spot that made transient failures invisible.
#
#   (ii) Retries up to 3 times on the transient-infra codes 125 (IO-misconfig /
#        sqlite EMFILE), 101 (Rust panic) and 134/137/139 (SIGABRT/SIGKILL/
#        SIGSEGV).  Safe here because every PDIAG scenario expects a High count
#        of 0 or 1 — far below 101/125 — so "infra failure" and "legitimate
#        finding count" cannot collide.
#
#   rc in {0,1} is AUTHORITATIVE: accepted immediately, never retried, so a
#   genuine 0-vs-1 mismatch still goes RED.  No assertion is weakened.
#
# Every scenario goes through this wrapper — deliberately NOT inheriting the
# PTODO gate's scenario (e), which calls the binary directly and thereby loses
# both the retry and the stderr capture.
#
# Usage: run_audit [reify-audit-args...]; callers capture rc with _x=$?.
# -----------------------------------------------------------------------
_err_tmp="$(mktemp)"
run_audit() {
    local _attempt rc=0 _retried=0
    for _attempt in 1 2 3; do
        rc=0
        env -u REIFY_PTODO_TASKS_DB \
            "$REIFY_AUDIT_BIN" "$@" >/dev/null 2>"$_err_tmp" || rc=$?
        if [ "$rc" -le 1 ]; then
            break
        fi
        case "$rc" in
            125|101|134|137|139)
                _retried=1
                if [ "$_attempt" -lt 3 ]; then
                    sleep 2
                fi
                ;;
            *)
                # Non-infra exit code — authoritative, do not retry.
                break
                ;;
        esac
    done
    # Surface captured stderr whenever a retry occurred: the retry itself is
    # the signal worth logging, regardless of the final rc.
    if [ "$_retried" -eq 1 ]; then
        echo "run_audit: transient infra retry occurred (rc=$rc); stderr:" >&2
        cat "$_err_tmp" >&2
    fi
    return "$rc"
}

# -----------------------------------------------------------------------
# Shared aux inputs, kept OUTSIDE every fixture repo so they are never
# git-tracked and never enumerated by the sweep.
#
#   --tasks-file []  bypasses the MCP task loader entirely; without it the
#                    binary tries to reach fused-memory and may exit 125 on
#                    EMFILE rather than reporting a finding count.
#   --runs-db <0-byte file>  is an acceptable runs DB for a structural pattern:
#                    the CLI opens it, but the PDIAG lane never reads ctx.conn.
# -----------------------------------------------------------------------
AUX="$(mktemp -d)"
AUX_TASKS="$AUX/tasks.json"
AUX_RUNS="$AUX/runs.db"
printf '[]' > "$AUX_TASKS"
: > "$AUX_RUNS"

# -----------------------------------------------------------------------
# Fixture builder shared by (b) and (c).
#
# `$2` is appended verbatim to the anchor line, so (b) passes "" and (c) passes
# the assembled opt-out — the ONLY difference between the two trees.
#
# The path `crates/reify-eval/src/hermetic.rs` is chosen to satisfy
# is_swept_path: a `.rs` file whose third path segment is exactly `src`, with no
# `tests` segment, no `tests.rs`/`*_tests.rs` stem, and outside
# SCOPE_EXCLUDE_PREFIXES.
#
# `git add -A` without a commit is sufficient: RealGitOps enumerates the INDEX
# via `git ls-files`, so no committer identity is needed (same shape as the
# PTODO gate's fixtures).
# -----------------------------------------------------------------------
_make_pdiag_fixture() {
    local _dir="$1" _suffix="$2"
    git -C "$_dir" init -q
    mkdir -p "$_dir/crates/reify-eval/src"
    {
        printf 'pub fn emit(out: &mut Vec<Diag>) {\n'
        printf '    out.push(%s::%s("shell extraction failed".to_string()));%s\n' \
            "$CTOR_TYPE" "$CTOR_FN" "$_suffix"
        printf '}\n'
    } > "$_dir/crates/reify-eval/src/hermetic.rs"
    git -C "$_dir" add -A
}

# Assemble the swept anchor and the reviewed opt-out at runtime so this source
# never contains a literal form (SELF-MATCH SAFETY).
CTOR_TYPE="Diagnostic"
CTOR_FN="error"
ALLOW_KEY="pdiag"
ALLOW_VAL="allow"
ALLOW_COMMENT=" // ${ALLOW_KEY}:${ALLOW_VAL} — hermetic fixture, reviewed opt-out"

# -----------------------------------------------------------------------
# (a) RATCHET: the live tree must be within the committed baseline.
#     Gen-driven precision — wrapped in the RATCHET_SKIP guard.
# -----------------------------------------------------------------------
if [ "${RATCHET_SKIP}" = "0" ] && [ -x "$REIFY_AUDIT_BIN" ]; then
    echo ""
    echo "--- (a) Ratchet: live tree within committed pdiag-baseline.txt ---"

    RAN=1
    _fail_before_a=$FAIL

    # Precondition: an ABSENT manifest would still exit non-zero (empty
    # baseline → every code-less file a NewFile High), so (a) would go red for
    # the right reason but the wrong stated cause.  Assert the file first so the
    # message is unambiguous.
    assert "(a) precondition: crates/reify-audit/pdiag-baseline.txt exists" \
        test -f "$BASELINE"

    set +e
    run_audit \
        --pattern PDIAG \
        --project-root "$REPO_ROOT" \
        --runs-db "$AUX_RUNS" \
        --tasks-file "$AUX_TASKS" \
        --no-jcodemunch
    _exit_ratchet=$?
    set -e

    assert "(a) live tree is within the committed baseline (no ratchet regression)" \
        _ratchet_exit_ok "$_exit_ratchet" "$_err_tmp"

    # Sentinel emitted ONLY when every (a) assert passed — a broken gate
    # suppresses it, so a meta-test grepping for the token stays RED rather
    # than silently passing.  The token carries no swept substring.
    [ "$FAIL" -eq "$_fail_before_a" ] && echo "@@PDIAG_HARDGATE_A_PASSED@@"
fi

# -----------------------------------------------------------------------
# (b)+(c) HARD GATE and ESCAPE — hermetic, staleness-stable.  Run whenever the
#     binary is PRESENT, independent of RATCHET_SKIP.
# -----------------------------------------------------------------------
if [ -x "$REIFY_AUDIT_BIN" ]; then
    echo ""
    echo "--- (b) Hard gate: code-less diagnostic at a swept path → High → non-zero exit ---"

    RAN=1
    FIX_B="$(mktemp -d)"
    _make_pdiag_fixture "$FIX_B" ""

    _fail_before_b=$FAIL

    # Guard the fixture itself: if the file were not tracked, the sweep would
    # legitimately find nothing and (b) would report a product regression for
    # what is actually an infra failure.
    assert "(b) precondition: crates/reify-eval/src/hermetic.rs is git-tracked" \
        git -C "$FIX_B" ls-files --error-unmatch crates/reify-eval/src/hermetic.rs

    # No pdiag-baseline.txt in the fixture → EMPTY baseline → the single
    # code-less file is a NewFile High → exit 1.  Asserting the EXACT code
    # separates "gate fired" from "binary errored" (125 = IO/arg misconfig,
    # 101 = Rust panic).
    set +e
    run_audit \
        --pattern PDIAG \
        --project-root "$FIX_B" \
        --runs-db "$AUX_RUNS" \
        --tasks-file "$AUX_TASKS" \
        --no-jcodemunch
    _exit_codeless=$?
    set -e

    assert "(b) code-less diagnostic + absent baseline → reify-audit exits 1 (exactly 1 High)" \
        bash -c '[ "$1" -eq 1 ]' -- "$_exit_codeless"

    # Assert on the CAPTURED stderr immediately: the next run_audit call
    # overwrites $_err_tmp.
    assert "(b) hard-gate finding cites $POLICY_DOC" \
        _stderr_cites_policy_doc "$_err_tmp"

    [ "$FAIL" -eq "$_fail_before_b" ] && echo "@@PDIAG_HARDGATE_B_PASSED@@"

    echo ""
    echo "--- (c) Escape: the reviewed opt-out suppresses the same site → exit 0 ---"

    FIX_C="$(mktemp -d)"
    _make_pdiag_fixture "$FIX_C" "$ALLOW_COMMENT"

    _fail_before_c=$FAIL

    set +e
    run_audit \
        --pattern PDIAG \
        --project-root "$FIX_C" \
        --runs-db "$AUX_RUNS" \
        --tasks-file "$AUX_TASKS" \
        --no-jcodemunch
    _exit_escaped=$?
    set -e

    assert "(c) reviewed opt-out on the anchor line → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_escaped"

    [ "$FAIL" -eq "$_fail_before_c" ] && echo "@@PDIAG_HARDGATE_C_PASSED@@"
else
    echo ""
    echo "test_reify_audit_pdiag.sh: reify-audit binary absent at '$REIFY_AUDIT_BIN' — (b)+(c) hard gate could not run" >&2
fi

# -----------------------------------------------------------------------
# Summary
#
# test_summary exits 1 when FAIL > 0, so control only reaches the $RAN floor
# on the otherwise-all-green path — which is exactly where a zero-assertion
# run would have been laundered into a passing hard gate.
# -----------------------------------------------------------------------
test_summary

if [ "$RAN" -eq 0 ]; then
    echo "test_reify_audit_pdiag.sh: NO scenario executed (REIFY_AUDIT_BIN='$REIFY_AUDIT_BIN' not executable; RATCHET_SKIP=$RATCHET_SKIP, guard rc=$_guard_rc) — refusing to report green for a hard gate that asserted nothing" >&2
    exit 1
fi
