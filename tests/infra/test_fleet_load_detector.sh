#!/usr/bin/env bash
# tests/infra/test_fleet_load_detector.sh
# Hermetic tests for scripts/fleet-load-detector.sh (task 5135).
#
# Fleet-wide host-oversubscription DETECTOR: observes HOST-AGGREGATE CPU
# oversubscription across ALL concurrently-dispatched orchestrator lanes by
# reading host-global /proc/loadavg (the aggregate contributed by every
# dispatched lane — no per-lane enumeration needed).
# Reify-side signal for the DF-owned L3b dispatch-admission companion
# (docs/prds/run-all-pool-contention-tiering-fix.md §9).
#
# SINGLE-AXIS since task 5985: the PSI `some avg10 >= 80` arm was DROPPED (not
# retuned) — avg10 never discriminated load regimes on this host. Block K pins
# its absence BEHAVIOURALLY (a would-have-fired PSI fixture is inert, the two
# flags are unknown, neither output channel carries an avg10= field) so the
# dead arm cannot silently return.
#
# Fully hermetic: env-injected synthetic loadavg-path/nproc; no real
# /proc reads, no CPU burn. Classified `pool` in run-all-classification.manifest.
#
# Test seams (env-controlled synthetic sources; script never touches the
# real host /proc during these tests):
#   REIFY_FLEET_LOAD_LOADAVG_PATH      — /proc/loadavg-format file (field 1 = load1)
#   REIFY_FLEET_LOAD_NPROC             — synthetic nproc (avoids reading the real host)
#   REIFY_FLEET_LOAD_RATIO_THRESHOLD   — ratio ceiling override (default 4.0)
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks (grown incrementally across task 5135's plan.json TDD steps):
#   A — CLI guard: --help, unknown flag, missing/unknown subcommand
#   B/F/C/H/J/G — happy path, boundary (>=), incident reproduction, threshold-knob
#                 override, CLI-flag parity, fail-open
#   K — negative pins: the dropped PSI avg10 arm stays dropped (task 5985)
#   I — dark-factory-orchestrator.yaml config-drift cross-check
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
# RETAINED after task 5985 dropped the avg10 arm: Block K repurposes it to build
# a would-have-fired (99.00) PSI fixture and prove the detector ignores it.
_mk_psi() {
    local _f
    _f="$(mktemp /tmp/test-fleet-load-detector-psi-XXXXXX)"
    _TMPDIRS+=("$_f")
    printf 'some avg10=%s avg60=0.50 avg300=0.10 total=12345\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' "$1" > "$_f"
    echo "$_f"
}

# ──────────────────────────────────────────────────────────────────────────────
# Block B — happy path: ratio < threshold → healthy
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: happy path (healthy) ---"

B_LOADAVG="$(_mk_loadavg 0.50)"

REIFY_FLEET_LOAD_LOADAVG_PATH="$B_LOADAVG" REIFY_FLEET_LOAD_NPROC=32 \
    run_helper check
assert "B1: healthy fixture exits 0" test "$RC" -eq 0
assert "B2: stdout is exactly the expected FLEET_LOAD verdict line" \
    bash -c '[ "$1" = "FLEET_LOAD status=ok load1=0.50 nproc=32 ratio=0.02 ratio_threshold=4.0 reason=none" ]' \
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
REIFY_FLEET_LOAD_LOADAVG_PATH="$F1_LOADAVG" REIFY_FLEET_LOAD_NPROC=32 \
    run_helper check
assert "F1: ratio exactly at threshold (4.00) is status=oversubscribed" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])status=oversubscribed([[:space:]]|$)"' _ "$OUT"
assert "F1: ratio field reads 4.00" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])ratio=4\.00([[:space:]]|$)"' _ "$OUT"

# F2: load1=127 nproc=32 → ratio 3.97 (just below 4.0) → status=ok, exit 0
F2_LOADAVG="$(_mk_loadavg 127)"
REIFY_FLEET_LOAD_LOADAVG_PATH="$F2_LOADAVG" REIFY_FLEET_LOAD_NPROC=32 \
    run_helper check
assert "F2: ratio just below threshold (3.97) is status=ok" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])status=ok([[:space:]]|$)"' _ "$OUT"
assert "F2: just-below-threshold exits 0" test "$RC" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block C — incident reproduction (ratio axis): the reference esc-4037
# loadavg~419 on 32 cores incident (ratio≈13.1×) → the sole (ratio) axis must
# flag oversubscription with the flagged-path exit code + marker.
#
# Since task 5985 dropped the avg10 arm this block also subsumes the old G2
# ("PSI unreadable, ratio high → ratio alone still fires"): with no PSI axis at
# all, every high-ratio run IS that case.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: incident reproduction (ratio axis) ---"

C_LOADAVG="$(_mk_loadavg 419)"
REIFY_FLEET_LOAD_LOADAVG_PATH="$C_LOADAVG" REIFY_FLEET_LOAD_NPROC=32 \
    run_helper check
assert "C1: incident fixture (419/32) exits 3" test "$RC" -eq 3
assert "C2: stdout status=oversubscribed" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])status=oversubscribed([[:space:]]|$)"' _ "$OUT"
assert "C3: stdout reason=ratio" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])reason=ratio$"' _ "$OUT"
assert "C4: stderr carries the @@REIFY_FLEET_OVERSUBSCRIBED@@ marker" \
    bash -c 'printf "%s\n" "$1" | grep -q "@@REIFY_FLEET_OVERSUBSCRIBED@@"' _ "$ERR_OUT"
assert "C5: marker line carries ratio=/load1=/nproc= fields" \
    bash -c 'printf "%s\n" "$1" | grep -E "@@REIFY_FLEET_OVERSUBSCRIBED@@" | grep -qE "ratio=.*load1=.*nproc="' \
    _ "$ERR_OUT"

# (Blocks D and E — "avg10 axis alone → reason=avg10" and "both axes high →
# reason=both" — were DELETED by task 5985 along with the avg10 arm itself.
# Block K below pins that those verdicts can no longer be produced.)

# ──────────────────────────────────────────────────────────────────────────────
# Block H — threshold-knob override: proves the threshold is a real runtime
# knob (env-driven), not a hard-coded constant, and that garbage input falls
# back to the documented default rather than corrupting the awk comparison.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block H: threshold-knob override ---"

# H1: incident fixture (419/32≈13.1) but a raised ratio ceiling → healthy.
H1_LOADAVG="$(_mk_loadavg 419)"
REIFY_FLEET_LOAD_LOADAVG_PATH="$H1_LOADAVG" REIFY_FLEET_LOAD_NPROC=32 \
    REIFY_FLEET_LOAD_RATIO_THRESHOLD=20 \
    run_helper check
assert "H1: raised ratio threshold (20) makes the incident fixture status=ok" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])status=ok([[:space:]]|$)"' _ "$OUT"
assert "H1: raised ratio threshold (20) exits 0" test "$RC" -eq 0

# H3: garbage REIFY_FLEET_LOAD_RATIO_THRESHOLD falls back to the documented
# default (4.0) rather than leaking into the stdout line or corrupting the
# awk comparison (an unvalidated non-numeric string can make the comparison
# behave unpredictably instead of safely defaulting).
H3_LOADAVG="$(_mk_loadavg 0.50)"
REIFY_FLEET_LOAD_LOADAVG_PATH="$H3_LOADAVG" REIFY_FLEET_LOAD_NPROC=32 \
    REIFY_FLEET_LOAD_RATIO_THRESHOLD=abc \
    run_helper check
assert "H3: garbage ratio threshold falls back to 4.0 in the stdout line" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])ratio_threshold=4\.0([[:space:]]|$)"' _ "$OUT"
assert "H3: garbage ratio threshold still reports status=ok on a healthy fixture" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])status=ok([[:space:]]|$)"' _ "$OUT"

# (H2 and H4 — the avg10-threshold override and its garbage-fallback mirror —
# were DELETED by task 5985 along with the knob itself; Block K2 pins that
# --avg10-threshold is now an unknown flag.)

# ──────────────────────────────────────────────────────────────────────────────
# Block J — CLI-flag parity: every block above drives the script exclusively
# via the REIFY_FLEET_LOAD_* env seams. This block drives the equivalent
# --flags instead, so a regression in the flag-parsing branches themselves
# (wrong shift count, swapped variable assignment, a broken missing-value
# guard) is caught — those branches are otherwise untested.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block J: CLI-flag parity ---"

# J1: --loadavg-path/--nproc/--ratio-threshold entirely via CLI flags (no env
# vars at all) on the incident fixture with a raised ratio ceiling — mirrors H1
# but through the flag-parsing branches.
J1_LOADAVG="$(_mk_loadavg 419)"
run_helper check --loadavg-path "$J1_LOADAVG" --nproc 32 --ratio-threshold 20
assert "J1: CLI-flag form (raised ratio threshold) makes incident fixture status=ok" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])status=ok([[:space:]]|$)"' _ "$OUT"
assert "J1: CLI-flag form exits 0" test "$RC" -eq 0

# (J2 — CLI parity for --avg10-threshold — was DELETED by task 5985; Block K2
# pins that the flag is now rejected rather than honoured.)

# J3: missing-value guard — `--nproc` with no following value must exit 2
# (usage error), proving the `[ $# -ge 2 ] || { err ...; exit 2; }` guard
# fires for a value-consuming flag rather than silently consuming the next
# token or falling through.
run_helper check --nproc
assert "J3: --nproc with no value exits 2" test "$RC" -eq 2

# ──────────────────────────────────────────────────────────────────────────────
# Block G — fail-open: an unreadable/unparseable measurement source must never
# spuriously flag/throttle the fleet (C-A4 philosophy, mirrors cpu-admit.sh /
# load_tolerance_lib.sh). A guaranteed-absent source path is allocated via
# `mktemp -u` (name only, file never created) — same idiom as test_cpu_admit
# .sh Cycle E / test_jobserver_canary.sh's "guaranteed-absent path".
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: fail-open (unreadable sources) ---"

G_MISSING_LOADAVG="$(mktemp -u /tmp/test-fleet-load-detector-missing-loadavg-XXXXXX)"

# G1: the sole measurement source is unreadable → exit 0, status=ok,
# reason=none, and a WARNING on stderr (never a spurious flag when the detector
# cannot measure at all). Since task 5985 dropped the avg10 arm, loadavg IS the
# only source, so this single case is the whole blind-detector contract.
REIFY_FLEET_LOAD_LOADAVG_PATH="$G_MISSING_LOADAVG" REIFY_FLEET_LOAD_NPROC=32 \
    run_helper check
assert "G1: unreadable loadavg exits 0 (fail-open)" test "$RC" -eq 0
assert "G1: unreadable loadavg is status=ok" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])status=ok([[:space:]]|$)"' _ "$OUT"
assert "G1: unreadable loadavg is reason=none" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])reason=none$"' _ "$OUT"
assert "G1: unreadable loadavg prints a WARNING on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "warning"' _ "$ERR_OUT"
assert "G1: unreadable loadavg emits no oversubscribed marker" \
    bash -c '! printf "%s\n" "$1" | grep -q "@@REIFY_FLEET_OVERSUBSCRIBED@@"' _ "$ERR_OUT"

# (G2/G3/G4 — the per-axis "one source unreadable, the other decides" matrix —
# were DELETED by task 5985: with a single axis there is no "other axis". G2's
# surviving intent (a high ratio still fires) is Block C; G1 above covers the
# fully-blind case.)

# ──────────────────────────────────────────────────────────────────────────────
# Block K — the PSI avg10 arm is DROPPED (task 5985). NEGATIVE pins: a pure
# deletion of Blocks D/E/G3/G4/H2/H4/J2 would leave nothing that notices the
# dead arm being reintroduced — the exact failure mode 5985 fixes. These assert
# the arm's ABSENCE behaviourally (a would-have-fired PSI fixture is inert,
# both flags are unknown, neither output channel carries an avg10= field),
# which is stronger and cheaper than grepping the script source.
#
# Rationale for the drop (dark_factory:3590): CPU PSI `some avg10` never
# discriminated load regimes on this 32-core host — the observed maximum across
# a load~85 and a load~190 sample was 74.1, so the 80 ceiling has never fired
# and cannot; the whole distribution slides upward with load (~1.4× separation)
# rather than spreading out, so no threshold behaves sensibly across both.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block K: the PSI avg10 arm is dropped (task 5985) ---"

# K1: a PSI fixture at avg10=99.00 — comfortably above the OLD 80 ceiling, so
# it WOULD have flagged oversubscribed — is now completely inert next to a
# healthy loadavg. This is the load-bearing pin: it fails loudly if the arm
# ever comes back, whatever form it takes.
K1_LOADAVG="$(_mk_loadavg 0.50)"
K1_PSI="$(_mk_psi 99.00)"
REIFY_FLEET_LOAD_LOADAVG_PATH="$K1_LOADAVG" REIFY_FLEET_LOAD_PSI_PATH="$K1_PSI" REIFY_FLEET_LOAD_NPROC=32 \
    run_helper check
assert "K1: would-have-fired avg10=99.00 is inert — exits 0" test "$RC" -eq 0
assert "K1: would-have-fired avg10=99.00 is inert — status=ok" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])status=ok([[:space:]]|$)"' _ "$OUT"
assert "K1: would-have-fired avg10=99.00 is inert — reason=none" \
    bash -c 'printf "%s\n" "$1" | grep -qE "(^|[[:space:]])reason=none$"' _ "$OUT"
assert "K1: would-have-fired avg10=99.00 emits no oversubscribed marker" \
    bash -c '! printf "%s\n" "$1" | grep -q "@@REIFY_FLEET_OVERSUBSCRIBED@@"' _ "$ERR_OUT"

# K4: the healthy verdict line carries no avg10=/avg10_threshold= field at all
# (asserted on the same K1 run — the stdout contract lost both terms).
assert "K4: stdout verdict line carries no avg10=/avg10_threshold= field" \
    bash -c '! printf "%s\n" "$1" | grep -qE "(^|[[:space:]])avg10(_threshold)?="' _ "$OUT"

# K2: --avg10-threshold is now an UNKNOWN flag (rejected via the pre-existing
# `-*` catch-all), not a silently-ignored one.
run_helper check --avg10-threshold 40
assert "K2: check --avg10-threshold exits 2 (unknown flag)" test "$RC" -eq 2
assert "K2: stderr names the rejected --avg10-threshold flag" \
    bash -c 'printf "%s\n" "$1" | grep -qE -- "--avg10-threshold"' _ "$ERR_OUT"

# K3: --psi-path is likewise gone (the PSI source seam no longer exists).
K3_PSI="$(_mk_psi 99.00)"
run_helper check --psi-path "$K3_PSI"
assert "K3: check --psi-path exits 2 (unknown flag)" test "$RC" -eq 2

# K5: the stderr marker line on the flagged path also lost its avg10= field
# (both output channels changed in lockstep — C5 pins what remains).
K5_LOADAVG="$(_mk_loadavg 419)"
REIFY_FLEET_LOAD_LOADAVG_PATH="$K5_LOADAVG" REIFY_FLEET_LOAD_NPROC=32 \
    run_helper check
assert "K5: oversubscribed marker line carries no avg10= field" \
    bash -c '! printf "%s\n" "$1" | grep -E "@@REIFY_FLEET_OVERSUBSCRIBED@@" | grep -q "avg10="' _ "$ERR_OUT"

# K6: CLI surface parity with Block A — the usage text must not advertise a
# flag the parser rejects.
run_helper check --help
assert "K6: check --help advertises neither --avg10-threshold nor --psi-path" \
    bash -c '! printf "%s\n" "$1" | grep -qE -- "--(avg10-threshold|psi-path)"' _ "$ERR_OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block I — dark-factory-orchestrator.yaml config-drift cross-check. Self-contained (does
# NOT enlarge tests/infra/test_cpu_governance_config.sh): mirrors that test's
# (A2) PyYAML-guarded value-drift check + (B) always-run plain-grep knob-name
# check, scoped to the new cpu_governance.fleet_load_detector sub-block.
# `enabled` is DF-consumed (not grep-checked against a reify script, exactly
# like DF_AGENT_CPU_GOVERN) — only its presence/truthiness is asserted here.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block I: dark-factory-orchestrator.yaml config-drift cross-check ---"

ORCH_YAML="$REPO_ROOT/dark-factory-orchestrator.yaml"

# (I-B) KNOB-NAME CROSS-CHECK — always runs (bash grep, no python needed)
echo "--- (I-B) knob-name cross-check (config <-> script) ---"

assert "REIFY_FLEET_LOAD_RATIO_THRESHOLD cited in dark-factory-orchestrator.yaml" \
    grep -q "REIFY_FLEET_LOAD_RATIO_THRESHOLD" "$ORCH_YAML"

assert "REIFY_FLEET_LOAD_RATIO_THRESHOLD referenced in scripts/fleet-load-detector.sh" \
    grep -q "REIFY_FLEET_LOAD_RATIO_THRESHOLD" "$SCRIPT"

assert "REIFY_FLEET_LOAD_AVG10_THRESHOLD cited in dark-factory-orchestrator.yaml" \
    grep -q "REIFY_FLEET_LOAD_AVG10_THRESHOLD" "$ORCH_YAML"

assert "REIFY_FLEET_LOAD_AVG10_THRESHOLD referenced in scripts/fleet-load-detector.sh" \
    grep -q "REIFY_FLEET_LOAD_AVG10_THRESHOLD" "$SCRIPT"

# (I-A) STRUCTURE + VALUE DRIFT — parse YAML (SKIP if PyYAML unavailable,
# mirrors test_cpu_governance_config.sh's SKIP guard)
if ! python3 -c 'import yaml' 2>/dev/null; then
    echo "SKIP: python3 'yaml' (PyYAML) not available; skipping YAML structure/value-drift assertions"
else
    echo "--- (I-A) structural + value-drift assertions via PyYAML ---"

    _FLD_PARSE_PY="$(mktemp /tmp/fleet_load_detector_config_parse_XXXXXX.py)"
    _TMPDIRS+=("$_FLD_PARSE_PY")

    cat > "$_FLD_PARSE_PY" << 'PYEOF'
"""Validate dark-factory-orchestrator.yaml cpu_governance.fleet_load_detector block (task 5135).
Usage:
  python3 <script> <orch_yaml> <check> [<script_path>]
Checks (no <script_path>):
  parse_ok                       — file parses as valid YAML (no exception)
  has_fleet_load_detector        — cpu_governance.fleet_load_detector block exists
  has_ratio_threshold            — ...ratio_threshold key present
  has_avg10_threshold            — ...avg10_threshold key present
  has_enabled                    — ...enabled key present
  enabled_truthy                 — ...enabled is truthy
Checks (with <script_path> — value-drift cross-check):
  ratio_threshold_matches_script — YAML ratio_threshold == fleet-load-detector.sh :- default
  avg10_threshold_matches_script — YAML avg10_threshold == fleet-load-detector.sh :- default
Exit 0 on pass, 1 on fail.
"""
import sys, yaml, re

orch_yaml_path = sys.argv[1]
check = sys.argv[2]

with open(orch_yaml_path) as f:
    d = yaml.safe_load(f)

if check == "parse_ok":
    # If we got here, the file parsed
    sys.exit(0)

cg = d.get("cpu_governance") or {}
fld = cg.get("fleet_load_detector")

if check == "has_fleet_load_detector":
    sys.exit(0 if fld is not None else 1)

if fld is None:
    sys.exit(1)

if check == "has_ratio_threshold":
    sys.exit(0 if "ratio_threshold" in fld else 1)

if check == "has_avg10_threshold":
    sys.exit(0 if "avg10_threshold" in fld else 1)

if check == "has_enabled":
    sys.exit(0 if "enabled" in fld else 1)

if check == "enabled_truthy":
    sys.exit(0 if fld.get("enabled") else 1)

if check == "ratio_threshold_matches_script":
    script_path = sys.argv[3]
    content = open(script_path).read()
    m = re.search(r'REIFY_FLEET_LOAD_RATIO_THRESHOLD:-([0-9.]+)', content)
    if not m:
        print(f"REIFY_FLEET_LOAD_RATIO_THRESHOLD default not found in {script_path}", file=sys.stderr)
        sys.exit(1)
    script_val = float(m.group(1))
    yaml_val = fld.get("ratio_threshold")
    if yaml_val is None or float(yaml_val) != script_val:
        print(f"Drift: YAML ratio_threshold={yaml_val} != fleet-load-detector.sh default={script_val}", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

if check == "avg10_threshold_matches_script":
    script_path = sys.argv[3]
    content = open(script_path).read()
    m = re.search(r'REIFY_FLEET_LOAD_AVG10_THRESHOLD:-([0-9.]+)', content)
    if not m:
        print(f"REIFY_FLEET_LOAD_AVG10_THRESHOLD default not found in {script_path}", file=sys.stderr)
        sys.exit(1)
    script_val = float(m.group(1))
    yaml_val = fld.get("avg10_threshold")
    if yaml_val is None or float(yaml_val) != script_val:
        print(f"Drift: YAML avg10_threshold={yaml_val} != fleet-load-detector.sh default={script_val}", file=sys.stderr)
        sys.exit(1)
    sys.exit(0)

print(f"unknown check: {check}", file=sys.stderr)
sys.exit(2)
PYEOF

    assert "dark-factory-orchestrator.yaml parses as valid YAML" \
        python3 "$_FLD_PARSE_PY" "$ORCH_YAML" parse_ok

    assert "cpu_governance.fleet_load_detector block exists" \
        python3 "$_FLD_PARSE_PY" "$ORCH_YAML" has_fleet_load_detector

    assert "fleet_load_detector.ratio_threshold key present" \
        python3 "$_FLD_PARSE_PY" "$ORCH_YAML" has_ratio_threshold

    assert "fleet_load_detector.avg10_threshold key present" \
        python3 "$_FLD_PARSE_PY" "$ORCH_YAML" has_avg10_threshold

    assert "fleet_load_detector.enabled key present" \
        python3 "$_FLD_PARSE_PY" "$ORCH_YAML" has_enabled

    assert "fleet_load_detector.enabled is truthy" \
        python3 "$_FLD_PARSE_PY" "$ORCH_YAML" enabled_truthy

    assert "REIFY_FLEET_LOAD_RATIO_THRESHOLD: YAML ratio_threshold matches fleet-load-detector.sh :-fallback (4.0)" \
        python3 "$_FLD_PARSE_PY" "$ORCH_YAML" ratio_threshold_matches_script "$SCRIPT"

    assert "REIFY_FLEET_LOAD_AVG10_THRESHOLD: YAML avg10_threshold matches fleet-load-detector.sh :-fallback (80)" \
        python3 "$_FLD_PARSE_PY" "$ORCH_YAML" avg10_threshold_matches_script "$SCRIPT"
fi

test_summary
