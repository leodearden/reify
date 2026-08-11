#!/usr/bin/env bash
# tests/infra/test_reify_audit_ptodo.sh
#
# Infra gate for the PTODO detector (task e / #4557):
#   (a) RATCHET  — live ptodo-baseline-gen fingerprints must be a subset of
#                  the committed crates/reify-audit/ptodo-baseline.txt
#                  (live - baseline = empty).
#   (b) SCENARIO 13 (hermetic) — a git-tracked code file carrying a fresh
#                  untracked marker produces fingerprints absent from an empty
#                  baseline, proving the ratchet fires red on new violations.
#   (c) EXIT-CODE HARD GATE (task η, #4559) — structural untracked High
#                  finding → reify-audit exits non-zero.  Runs whenever the
#                  binary is PRESENT, independent of staleness/RATCHET_SKIP.
#   (d) ORPHANED-CITE HARD GATE (task #4733) — a cite to a done task is
#                  classified orphaned→High → reify-audit exits non-zero.
#                  Hermetic (sqlite3 seeded tasks.db + --tasks-file []).
#                  Runs whenever binary is PRESENT and sqlite3 is available.
#   (e) G-ALLOW ORPHANED HARD GATE (task #4754) — a // G-allow: owner-cite
#                  pointing to a done task is classified g-allow-orphaned→High
#                  → reify-audit exits non-zero.  Hermetic (sqlite3 seeded
#                  tasks.db + --tasks-file []).  Mirrors scenario (d) for the
#                  G-allow advisory→hard-gate flip.
#   (f) LANE δ-A HARD GATE (task #6087) — an #[allow(...dead_code...)]
#                  attribute whose trailing rationale defers the work and cites
#                  a done task is classified orphaned→High → reify-audit exits
#                  non-zero.  Hermetic, mirrors (d); adds a BENIGN control repo
#                  so the lane's false-positive guards are proven at the binary
#                  level and not only in unit tests.
#
# Design invariant (PRD 6.6): fingerprint derivation lives ONLY in the
# ptodo-baseline-gen binary (the same ptodo::fingerprint path the ratchet uses).
# No fingerprint re-derivation happens in this bash file.
#
# Budget-safe restructure (task #4733):
#   Scenarios (a)+(b) are precision-sensitive (require a FRESH gen binary to
#   emit correct fingerprints) and are wrapped in a RATCHET_SKIP guard.
#   Scenarios (c)+(d) are STABLE across the warm-lane staleness window — a
#   present-but-stale binary emits High findings correctly — so they run
#   whenever REIFY_AUDIT_BIN is executable, regardless of RATCHET_SKIP.
#   This prevents the whole-test-skip bug where a stale warm-lane binary
#   caused the PTODO hard gate to be silently bypassed (incident 2026-06-22/23).
#
# NO-SILENT-GREEN FLOOR (residual of #4733, esc-5405-7).  The rc-75 partition
# above is only sound while the binary is PRESENT-but-stale.  The same guard
# also returns 75 for an ABSENT binary, and returns 125 when a rebuild was
# attempted and the binary is still judged stale — whenever the binary is not
# executable the `[ -x "$REIFY_AUDIT_BIN" ]` guards below are false, EVERY
# scenario is skipped, and an unguarded `test_summary` would print "0 passed,
# 0 failed" and exit 0.  run_all.sh grades on exit code alone, so that is a
# hard gate reporting green having asserted nothing — the exact failure mode
# this file's partition was written to prevent.  Two floors close it:
#   * a guard rc outside {0,75} with the binary ABSENT aborts LOUD immediately
#     (a broken toolchain, not a budget-safe skip).  With the binary PRESENT
#     it degrades to RATCHET_SKIP=1 and falls through instead, so (c)+(d)+(e)
#     still run — see the rc-125 branch below for why the two cases differ
#     (#5962 review);
#   * the $RAN tracker refuses to exit 0 unless at least one scenario block
#     actually executed.
# Both are pinned by tests/infra/test_reify_audit_ptodo_budget_skip.sh.
#
# ACCEPTED UNCOVERED SKIP: the required-tool loop below still exits 0 before
# any scenario runs when git/cargo/comm/sort/sqlite3 is off PATH, and the
# floors above do not cover it.  They are scoped to detector usability; that
# loop is an environment-capability check.  All five tools are present in the
# merge gate (verify.sh), and hard-failing there would break legitimate
# dev-box `run_all.sh` runs, so it stays a graceful skip by design.
#
# SELF-MATCH SAFETY: this file must not contain any literal marker tokens that
# the PTODO structural lane sweeps for.  Marker text in scenarios (b)/(c)/(d)/(f)
# is assembled from shell variables at runtime so the written fixture carries a
# real token while this .sh source stays clean.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

# Graceful skip when required tools are absent.
for _tool in git cargo comm sort sqlite3; do
    if ! command -v "$_tool" >/dev/null 2>&1; then
        echo "test_reify_audit_ptodo.sh: $_tool not on PATH — skipping" >&2
        exit 0
    fi
done

echo "=== PTODO detector infra gate ==="

# -----------------------------------------------------------------------
# Pure ratchet-regression checker (task 5260, ITEM 3). When the live-minus-
# baseline fingerprint set ($1) is non-empty, print the offending fingerprints
# (one per line, with a count header) to stderr and return 1; when empty, stay
# byte-for-byte silent and return 0. Passed directly to assert() so a real
# ratchet regression lands the fingerprints in assert()'s on-FAIL captured-
# output dump (test_helpers.sh:24, esc-4959-57) — co-located with the failing
# assertion, exactly where the 4636 RCA found zero actionable output.
# -----------------------------------------------------------------------
_ratchet_check_subset() {
    local _new="$1"
    if [ -n "$_new" ]; then
        printf 'RATCHET REGRESSION — %s live fingerprint(s) NOT in committed baseline:\n' \
            "$(printf '%s\n' "$_new" | grep -c .)" >&2
        printf '%s\n' "$_new" | sed 's/^/  + /' >&2
        return 1
    fi
    return 0
}

# -----------------------------------------------------------------------
# Vacuity floor (task #6127, esc-6087-3).  _ratchet_check_subset above can
# only ever report "no NEW fingerprints"; it says nothing about whether the
# generator produced ANY.  A zero-fingerprint run therefore passes it
# trivially — the empty set is a subset of everything — and scenario (a)
# reports green having asserted nothing.  This is the precondition check
# that makes that subset assertion meaningful: the live set ($1) must be
# non-empty.  Silent + rc0 on the passing path so an all-green suite stays
# byte-for-byte unchanged (test_helpers.sh:48-51); a multi-line diagnostic
# to stderr + rc1 otherwise, landing in assert()'s on-FAIL captured-output
# dump.
#
# The committed-baseline content ($2) is used only to size the diagnostic.
# No fingerprints are re-derived here — counting lines of an already-produced
# set is not derivation, so the PRD 6.6 invariant in this file's header
# (derivation lives ONLY in ptodo-baseline-gen) is preserved.
# -----------------------------------------------------------------------
_ratchet_check_nonempty() {
    local _live="$1"
    local _baseline="${2:-}"
    local _live_n _baseline_n
    _live_n="$(printf '%s\n' "$_live" | grep -c . || true)"
    _baseline_n="$(printf '%s\n' "$_baseline" | grep -c . || true)"

    if [ "$_live_n" -ge 1 ]; then
        return 0
    fi

    {
        printf 'RATCHET VACUITY — ptodo-baseline-gen emitted 0 fingerprints (committed baseline entries: %s).\n' \
            "$_baseline_n"
        printf '  This is NOT a pass.  The ratchet oracle is `comm -23 <live> <baseline>`\n'
        printf '  (subset-of), and the empty set is a subset of everything — so the\n'
        printf '  subset assertion just observed nothing.  Reporting green here would\n'
        printf '  claim "the detector found no violations" when the detector in fact\n'
        printf '  reported nothing at all.\n'
        printf '  Leading hypothesis: target/release/ptodo-baseline-gen is STALE or\n'
        printf '  reverted.  mtime is a weak oracle for staleness — see the SCOPE LIMITATION\n'
        printf '  block in scripts/reify-audit-freshness.sh:28-36: the freshness epoch\n'
        printf '  only tracks commits under crates/reify-audit/, so a binary can read\n'
        printf '  "fresh" while predating a behavioural change made anywhere else.\n'
        printf '  NEXT: run `cargo build --release -p reify-audit`, then re-run.\n'
    } >&2
    return 1
}

# -----------------------------------------------------------------------
# ITEM 3 meta-test (task 5260): _ratchet_check_subset must NAME the offending
# live fingerprints on stderr so a ratchet regression lands actionable output in
# assert()'s on-FAIL captured-output dump (the 4636 failure produced zero
# fingerprints — esc-4959-57). Unconditional + hermetic: runs before binary
# resolution and independent of RATCHET_SKIP, driven by a synthetic fingerprint
# set (a command-substitution subshell inherits the shell function).
# -----------------------------------------------------------------------
_FAKE_FP=$'crates/foo/src/a.rs:42:PTODO-untracked\ncrates/foo/src/b.rs:7:PTODO-orphaned'
_DIAG="$(_ratchet_check_subset "$_FAKE_FP" 2>&1 1>/dev/null || true)"
assert "ratchet regression diagnostic names the offending fingerprints (4636 actionability)" \
    bash -c 'case "$1" in *a.rs:42:PTODO-untracked*b.rs:7:PTODO-orphaned*) exit 0;; *) exit 1;; esac' -- "$_DIAG"
_EMPTY="$(_ratchet_check_subset "" 2>&1 || true)"
assert "ratchet check is silent + rc0 when live set is empty" \
    bash -c '[ -z "$1" ]' -- "$_EMPTY"

# -----------------------------------------------------------------------
# VACUITY-FLOOR meta-test (task #6127, esc-6087-3).  The subset oracle above
# is trivially satisfied by the EMPTY set: a generator run that emitted zero
# fingerprints makes `comm -23 live baseline` empty, _ratchet_check_subset
# returns 0, and scenario (a) reports green having asserted nothing.
# _ratchet_check_nonempty is the floor that closes it.
#
# Same properties as the block above — unconditional, hermetic, driven by
# synthetic strings, and placed BEFORE binary resolution so it runs
# independently of RATCHET_SKIP.  It deliberately mints no temp files: the
# single EXIT trap below (see "Single EXIT trap covers all temp paths") is
# registered later, and a second `trap` here would silently replace it.
#
# Signature: _ratchet_check_nonempty <live-content> <baseline-content>, both
# newline-joined STRINGS (matching _ratchet_check_subset's argument
# convention) so it can be passed straight to assert() and land its
# diagnostic in the on-FAIL captured-output dump.
#
# These two asserts pin the LIVE-SET axis with the baseline held non-empty;
# the BASELINE axis (auto-disarm on a drained baseline) is pinned below.
# The WIRING of the floor into scenario (a) — which this hermetic block
# cannot see — is pinned by
# tests/infra/test_reify_audit_ptodo_ratchet_vacuity.sh.
# -----------------------------------------------------------------------
_VAC_DIAG="$(_ratchet_check_nonempty "" "$_FAKE_FP" 2>&1 1>/dev/null || true)"
assert "vacuity floor fires on an empty live set, with the RATCHET VACUITY anchor + the stale-binary hypothesis" \
    bash -c 'case "$1" in *"RATCHET VACUITY"*) ;; *) exit 1 ;; esac
             case "$1" in *stale*) exit 0 ;; *) exit 1 ;; esac' -- "$_VAC_DIAG"

# Positive control — a detector, not a constant failure: a non-empty live set
# must be byte-for-byte silent and rc0, so an all-green suite's output is
# unchanged (test_helpers.sh:48-51).
_VAC_RC=0
_VAC_OUT="$(_ratchet_check_nonempty "$_FAKE_FP" "$_FAKE_FP" 2>&1)" || _VAC_RC=$?
assert "vacuity floor is silent + rc0 when the live set is non-empty" \
    bash -c '[ -z "$1" ] && [ "$2" -eq 0 ]' -- "$_VAC_OUT" "$_VAC_RC"

# BASELINE axis — the floor must AUTO-DISARM once the baseline is drained.
# The ratchet is shrink-only (PRD 6.6: "After δ the baseline should be ≈
# empty"), so when the last grandfathered entry is burned down a
# 0-fingerprint live set becomes the CORRECT end state, not a vacuous pass.
# A floor hardcoded at >= 1 would false-RED from that moment on, and could
# only be defused by whoever removes the last baseline entry also
# remembering to delete an assertion in a different file.  Gating on the
# committed baseline being non-empty retires the floor at exactly the moment
# the ratchet succeeds, with nobody having to remember anything.
_VAC_DRAINED_RC=0
_VAC_DRAINED_OUT="$(_ratchet_check_nonempty "" "" 2>&1)" || _VAC_DRAINED_RC=$?
assert "vacuity floor auto-disarms (silent + rc0) when the committed baseline is itself empty" \
    bash -c '[ -z "$1" ] && [ "$2" -eq 0 ]' -- "$_VAC_DRAINED_OUT" "$_VAC_DRAINED_RC"

# -----------------------------------------------------------------------
# Resolve ptodo-baseline-gen binary (ride freshness guard).
# The freshness guard rebuilds target/release/reify-audit (and all crate
# bins, incl. ptodo-baseline-gen) when the binary predates the last
# crates/reify-audit commit.
#
# Testability seam (task #4624): REIFY_AUDIT_BIN and REIFY_PTODO_GEN_BIN can
# be overridden by environment variables for hermetic meta-tests that need to
# exercise the budget-safe skip path without a real binary on disk.
# -----------------------------------------------------------------------
REIFY_AUDIT_BIN="${REIFY_AUDIT_BIN:-$REPO_ROOT/target/release/reify-audit}"
GEN="${REIFY_PTODO_GEN_BIN:-$REPO_ROOT/target/release/ptodo-baseline-gen}"

source "$REPO_ROOT/scripts/reify-audit-freshness.sh"

# Use rebuild-budget-safe mode (task #4624): when REIFY_AUDIT_NO_COLD_BUILD=1
# and the binary is absent/stale, the guard returns 75 (EX_TEMPFAIL skip
# sentinel) instead of invoking `cargo build` inside the 20m run_all.sh wall.
#
# Task #4733 restructure: map 75 → RATCHET_SKIP=1 rather than exit 0.
# This lets scenarios (c)+(d) (the High-severity hard gate) still execute
# when the binary is PRESENT (only stale), while keeping the graceful-skip
# contract for scenarios (a)+(b) (the gen-driven ratchet, which is
# precision-sensitive and genuinely needs a fresh binary).
#
# A truly ABSENT binary cannot run anything; when REIFY_AUDIT_BIN is not
# executable, (c)+(d) skip gracefully with a one-line note.  That case is
# already defended upstream by verify.sh:1049's positive assertion that
# hard-aborts the plan if the pre-build produced no binary.
RATCHET_SKIP=0

# Did ANY scenario actually execute?  Consulted after test_summary; a run that
# asserted nothing must not exit 0.  See NO-SILENT-GREEN FLOOR in the header.
RAN=0

set +e
reify_audit_guard "$REIFY_AUDIT_BIN" rebuild-budget-safe "$REPO_ROOT" 2>&1
_guard_rc=$?
set -e

if [ "$_guard_rc" -eq 75 ]; then
    echo "test_reify_audit_ptodo.sh: reify-audit binary absent/stale and REIFY_AUDIT_NO_COLD_BUILD=1 — SKIP (budget-safe)" >&2
    RATCHET_SKIP=1
elif [ "$_guard_rc" -ne 0 ]; then
    # Any other nonzero rc — 125 from reify_audit_guard means the binary is
    # STILL judged stale after the rebuild path ran.  That covers two very
    # different worlds: a failed `cargo build -p reify-audit` (no usable
    # detector at all), and a build that was a legitimate no-op — cargo's
    # fingerprint says up-to-date — while the on-disk mtime still predates the
    # last crates/reify-audit commit, e.g. a warm-lane seeded target/ with
    # stamped mtimes, where the binary is fully usable.
    #
    # So split on detector USABILITY, exactly as the rc-75 partition above
    # does (#5962 review).  Collapsing both worlds into one unconditional
    # `exit 1` would turn every stamped-mtime warm-lane run into a hard RED
    # while emitting a diagnostic ("could not be made usable") that is
    # factually wrong about an executable binary.
    if [ -x "$REIFY_AUDIT_BIN" ]; then
        # PRESENT: the detector runs, so the staleness-tolerant (c)+(d)+(e)
        # hard gate must still execute — that is exactly the #4733 partition
        # documented in the header ("a present-but-stale binary emits High
        # findings correctly").  Only the precision-sensitive ratchet
        # ((a)+(b)) is skipped.  This is not a silent green: those scenarios
        # set $RAN, and the floor after test_summary still refuses a run that
        # executed none of them.
        echo "test_reify_audit_ptodo.sh: reify-audit freshness guard failed (rc=$_guard_rc) but '$REIFY_AUDIT_BIN' is executable — skipping the precision-sensitive ratchet ((a)+(b)); the (c)+(d)+(e)+(f) hard gate still runs against the stale binary" >&2
        RATCHET_SKIP=1
    else
        # ABSENT: nothing can run.  Leaving RATCHET_SKIP=0 here would look
        # like "ratchet enabled" while the `-x` guards below silently skip
        # every scenario.  That is not a budget-safe skip; it is a broken
        # toolchain, and it must be loud.
        # Pinned by tests/infra/test_reify_audit_ptodo_budget_skip.sh,
        # second invocation (REIFY_AUDIT_NO_COLD_BUILD unset).
        echo "test_reify_audit_ptodo.sh: reify-audit freshness guard failed (rc=$_guard_rc) — the detector could not be made usable and no budget-safe skip was requested; refusing to report green" >&2
        exit 1
    fi
fi

# If ratchet not yet skipped, ensure GEN is available.
# GEN checks are inside RATCHET_SKIP==0 because they only affect the ratchet
# path; the (c)+(d) hard gate runs from REIFY_AUDIT_BIN, not GEN.
if [ "${RATCHET_SKIP}" = "0" ]; then
    if [ ! -x "$GEN" ]; then
        if [ "${REIFY_AUDIT_NO_COLD_BUILD:-0}" = "1" ]; then
            echo "test_reify_audit_ptodo.sh: ptodo-baseline-gen absent and REIFY_AUDIT_NO_COLD_BUILD=1 — SKIP (budget-safe)" >&2
            RATCHET_SKIP=1
        else
            echo "ptodo-baseline-gen not found after freshness guard; building..." >&2
            cargo build --release -q -p reify-audit 2>/dev/null
        fi
    fi

    if [ ! -x "$GEN" ]; then
        echo "test_reify_audit_ptodo.sh: ptodo-baseline-gen unavailable — skipping ratchet" >&2
        RATCHET_SKIP=1
    fi
fi

BASELINE="$REPO_ROOT/crates/reify-audit/ptodo-baseline.txt"

# -----------------------------------------------------------------------
# Single EXIT trap covers all temp paths.  Registering two separate traps
# would silently replace the first with the second, leaking temps on exit.
# -----------------------------------------------------------------------
LIVE_TMP=""
FIX=""         # dirty fixture (scenario b/c): git repo with untracked marker
FIX_LIVE=""
FIX2=""        # scenario (c) clean-fixture temp dir
FIX2_RUNS=""   # scenario (c)/(d)/(e) empty runs-db file
FIX_D=""       # scenario (d) orphaned-cite fixture temp dir
FIX_C_TASKS="" # scenario (c) tasks-file bypass (empty JSON array, avoids MCP loading)
_err_tmp=""    # stderr capture file for run_audit (task #4800 defense-in-depth)
FIX_E=""       # scenario (e) G-allow orphaned-cite fixture temp dir
cleanup_all() {
    # Use "|| true" to ensure each line exits 0 even when the variable is empty
    # ([ -n "" ] && rm exits 1 from the short-circuit, which would propagate as
    # the trap's exit code and override the script's exit status).
    [ -n "$LIVE_TMP"    ] && rm -f  "$LIVE_TMP"    || true
    [ -n "$FIX"         ] && rm -rf "$FIX"          || true
    [ -n "$FIX_LIVE"    ] && rm -f  "$FIX_LIVE"    || true
    [ -n "$FIX2"        ] && rm -rf "$FIX2"         || true
    [ -n "$FIX2_RUNS"   ] && rm -f  "$FIX2_RUNS"   || true
    [ -n "$FIX_C_TASKS" ] && rm -f  "$FIX_C_TASKS" || true
    [ -n "$FIX_D"       ] && rm -rf "$FIX_D"        || true
    [ -n "$_err_tmp"    ] && rm -f  "$_err_tmp"     || true
    [ -n "$FIX_E"       ] && rm -rf "$FIX_E"        || true
}
trap cleanup_all EXIT

# -----------------------------------------------------------------------
# run_audit — defense-in-depth wrapper (task #4800)
#
# Routes all four reify-audit invocations through a retry+visibility helper:
#
#   (i)  Captures stderr to $_err_tmp so "git ls-files failed"/sqlite
#        breadcrumbs become VISIBLE in merge-gate logs when something goes
#        wrong — closing the 2>/dev/null blind spot that made transient
#        failures invisible.
#
#   (ii) Retries up to 3 times on specific transient-infra exit codes:
#        125 (IO-misconfig / sqlite EMFILE), 101 (Rust panic), 134/137/139
#        (SIGABRT/SIGKILL/SIGSEGV).  This absorbs the SECONDARY sqlite
#        rusqlite::Connection::open EMFILE→125 vector that the Rust
#        RealGitOps spawn-retry does NOT cover.  These codes are assumed
#        transient because the current fixtures only ever produce High-counts
#        of 0 or 1 — well below 101/125 — so there is no collision between
#        "infra failure" and "legitimate finding count".  All other exit
#        codes (not in the set above, and not in {0,1}) are treated as
#        AUTHORITATIVE and not retried.
#
#   Treats rc in {0,1} as AUTHORITATIVE — accepts immediately and never
#   retries — so a genuine wrong-finding-count (a real 0-vs-1 mismatch)
#   still goes RED.  No real assertion is weakened.
#
# Usage: run_audit [reify-audit-args...]
# Returns the final exit code; callers capture it with _exit_*=$?.
# -----------------------------------------------------------------------
_err_tmp="$(mktemp)"
run_audit() {
    local _attempt rc=0 _retried=0
    for _attempt in 1 2 3; do
        rc=0
        env -u REIFY_PTODO_TASKS_DB \
            "$REIFY_AUDIT_BIN" "$@" >/dev/null 2>"$_err_tmp" || rc=$?
        # rc in {0,1} is authoritative — accept immediately, never retry.
        if [ "$rc" -le 1 ]; then
            break
        fi
        # Retry only on the specific transient-infra codes enumerated above.
        # Any other rc (including ≥2 as a High-severity count) is authoritative.
        case "$rc" in
            125|101|134|137|139)
                _retried=1
                if [ "$_attempt" -lt 3 ]; then
                    sleep 2
                fi
                ;;
            *)
                # Non-infra exit code — treat as authoritative, do not retry.
                break
                ;;
        esac
    done
    # Surface captured stderr whenever a retry occurred — the retry itself is
    # the signal worth logging, regardless of the final rc.  This ensures the
    # "git ls-files failed"/sqlite breadcrumb is visible in merge-gate logs
    # even when the retry ultimately succeeds.
    if [ "$_retried" -eq 1 ]; then
        echo "run_audit: transient infra retry occurred (rc=$rc); stderr:" >&2
        cat "$_err_tmp" >&2
    fi
    return "$rc"
}

# -----------------------------------------------------------------------
# (a)+(b) RATCHET and HERMETIC FIXTURE — gen-driven, precision-sensitive.
# Wrapped in RATCHET_SKIP==0 guard (task #4733).
# -----------------------------------------------------------------------
if [ "${RATCHET_SKIP}" = "0" ] && [ -x "$GEN" ]; then
    RAN=1
    LIVE_TMP="$(mktemp)"

    # -----------------------------------------------------------------------
    # (a) RATCHET: live fingerprints (degraded-structural) must be a subset
    #     of the committed baseline.
    #     The committed baseline was generated WITH the task DB (a superset of
    #     structural + liveness findings), so a structural-only live set is
    #     guaranteed to be a subset of the baseline when the tree is clean.
    #     comm -23 <(sorted live) <(sorted baseline) = lines in live NOT in baseline.
    # -----------------------------------------------------------------------
    echo ""
    echo "--- (a) Ratchet: live fingerprints subset of committed baseline ---"

    # Run the generator in degraded-structural mode (no task DB).
    # Stderr may emit a breadcrumb about missing DB — that is expected and ignored.
    # Do NOT use || true here: a non-zero exit from the generator signals a broken
    # detector binary, not a missing DB.  The generator exits 0 regardless of
    # finding count; a non-zero exit is an infrastructure failure that must go red.
    env -u REIFY_PTODO_TASKS_DB "$GEN" --project-root "$REPO_ROOT" >"$LIVE_TMP" 2>/dev/null

    # comm -23 requires both inputs sorted; the generator sorts internally but
    # sort -u here is defensive.
    NEW_IN_LIVE="$(comm -23 <(sort -u "$LIVE_TMP") <(sort -u "$BASELINE"))"

    assert "live fingerprints are a subset of committed baseline (no ratchet regression)" \
        _ratchet_check_subset "$NEW_IN_LIVE"

    # -----------------------------------------------------------------------
    # (b) SCENARIO 13 (hermetic): a fresh untracked marker in a temp git
    #     repo produces fingerprints absent from an empty baseline, proving
    #     the gate would go red on a new violation.
    #
    #     SELF-MATCH SAFETY: the marker token is assembled from a variable
    #     so this .sh source never contains a literal form.
    # -----------------------------------------------------------------------
    echo ""
    echo "--- (b) Scenario 13: hermetic fixture detects fresh untracked marker ---"

    FIX="$(mktemp -d)"
    FIX_LIVE="$(mktemp)"

    git -C "$FIX" init -q
    mkdir -p "$FIX/src"

    # Assemble the marker token at runtime so this source file never contains
    # a literal swept token (the written fixture file carries the real marker).
    M="TODO"
    printf '// %s: wire this into the real implementation\n' "$M" > "$FIX/src/fresh.rs"
    git -C "$FIX" add -A

    # Run the generator on the hermetic fixture in degraded-structural mode.
    env -u REIFY_PTODO_TASKS_DB "$GEN" --project-root "$FIX" >"$FIX_LIVE" 2>/dev/null || true

    # The fixture live set must contain at least one untracked line for fresh.rs.
    UNTRACKED_LINE="$(grep 'src/fresh.rs' "$FIX_LIVE" | grep ':: untracked ::' || true)"
    assert "fixture live output contains an ':: untracked ::' line for src/fresh.rs" \
        bash -c '[ -n "$1" ]' -- "$UNTRACKED_LINE"

    # comm -23 against an empty baseline must be non-empty (ratchet goes red).
    NEW_IN_FIXTURE="$(comm -23 <(sort -u "$FIX_LIVE") /dev/null)"
    assert "fixture live fingerprints are NOT in empty baseline (gate fires red)" \
        bash -c '[ -n "$1" ]' -- "$NEW_IN_FIXTURE"
fi

# -----------------------------------------------------------------------
# (c) EXIT-CODE HARD GATE (task η, #4559): an untracked marker is
#     Severity::High → reify-audit exits NON-ZERO (exit code = High count).
#
#     Runs whenever REIFY_AUDIT_BIN is executable, independent of
#     staleness/RATCHET_SKIP (task #4733 fix).
#
#     Two hermetic cases using a dedicated dirty fixture (not $FIX from (b),
#     which may be absent when RATCHET_SKIP=1):
#       (c-dirty)  fresh.rs carries marker → non-zero exit
#       (c-clean)  repo with marker-free content → exit 0
#
#     VALIDATED DESIGN:
#       - An empty 0-byte file is an acceptable --runs-db for --pattern PTODO
#         (the CLI opens it but the PTODO lanes never read ctx.conn).
#       - env -u REIFY_PTODO_TASKS_DB prevents a stale env var from routing
#         liveness checks to an unexpected tasks DB.
#       - --tasks-file [] (FIX_C_TASKS) bypasses the MCP task loader entirely;
#         without it the binary tries to connect to fused-memory which may fail
#         with EMFILE or other infra errors, causing exit 125 (not 1).
#       - We test via EXIT CODE, not stream parsing (JSON goes to stderr;
#         the gate cares only about the process exit code = High-count).
#       - Uses structural High kind (untracked) which works without a
#         tasks.db; orphaned (liveness High) is exercised in scenario (d).
#
#     SELF-MATCH SAFETY: marker token assembled from $M at runtime.
# -----------------------------------------------------------------------
if [ -x "$REIFY_AUDIT_BIN" ]; then
    RAN=1
    echo ""
    echo "--- (c) Exit-code hard gate: untracked → High → non-zero exit ---"

    # Set up the dirty fixture for (c).  When scenario (b) ran (RATCHET_SKIP=0),
    # $FIX is already a git repo with src/fresh.rs carrying the marker, so we
    # reuse it.  When RATCHET_SKIP=1 (stale binary, b skipped), $FIX is empty
    # and we create a fresh fixture so (c) is self-contained.
    M="TODO"
    if [ -z "$FIX" ]; then
        FIX="$(mktemp -d)"
        git -C "$FIX" init -q
        mkdir -p "$FIX/src"
        printf '// %s: wire this into the real implementation\n' "$M" > "$FIX/src/fresh.rs"
        git -C "$FIX" add -A
    fi

    FIX2_RUNS="$(mktemp)"
    FIX_C_TASKS="$(mktemp)"
    printf '[]' > "$FIX_C_TASKS"

    # Snapshot FAIL before scenario (c) begins.  @@HARDGATE_C_PASSED@@ is emitted
    # ONLY when the counter is unchanged after all (c) asserts — i.e. every assert
    # passed.  A broken gate (any FAIL increment) suppresses the sentinel so the
    # meta-test stays RED (fixes silent_pass_on_failure).  The token contains no
    # TODO/FIXME/HACK substring and appears only in echo lines — SELF-MATCH SAFETY.
    _fail_before_c=$FAIL

    # Guard: assert the precondition — $FIX must be set and src/fresh.rs must
    # be git-tracked.  A failed precondition is an infra failure, not a product
    # regression — fail early with a clear message.
    assert "(c-dirty) precondition: \$FIX set and src/fresh.rs git-tracked" \
        bash -c 'git -C "$1" ls-files --error-unmatch src/fresh.rs >/dev/null 2>&1' -- "$FIX"

    # (c-dirty) marker present → exactly 1 High finding → exit 1.
    # Asserting the exact code (1) distinguishes "gate fired" from "binary errored"
    # (e.g. IO misconfig exits 125, Rust panic exits 101).
    # run_audit retries on transient rc>=2 and surfaces stderr on any retry.
    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_C_TASKS" \
        --no-jcodemunch
    _exit_dirty=$?
    set -e

    assert "(c-dirty) untracked marker → reify-audit exits 1 (exactly 1 High finding)" \
        bash -c '[ "$1" -eq 1 ]' -- "$_exit_dirty"

    # (c-clean) a clean repo has no High findings → exit 0.
    FIX2="$(mktemp -d)"
    git -C "$FIX2" init -q
    mkdir -p "$FIX2/src"
    printf '// no markers here — purely a comment\n' > "$FIX2/src/clean.rs"
    git -C "$FIX2" add -A

    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX2" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_C_TASKS" \
        --no-jcodemunch
    _exit_clean=$?
    set -e

    assert "(c-clean) no markers → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_clean"

    # Emit passing-branch sentinel for scenario (c).  Gated on FAIL counter
    # unchanged — suppressed if any (c) assert failed (fixes silent_pass_on_failure).
    [ "$FAIL" -eq "$_fail_before_c" ] && echo "@@HARDGATE_C_PASSED@@"

    # -----------------------------------------------------------------------
    # (d) ORPHANED-CITE HARD GATE (task #4733): a cite to a DONE task is
    #     classified orphaned→High → reify-audit exits NON-ZERO.
    #
    #     Hermetic recipe (mirrors crates/reify-audit/tests/cli.rs:1632-1716):
    #       - Temp git repo with a single cited marker in src/cited.rs
    #         (assembled from $M + $CITE_ID so this source never contains the
    #         literal swept form — SELF-MATCH SAFETY).
    #       - Seed <repo>/.taskmaster/tasks/tasks.db via sqlite3 with the
    #         production tasks schema + INSERT (master,4444,'done').
    #       - Write [] to a temp file for --tasks-file (bypasses MCP loader
    #         while the PTODO β liveness lane still reads the sqlite3 tasks.db).
    #       - env -u REIFY_PTODO_TASKS_DB prevents stale env from routing.
    #
    #     Two assertions:
    #       (d-orphan)  cited.rs + task done → orphaned High → exit 1
    #       (d-control) UPDATE task to pending → live cite → exit 0
    #
    #     VALIDATED DESIGN (from crates/reify-audit/tests/cli.rs §8.3):
    #       - src/cited.rs has ONLY the cited marker (no bare markers) so the
    #         structural untracked lane does NOT fire → exactly 1 High (orphaned).
    #       - The tasks.db is seeded AFTER git-add to mirror the untracked-in-
    #         worktree reality of a real merge verify.
    # -----------------------------------------------------------------------
    echo ""
    echo "--- (d) Orphaned-cite hard gate: done-task cite → orphaned → High → non-zero exit ---"

    FIX_D="$(mktemp -d)"
    git -C "$FIX_D" init -q
    mkdir -p "$FIX_D/src"

    # Assemble the cited marker token at runtime (SELF-MATCH SAFETY).
    M="TODO"
    CITE_ID="4444"
    printf '// %s(#%s): wire the orphaned-cite path\n' "$M" "$CITE_ID" > "$FIX_D/src/cited.rs"
    git -C "$FIX_D" add -A

    # Seed tasks.db AFTER the git commit (mirrors untracked-in-worktree reality).
    # Schema mirrors crates/reify-audit/tests/common/schema.rs TASKS_DB_SCHEMA.
    mkdir -p "$FIX_D/.taskmaster/tasks"
    # LD_LIBRARY_PATH="" so sqlite3 uses the system lib, not /opt/reify-deps/lib's
    # newer libsqlite3 (set by verify.sh) which would crash with a header/source
    # version mismatch (esc-4581-87).
    LD_LIBRARY_PATH="" sqlite3 "$FIX_D/.taskmaster/tasks/tasks.db" "
CREATE TABLE tasks (
    tag TEXT NOT NULL DEFAULT 'master',
    id INTEGER NOT NULL,
    title TEXT,
    status TEXT NOT NULL,
    metadata TEXT,
    PRIMARY KEY (tag, id)
);
INSERT INTO tasks (tag, id, status) VALUES ('master', ${CITE_ID}, 'done');
"

    # Write an empty JSON array for --tasks-file (bypasses MCP; liveness lane
    # still reads the sqlite3 tasks.db at <project_root>/.taskmaster/tasks/tasks.db).
    FIX_D_TASKS_FILE="$FIX_D/tasks.json"
    printf '[]' > "$FIX_D_TASKS_FILE"

    # Snapshot FAIL before scenario (d) begins.  @@HARDGATE_D_PASSED@@ is emitted
    # ONLY when the counter is unchanged after all (d) asserts — i.e. every assert
    # passed.  A broken gate suppresses the sentinel — SELF-MATCH SAFETY as above.
    _fail_before_d=$FAIL

    # (d-orphan) done task → orphaned → High → exit 1.
    # run_audit retries on transient rc>=2 and surfaces stderr on any retry.
    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX_D" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_D_TASKS_FILE" \
        --no-jcodemunch
    _exit_orphan=$?
    set -e

    assert "(d-orphan) orphaned cite (#${CITE_ID}) → done-task → reify-audit exits 1 (exactly 1 High)" \
        bash -c '[ "$1" -eq 1 ]' -- "$_exit_orphan"

    # (d-control) UPDATE task status to pending → live cite → no High → exit 0.
    LD_LIBRARY_PATH="" sqlite3 "$FIX_D/.taskmaster/tasks/tasks.db" \
        "UPDATE tasks SET status='pending' WHERE id=${CITE_ID};"

    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX_D" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_D_TASKS_FILE" \
        --no-jcodemunch
    _exit_live=$?
    set -e

    assert "(d-control) pending-task cite → live cite → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_live"

    # Emit passing-branch sentinel for scenario (d).  Gated on FAIL counter
    # unchanged — suppressed if any (d) assert failed (fixes silent_pass_on_failure).
    [ "$FAIL" -eq "$_fail_before_d" ] && echo "@@HARDGATE_D_PASSED@@"

    # -----------------------------------------------------------------------
    # (e) G-ALLOW ORPHANED HARD GATE (task #4754): a `// G-allow:` OWNER-cite
    #     pointing to a DONE task is classified g-allow-orphaned→High →
    #     reify-audit exits NON-ZERO.
    #
    #     Mirrors scenario (d) exactly, differing only in the fixture line:
    #     a `// G-allow:` owner marker (assembled from $GA + $CITE_ID_E so
    #     this .sh source never contains a literal swept form — SELF-MATCH
    #     SAFETY) instead of a TODO cite.
    #
    #     check() only scans G-allow markers in .rs files, and the temp git
    #     repo is not under crates/reify-audit/ (the allowlisted prefix), so
    #     the marker in src/gallow.rs is swept and resolved.
    #
    #     Two assertions:
    #       (e-orphan)  gallow.rs + task done → g-allow-orphaned High → exit 1
    #       (e-control) UPDATE task to pending → live cite → exit 0
    #
    #     RED until step-3 removes the `f.severity = Severity::Medium` remap
    #     in check() — currently Medium findings are exit-neutral (exit 0).
    # -----------------------------------------------------------------------
    echo ""
    echo "--- (e) G-allow orphaned hard gate: done-task owner-cite → High → non-zero exit ---"

    FIX_E="$(mktemp -d)"
    git -C "$FIX_E" init -q
    mkdir -p "$FIX_E/src"

    # Assemble the G-allow owner-cite marker at runtime (SELF-MATCH SAFETY:
    # check() only scans .rs files for `// G-allow:` markers, but we use the
    # $GA variable convention to keep this .sh source free of literal owner
    # markers, matching (d)'s $M/$CITE_ID discipline).
    GA="G-allow"
    CITE_ID_E="5555"
    printf '// %s: task #%s owner reason\n' "$GA" "$CITE_ID_E" > "$FIX_E/src/gallow.rs"
    git -C "$FIX_E" add -A

    # Seed tasks.db AFTER the git add (mirrors (d)'s untracked-in-worktree reality).
    # Schema mirrors crates/reify-audit/tests/common/schema.rs TASKS_DB_SCHEMA.
    mkdir -p "$FIX_E/.taskmaster/tasks"
    # LD_LIBRARY_PATH="" so sqlite3 uses the system lib (esc-4581-87).
    LD_LIBRARY_PATH="" sqlite3 "$FIX_E/.taskmaster/tasks/tasks.db" "
CREATE TABLE tasks (
    tag TEXT NOT NULL DEFAULT 'master',
    id INTEGER NOT NULL,
    title TEXT,
    status TEXT NOT NULL,
    metadata TEXT,
    PRIMARY KEY (tag, id)
);
INSERT INTO tasks (tag, id, status) VALUES ('master', ${CITE_ID_E}, 'done');
"

    # Write an empty JSON array for --tasks-file (bypasses MCP; liveness lane
    # still reads the sqlite3 tasks.db at <project_root>/.taskmaster/tasks/tasks.db).
    FIX_E_TASKS_FILE="$FIX_E/tasks.json"
    printf '[]' > "$FIX_E_TASKS_FILE"

    # Snapshot FAIL before scenario (e) begins.  @@HARDGATE_E_PASSED@@ is emitted
    # ONLY when the counter is unchanged after all (e) asserts — i.e. every assert
    # passed.  A broken gate suppresses the sentinel.
    _fail_before_e=$FAIL

    # (e-orphan) done task → g-allow-orphaned → High → exit 1.
    # RED until check() removes the Medium remap (step-3 of task #4754).
    set +e
    env -u REIFY_PTODO_TASKS_DB \
        "$REIFY_AUDIT_BIN" \
            --pattern PTODO \
            --project-root "$FIX_E" \
            --runs-db "$FIX2_RUNS" \
            --tasks-file "$FIX_E_TASKS_FILE" \
            --no-jcodemunch \
            >/dev/null 2>/dev/null
    _exit_gallow_orphan=$?
    set -e

    assert "(e-orphan) G-allow owner-cite (#${CITE_ID_E}) → done-task → g-allow-orphaned High → reify-audit exits 1" \
        bash -c '[ "$1" -eq 1 ]' -- "$_exit_gallow_orphan"

    # (e-control) UPDATE task status to pending → live cite → no High → exit 0.
    LD_LIBRARY_PATH="" sqlite3 "$FIX_E/.taskmaster/tasks/tasks.db" \
        "UPDATE tasks SET status='pending' WHERE id=${CITE_ID_E};"

    set +e
    env -u REIFY_PTODO_TASKS_DB \
        "$REIFY_AUDIT_BIN" \
            --pattern PTODO \
            --project-root "$FIX_E" \
            --runs-db "$FIX2_RUNS" \
            --tasks-file "$FIX_E_TASKS_FILE" \
            --no-jcodemunch \
            >/dev/null 2>/dev/null
    _exit_gallow_live=$?
    set -e

    assert "(e-control) pending-task G-allow owner-cite → live cite → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_gallow_live"

    # Emit passing-branch sentinel for scenario (e).  Gated on FAIL counter
    # unchanged — suppressed if any (e) assert failed (fixes silent_pass_on_failure).
    [ "$FAIL" -eq "$_fail_before_e" ] && echo "@@HARDGATE_E_PASSED@@"

    # -----------------------------------------------------------------------
    # (f) LANE δ-A HARD GATE (task #6087): an `#[allow(...dead_code...)]`
    #     attribute whose trailing rationale defers the work AND cites a DONE
    #     task is classified orphaned→High → reify-audit exits NON-ZERO.
    #
    #     A direct transposition of scenario (d), inheriting its three hard-won
    #     fixes for free: the LD_LIBRARY_PATH="" sqlite3 workaround
    #     (esc-4581-87), the gate-on-binary-not-on-RATCHET_SKIP structure
    #     (#4733), and the no-silent-green RAN floor.
    #
    #     Three assertions — the third is the one the ratified FP-control
    #     requirement buys, proving the guards at the BINARY level and not only
    #     in unit tests:
    #       (f-orphan)  allowed.rs + task done  → orphaned High → exit 1
    #       (f-control) UPDATE task to pending  → live cite     → exit 0
    #       (f-benign)  non-deferral rationale  → no finding    → exit 0
    #
    #     SELF-MATCH SAFETY: this file is swept and is NOT allowlisted.  Lane
    #     δ-A is .rs-gated so a literal attribute here could not fire it, but
    #     the file's discipline is followed regardless — the attribute text,
    #     the deferral needle and the cite id are assembled from shell
    #     variables at runtime, so no literal attribute+cite+prose triple ever
    #     appears in the committed source.
    # -----------------------------------------------------------------------
    echo ""
    echo "--- (f) Lane δ-A hard gate: allow(dead_code) + deferral + done-task cite → High → non-zero exit ---"

    FIX_F="$(mktemp -d)"
    git -C "$FIX_F" init -q
    mkdir -p "$FIX_F/src"

    # Assemble the δ-A anchor at runtime (SELF-MATCH SAFETY, as in (d)/(e)).
    ADC="allow(dead_code)"
    DEFER="pending"
    CITE_ID_F="6666"
    printf '#[%s] // production wiring %s task #%s (hermetic fixture)\n' \
        "$ADC" "$DEFER" "$CITE_ID_F" > "$FIX_F/src/allowed.rs"
    # The benign control lives in the SAME repo but must contribute nothing;
    # its rationale explains rather than defers (the dominant real-world class).
    printf '#[%s] // used by some, but not all, test binaries that include this\n' \
        "$ADC" > "$FIX_F/src/benign.rs"
    git -C "$FIX_F" add -A

    # Seed tasks.db AFTER the git add (mirrors (d)'s untracked-in-worktree reality).
    # Schema mirrors crates/reify-audit/tests/common/schema.rs TASKS_DB_SCHEMA.
    mkdir -p "$FIX_F/.taskmaster/tasks"
    # LD_LIBRARY_PATH="" so sqlite3 uses the system lib (esc-4581-87).
    LD_LIBRARY_PATH="" sqlite3 "$FIX_F/.taskmaster/tasks/tasks.db" "
CREATE TABLE tasks (
    tag TEXT NOT NULL DEFAULT 'master',
    id INTEGER NOT NULL,
    title TEXT,
    status TEXT NOT NULL,
    metadata TEXT,
    PRIMARY KEY (tag, id)
);
INSERT INTO tasks (tag, id, status) VALUES ('master', ${CITE_ID_F}, 'done');
"

    # Write an empty JSON array for --tasks-file (bypasses MCP; liveness lane
    # still reads the sqlite3 tasks.db at <project_root>/.taskmaster/tasks/tasks.db).
    FIX_F_TASKS_FILE="$FIX_F/tasks.json"
    printf '[]' > "$FIX_F_TASKS_FILE"

    # Snapshot FAIL before scenario (f) begins.  @@HARDGATE_F_PASSED@@ is emitted
    # ONLY when the counter is unchanged after all (f) asserts — i.e. every assert
    # passed.  A broken gate suppresses the sentinel.
    _fail_before_f=$FAIL

    # (f-orphan) done task → orphaned → High → exit 1.  Exactly one High: the
    # benign sibling contributes nothing, which is what makes 1 (not 2) the
    # assertion that also proves FP control.
    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX_F" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_F_TASKS_FILE" \
        --no-jcodemunch
    _exit_adc_orphan=$?
    set -e

    assert "(f-orphan) allow(dead_code) deferral cite (#${CITE_ID_F}) → done-task → orphaned High → reify-audit exits 1" \
        bash -c '[ "$1" -eq 1 ]' -- "$_exit_adc_orphan"

    # (f-control) UPDATE task status to pending → live cite → no High → exit 0.
    LD_LIBRARY_PATH="" sqlite3 "$FIX_F/.taskmaster/tasks/tasks.db" \
        "UPDATE tasks SET status='pending' WHERE id=${CITE_ID_F};"

    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX_F" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_F_TASKS_FILE" \
        --no-jcodemunch
    _exit_adc_live=$?
    set -e

    assert "(f-control) pending-task allow(dead_code) deferral cite → live cite → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_adc_live"

    # (f-benign) a repo carrying ONLY non-deferral allow-rationales → no
    # finding at all → exit 0.  This is the FP-control guard proven at the
    # binary level: an over-broad lane fires `untracked`→High here and the
    # exit code becomes non-zero.
    FIX_F2="$(mktemp -d)"
    git -C "$FIX_F2" init -q
    mkdir -p "$FIX_F2/src"
    {
        printf '#[%s] // used by some, but not all, test binaries that include this\n' "$ADC"
        printf '#[%s] // superseded by mark_%s_with_cause at the wire site\n' "$ADC" "$DEFER"
        printf '#[%s] // constructs a Pending producer for the demand-prune tests\n' "$ADC"
        printf '#[%s] // serialises the "%s" wire tag for the GUI bridge\n' "$ADC" "$DEFER"
    } > "$FIX_F2/src/benign.rs"
    git -C "$FIX_F2" add -A

    FIX_F2_TASKS_FILE="$FIX_F2/tasks.json"
    printf '[]' > "$FIX_F2_TASKS_FILE"

    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX_F2" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_F2_TASKS_FILE" \
        --no-jcodemunch
    _exit_adc_benign=$?
    set -e

    assert "(f-benign) non-deferral allow(dead_code) rationales (incl. identifier/enum-variant/quoted-state guards) → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_adc_benign"

    # Emit passing-branch sentinel for scenario (f).  Gated on FAIL counter
    # unchanged — suppressed if any (f) assert failed (fixes silent_pass_on_failure).
    [ "$FAIL" -eq "$_fail_before_f" ] && echo "@@HARDGATE_F_PASSED@@"
else
    echo ""
    echo "test_reify_audit_ptodo.sh: reify-audit binary absent at '$REIFY_AUDIT_BIN' — (c)+(d)+(e)+(f) hard gate could not run" >&2
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
    echo "test_reify_audit_ptodo.sh: NO scenario executed (REIFY_AUDIT_BIN='$REIFY_AUDIT_BIN' not executable; RATCHET_SKIP=$RATCHET_SKIP, guard rc=$_guard_rc) — refusing to report green for a hard gate that asserted nothing" >&2
    exit 1
fi
