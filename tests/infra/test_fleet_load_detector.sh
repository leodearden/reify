#!/usr/bin/env bash
# tests/infra/test_fleet_load_detector.sh
# Hermetic tests for scripts/fleet-load-detector.sh (task 5135).
#
# Fleet-wide host-oversubscription DETECTOR: observes HOST-AGGREGATE CPU
# oversubscription across ALL concurrently-dispatched orchestrator lanes by
# reading host-global /proc/loadavg + /proc/pressure/cpu (the aggregate
# contributed by every dispatched lane — no per-lane enumeration needed).
# Reify-side signal for the DF-owned L3b dispatch-admission companion
# (docs/prds/run-all-pool-contention-tiering-fix.md §9).
#
# Fully hermetic: env-injected synthetic loadavg-path/psi-path/nproc; no real
# /proc reads, no CPU burn. Classified `pool` in run-all-classification.manifest.
#
# Test seams (env-controlled synthetic sources; script never touches the
# real host /proc during these tests):
#   REIFY_FLEET_LOAD_LOADAVG_PATH      — /proc/loadavg-format file (field 1 = load1)
#   REIFY_FLEET_LOAD_PSI_PATH          — /proc/pressure/cpu-format file (avg10=)
#   REIFY_FLEET_LOAD_NPROC             — synthetic nproc (avoids reading the real host)
#   REIFY_FLEET_LOAD_RATIO_THRESHOLD   — ratio ceiling override (default 4.0)
#   REIFY_FLEET_LOAD_AVG10_THRESHOLD   — avg10 % ceiling override (default 80)
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks (grown incrementally across task 5135's plan.json TDD steps):
#   A — CLI guard: --help, unknown flag, missing/unknown subcommand
#   (B..I land in later steps)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob; co-registered
# `pool` in run-all-classification.manifest so the declared==discovered
# classification drift-guard (test_run_all_classification.sh) stays GREEN.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/fleet-load-detector.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/fleet-load-detector.sh hermetic tests (task 5135) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

ERR_FILE="$(mktemp /tmp/test-fleet-load-detector-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── run_helper ─────────────────────────────────────────────────────────────────
# Invokes the script directly (no PATH stub needed — synthetic sources are
# wired via REIFY_FLEET_LOAD_* env vars, inherited from inline prefixes on the
# call site). Sets OUT (stdout), ERR_OUT (stderr), RC (exit code) as globals.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: `check --help` exits 0 and prints usage on stderr
run_helper check --help
assert "A1: check --help exits 0" test "$RC" -eq 0
assert "A1: check --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A2: bare --help (no subcommand) also exits 0 and prints usage
run_helper --help
assert "A2: bare --help exits 0" test "$RC" -eq 0
assert "A2: bare --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A3: unknown flag (bare, no subcommand) exits 2
run_helper --unknown-flag-xyz
assert "A3: unknown flag exits 2" test "$RC" -eq 2

# A4: unknown flag after a valid subcommand exits 2 (regression guard for the
# flag-parse branch ordering — an unknown flag must not silently pass through
# once a subcommand has already been recognized)
run_helper check --unknown-flag-xyz
assert "A4: unknown flag after subcommand exits 2" test "$RC" -eq 2

# A5: no subcommand (bare invocation) exits 2
run_helper
assert "A5: no subcommand exits 2" test "$RC" -eq 2

# A6: unknown subcommand exits 2
run_helper frobulate
assert "A6: unknown subcommand exits 2" test "$RC" -eq 2

# ── fixture helpers ────────────────────────────────────────────────────────────
# _mk_loadavg LOAD1 — writes a /proc/loadavg-format file with field 1 = LOAD1.
_mk_loadavg() {
    local _f
    _f="$(mktemp /tmp/test-fleet-load-detector-loadavg-XXXXXX)"
    _TMPDIRS+=("$_f")
    printf '%s 0.40 0.30 1/100 999\n' "$1" > "$_f"
    echo "$_f"
}

# _mk_psi AVG10 — writes a /proc/pressure/cpu-format file with `some` avg10 = AVG10.
_mk_psi() {
    local _f
    _f="$(mktemp /tmp/test-fleet-load-detector-psi-XXXXXX)"
    _TMPDIRS+=("$_f")
    printf 'some avg10=%s avg60=0.50 avg300=0.10 total=12345\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' "$1" > "$_f"
    echo "$_f"
}

# ──────────────────────────────────────────────────────────────────────────────
# Block B — happy path: ratio < threshold AND avg10 < threshold → healthy
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: happy path (healthy) ---"

B_LOADAVG="$(_mk_loadavg 0.50)"
B_PSI="$(_mk_psi 1.00)"

REIFY_FLEET_LOAD_LOADAVG_PATH="$B_LOADAVG" REIFY_FLEET_LOAD_PSI_PATH="$B_PSI" REIFY_FLEET_LOAD_NPROC=32 \
    run_helper check
assert "B1: healthy fixture exits 0" test "$RC" -eq 0
assert "B2: stdout is exactly the expected FLEET_LOAD verdict line" \
    bash -c '[ "$1" = "FLEET_LOAD status=ok load1=0.50 nproc=32 ratio=0.02 ratio_threshold=4.0 avg10=1.00 avg10_threshold=80 reason=none" ]' \
    _ "$OUT"
assert "B3: stdout is a single line" \
    bash -c '[ "$(printf "%s\n" "$1" | wc -l)" -eq 1 ]' _ "$OUT"
assert "B4: no oversubscribed marker on stderr" \
    bash -c '! printf "%s\n" "$1" | grep -q "@@REIFY_FLEET_OVERSUBSCRIBED@@"' _ "$ERR_OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block F — boundary: pins the >= operator on the ratio axis via the STDOUT
# status field only. Exit-code/marker behavior for the flagged path is wired
# in step-6 — asserting RC here for the AT-threshold row would regress once
# step-6 lands (it flips from 0 to 3), so only the JUST-BELOW row (whose exit
# code never changes) asserts RC.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: boundary (>= operator) ---"

# F1: load1=128 nproc=32 → ratio exactly 4.0 → status=oversubscribed (>=, not >)
F1_LOADAVG="$(_mk_loadavg 128)"
F_PSI="$(_mk_psi 1.00)"
REIFY_FLEET_LOAD_LOADAVG_PATH="$F1_LOADAVG" REIFY_FLEET_LOAD_PSI_PATH="$F_PSI" REIFY_FLEET_LOAD_NPROC=32 \
    run_helper check
assert "F1: ratio exactly at threshold (4.00) is status=oversubscribed" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])status=oversubscribed([[:space:]]|$)"' _ "$OUT"
assert "F1: ratio field reads 4.00" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])ratio=4\.00([[:space:]]|$)"' _ "$OUT"

# F2: load1=127 nproc=32 → ratio 3.97 (just below 4.0) → status=ok, exit 0
F2_LOADAVG="$(_mk_loadavg 127)"
REIFY_FLEET_LOAD_LOADAVG_PATH="$F2_LOADAVG" REIFY_FLEET_LOAD_PSI_PATH="$F_PSI" REIFY_FLEET_LOAD_NPROC=32 \
    run_helper check
assert "F2: ratio just below threshold (3.97) is status=ok" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])status=ok([[:space:]]|$)"' _ "$OUT"
assert "F2: just-below-threshold exits 0" test "$RC" -eq 0

test_summary
