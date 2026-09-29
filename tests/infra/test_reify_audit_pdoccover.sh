#!/usr/bin/env bash
# tests/infra/test_reify_audit_pdoccover.sh
#
# Infra hard gate for PDOCCOVER, the registry<->chunk name-drift detector
# (task #6931).  Two scenarios:
#
#   (a) RATCHET — `reify-audit --pattern PDOCCOVER --project-root $REPO_ROOT`
#                 exits 0 against the committed
#                 crates/reify-audit/pdoccover-baseline.txt.  reify-audit's exit
#                 code is its High count, and every PDOCCOVER category is High:
#                 new omission or fabrication debt, a stale ledger row, a stale
#                 allow marker, a reasonless allow marker, and a tree it could
#                 not read (census-empty / no-chunks) all red the gate.
#
#   (b) MATRIX  — tests/infra/test_reify_audit_pdoccover.py drives the same
#                 binary over hermetic staged fixture repos (stdlib unittest).
#
# The debt derivation lives only in pdoccover::baseline_ledger, which both the
# ratchet and src/bin/pdoccover-baseline-gen.rs call; nothing here re-derives a
# row — every assertion reads the binary's exit code and stderr.
#
# Budget-safe partition, exactly test_reify_audit_pdiag.sh's (esc-5405-7/-9):
# a present-but-possibly-stale binary sets RATCHET_SKIP=1, which skips (a) —
# a live scan compared against a ledger committed alongside the current
# scanner — and tells (b) to skip the cases a pre-ledger binary cannot know.
# An absent binary exits 1.  The RAN floor refuses a green that asserted
# nothing.
#
# Auto-discovered by tests/infra/run_all.sh; bucketed intra-run-serial in
# tests/infra/run-all-classification.manifest (the freshness guard may fork
# cargo and mutate the lane-shared CoW target/).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

for _tool in git cargo python3; do
    if ! command -v "$_tool" >/dev/null 2>&1; then
        echo "test_reify_audit_pdoccover.sh: $_tool not on PATH — skipping" >&2
        exit 0
    fi
done

echo "=== PDOCCOVER detector infra gate ==="

BASELINE="$REPO_ROOT/crates/reify-audit/pdoccover-baseline.txt"

# Scenario (a)'s failure path: name the exit code, replay the detector's JSON
# findings, and route the reader to the fix.  Silent and 0 on success.
_ratchet_exit_ok() {
    local _rc="$1" _errfile="$2"
    if [ "$_rc" -ne 0 ]; then
        printf 'PDOCCOVER RATCHET RED — reify-audit --pattern PDOCCOVER exited %s against the committed ledger.\n' \
            "$_rc" >&2
        printf 'Fix the finding: document the name, correct the chunk, or mark the line `pdoccover:allow — <reason>`.\n' >&2
        printf 'A stale-baseline-entry: delete the row, or rerun `cargo run -p reify-audit --bin pdoccover-baseline-gen -- --project-root . > crates/reify-audit/pdoccover-baseline.txt`.\n' >&2
        printf 'Only deliberate, review-justified debt goes in via `--admit-new`.\n' >&2
        if [ -s "$_errfile" ]; then
            printf -- '---- reify-audit stderr (tail -40) ----\n' >&2
            tail -n 40 "$_errfile" >&2
        fi
        return 1
    fi
    return 0
}

# REIFY_AUDIT_BIN is overridable so meta-tests can exercise the skip paths.
REIFY_AUDIT_BIN="${REIFY_AUDIT_BIN:-$REPO_ROOT/target/release/reify-audit}"

source "$REPO_ROOT/scripts/reify-audit-freshness.sh"

RATCHET_SKIP=0
RAN=0

set +e
reify_audit_guard "$REIFY_AUDIT_BIN" rebuild-budget-safe "$REPO_ROOT" 2>&1
_guard_rc=$?
set -e

if [ "$_guard_rc" -eq 75 ]; then
    echo "test_reify_audit_pdoccover.sh: reify-audit binary absent/stale and REIFY_AUDIT_NO_COLD_BUILD=1 — (a) SKIP (budget-safe)" >&2
    RATCHET_SKIP=1
elif [ "$_guard_rc" -ne 0 ]; then
    # 125: still judged stale after the rebuild path ran.  Split on USABILITY,
    # as test_reify_audit_pdiag.sh does: an executable binary still runs the
    # staleness-stable half of (b); an absent one cannot run anything.
    if [ -x "$REIFY_AUDIT_BIN" ]; then
        echo "test_reify_audit_pdoccover.sh: reify-audit freshness guard failed (rc=$_guard_rc) but '$REIFY_AUDIT_BIN' is executable — skipping (a) and the ledger-grammar half of (b)" >&2
        RATCHET_SKIP=1
    else
        echo "test_reify_audit_pdoccover.sh: reify-audit freshness guard failed (rc=$_guard_rc) — the detector could not be made usable and no budget-safe skip was requested; refusing to report green" >&2
        exit 1
    fi
fi

AUX=""
_err_tmp=""
cleanup_all() {
    [ -n "$AUX"      ] && rm -rf "$AUX"      || true
    [ -n "$_err_tmp" ] && rm -f  "$_err_tmp" || true
}
trap cleanup_all EXIT

# run_audit — capture stderr to $_err_tmp and retry the transient-infra codes
# 125/101/134/137/139.  rc 0 or 1 is authoritative and never retried.
_err_tmp="$(mktemp)"
run_audit() {
    local _attempt rc=0 _retried=0
    for _attempt in 1 2 3; do
        rc=0
        "$REIFY_AUDIT_BIN" "$@" >/dev/null 2>"$_err_tmp" || rc=$?
        case "$rc" in
            125|101|134|137|139)
                _retried=1
                [ "$_attempt" -lt 3 ] && sleep 2
                ;;
            *) break ;;
        esac
    done
    if [ "$_retried" -eq 1 ]; then
        echo "run_audit: transient infra retry occurred (rc=$rc); stderr:" >&2
        cat "$_err_tmp" >&2
    fi
    return "$rc"
}

# --tasks-file [] keeps the task loader off fused-memory; a 0-byte runs DB is
# acceptable because PDOCCOVER never reads ctx.conn.
AUX="$(mktemp -d)"
AUX_TASKS="$AUX/tasks.json"
AUX_RUNS="$AUX/runs.db"
printf '[]' > "$AUX_TASKS"
: > "$AUX_RUNS"

# -----------------------------------------------------------------------
# (a) RATCHET: the live tree must be exactly within the committed ledger.
# -----------------------------------------------------------------------
if [ "$RATCHET_SKIP" = "0" ] && [ -x "$REIFY_AUDIT_BIN" ]; then
    echo ""
    echo "--- (a) Ratchet: live tree against committed pdoccover-baseline.txt ---"
    RAN=1

    # An untracked ledger is inert — PDOCCOVER reads only git-tracked inputs —
    # so presence alone would let (a) red for the wrong stated cause.
    assert "(a) precondition: crates/reify-audit/pdoccover-baseline.txt exists" \
        test -f "$BASELINE"
    assert "(a) precondition: crates/reify-audit/pdoccover-baseline.txt is git-tracked" \
        git -C "$REPO_ROOT" ls-files --error-unmatch crates/reify-audit/pdoccover-baseline.txt

    set +e
    run_audit \
        --pattern PDOCCOVER \
        --project-root "$REPO_ROOT" \
        --runs-db "$AUX_RUNS" \
        --tasks-file "$AUX_TASKS" \
        --no-jcodemunch
    _exit_ratchet=$?
    set -e

    assert "(a) live tree is exactly within the committed ledger" \
        _ratchet_exit_ok "$_exit_ratchet" "$_err_tmp"
fi

# -----------------------------------------------------------------------
# (b) MATRIX: hermetic fixture repos through the same binary.
# -----------------------------------------------------------------------
if [ -x "$REIFY_AUDIT_BIN" ]; then
    echo ""
    echo "--- (b) Hermetic matrix: tests/infra/test_reify_audit_pdoccover.py ---"
    RAN=1

    source "$REPO_ROOT/scripts/lib_git_env_scrub.sh"
    export REIFY_AUDIT_BIN
    export REIFY_GIT_ENV_SCRUB_VARS
    export REIFY_PDOCCOVER_BIN_MAY_BE_STALE="$RATCHET_SKIP"

    assert "(b) tests/infra/test_reify_audit_pdoccover.py exits 0" \
        python3 "$SCRIPT_DIR/test_reify_audit_pdoccover.py"
else
    echo ""
    echo "test_reify_audit_pdoccover.sh: reify-audit binary absent at '$REIFY_AUDIT_BIN' — (b) could not run" >&2
fi

test_summary

if [ "$RAN" -eq 0 ]; then
    echo "test_reify_audit_pdoccover.sh: NO scenario executed (REIFY_AUDIT_BIN='$REIFY_AUDIT_BIN' not executable; RATCHET_SKIP=$RATCHET_SKIP, guard rc=$_guard_rc) — refusing to report green for a hard gate that asserted nothing" >&2
    exit 1
fi
