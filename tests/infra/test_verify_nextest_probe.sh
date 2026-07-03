#!/usr/bin/env bash
# tests/infra/test_verify_nextest_probe.sh — drift-guard for scripts/verify.sh's
# cargo-nextest availability probe (task 4971 / esc-4959-57).
#
# ROOT CAUSE (base branch): the nextest probe at scripts/verify.sh:907-910 runs
# `cargo nextest --version` exactly ONCE. Any non-zero rc — including a
# transient fork/exec failure under host-pressure — silently sets NEXTEST=0
# and emits the `-E`-less cargo-test fallback plan, even when cargo-nextest is
# genuinely installed. On gate roles (task/merge) with REIFY_GATE_EXCLUDE_HEAVY=1
# this silently drops the `-E "not (<heavy>)"` fragment, breaking plan-oracle
# infra tests (test_verify_offline_partition.sh assertion (b) and siblings).
#
# FIX: disambiguate "genuinely absent" (`command -v cargo-nextest` also fails —
# keep the graceful cargo-test fallback) from "present but probe failed" (retry
# up to 3x, then hard-fail loudly rather than silently downgrade the plan).
#
# This suite drives REAL scripts/verify.sh test --scope all --print-plan under
# a hermetic PATH-shim `cargo` wrapper (counter-driven fail-then-succeed) plus a
# stub `cargo-nextest` presence marker, across five cycles:
#   (i)   probe fails N<3 times then succeeds (cargo-nextest present) ->
#         verify.sh exits 0, plan header shows nextest=1.
#   (ii)  probe fails persistently, cargo-nextest present -> verify.sh exits
#         NON-ZERO with a diagnostic naming cargo-nextest + the probe.
#   (iii) cargo-nextest genuinely absent -> fallback plan unchanged: exit 0,
#         nextest=0, no `-E "not (` fragment (regression guard).
#   (iv)  boundary: fail-count=3 recovers on the LAST permitted retry ->
#         exit 0, nextest=1 (pins the retry-loop's `-lt 3` bound from below).
#   (v)   boundary: fail-count=4 fails one probe past the retry budget (1
#         initial probe + 3 retries = 4 total) -> hard-fail (pins the bound
#         from above). (iv)/(v) together catch an off-by-one in either
#         direction that cycles (i) (N=2) and (ii) (N=999) don't exercise.
#
# Hermeticity: HOME is a temp dir so verify.sh:626 skips `. ~/.cargo/env`
# (which would re-prepend ~/.cargo/bin — the sole home of the real
# cargo-nextest — and shadow the shim); PATH="$SHIM_DIR:/usr/bin:/bin"
# excludes ~/.cargo/bin. The shim `cargo` passes any non-`nextest` subcommand
# through to the real cargo (captured before PATH is restricted) so incidental
# non-nextest cargo calls during plan-building would keep working — though
# --scope all never reaches the narrowing path that would call `cargo
# metadata` anyway.
#
# Modeled on tests/infra/test_agent_cargo_shim.sh (stub-cargo / hermetic-PATH /
# counter-file idiom) and tests/infra/test_verify_offline_partition.sh
# (--print-plan oracle driver + header / `-E "not ("` fixed-string assertions).
#
# Compile-free — this test never invokes a real cargo build/test (only
# verify.sh --print-plan, which is pure bash string-building, and the shim's
# own trivial version/marker output).
#
# Only --print-plan is exercised below; execute-mode invocations are covered
# by proxy, not by a separate cycle, because the probe/retry/hard-fail block
# (scripts/verify.sh's "Scope note re: --print-plan hermeticity") runs the
# identical, mode-agnostic code path in both modes.
#
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh); registered in
# tests/infra/run-all-classification.manifest (pool bucket).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== verify.sh nextest-probe retry/hard-fail drift-guard (task 4971) ==="

# Capture the REAL cargo's absolute path BEFORE PATH is restricted to the
# hermetic shim dir below — the shim's pass-through branch execs this directly.
_REAL_CARGO="$(command -v cargo)" || {
    echo "ERROR: real cargo not found on the ambient PATH"
    exit 1
}

WORKDIR="$(mktemp -d)"
SHIM_DIR="$WORKDIR/shim-bin"
TMP_HOME="$WORKDIR/home"
mkdir -p "$SHIM_DIR" "$TMP_HOME"
trap 'rm -rf "$WORKDIR"' EXIT

COUNTER_FILE="$WORKDIR/nextest_probe_counter"

# ---------------------------------------------------------------------------
# Harness helpers
# ---------------------------------------------------------------------------

# reset_counter — clears the shim's probe-attempt counter so the next
# verify.sh invocation starts counting from 0 again.
reset_counter() {
    rm -f "$COUNTER_FILE"
}

# make_cargo_shim — writes a counter-driven `cargo` wrapper into SHIM_DIR.
# When $1 == "nextest": increments COUNTER_FILE; while the resulting count is
# <= ${REIFY_SHIM_FAIL_COUNT:-0} the probe FAILS (stderr marker + exit 1);
# once the count exceeds the threshold it SUCCEEDS (prints a version line,
# exit 0) — and keeps succeeding on every later attempt (the counter only
# grows). Any other subcommand is passed straight through to the real cargo
# (the $_REAL_CARGO absolute path captured above) so incidental non-nextest
# cargo calls keep working. Static content — call once; REIFY_SHIM_FAIL_COUNT
# is read at shim-RUNTIME, not baked in at creation time, so a single shim
# instance is reused (with reset_counter) across every cycle.
make_cargo_shim() {
    cat > "$SHIM_DIR/cargo" <<SHIMEOF
#!/usr/bin/env bash
if [ "\$1" = "nextest" ]; then
    _count_file="$COUNTER_FILE"
    _count=0
    [ -f "\$_count_file" ] && _count="\$(cat "\$_count_file")"
    _count=\$((_count + 1))
    echo "\$_count" > "\$_count_file"
    if [ "\$_count" -le "\${REIFY_SHIM_FAIL_COUNT:-0}" ]; then
        echo "SHIM_CARGO_NEXTEST_PROBE_FAIL attempt=\$_count" >&2
        exit 1
    fi
    echo "cargo-nextest 0.0.0-shim (attempt \$_count)"
    exit 0
fi
exec "$_REAL_CARGO" "\$@"
SHIMEOF
    chmod +x "$SHIM_DIR/cargo"
}

# make_cargo_nextest_stub — writes an executable presence marker at
# SHIM_DIR/cargo-nextest so `command -v cargo-nextest` succeeds (binary
# genuinely present on PATH). verify.sh's probe never execs this directly (it
# goes through the `cargo` wrapper's nextest-subcommand branch above) — its
# mere presence is what the fix's genuine-absence disambiguation checks.
make_cargo_nextest_stub() {
    cat > "$SHIM_DIR/cargo-nextest" <<'STUBEOF'
#!/usr/bin/env bash
echo "cargo-nextest 0.0.0-shim"
exit 0
STUBEOF
    chmod +x "$SHIM_DIR/cargo-nextest"
}

# remove_cargo_nextest_stub — removes SHIM_DIR/cargo-nextest so
# `command -v cargo-nextest` fails (genuine absence from PATH), for the
# regression-guard cycle (iii).
remove_cargo_nextest_stub() {
    rm -f "$SHIM_DIR/cargo-nextest"
}

# run_verify [VAR=val ...] -- <verify.sh args...>
# Drives REAL scripts/verify.sh under the hermetic shim PATH + temp HOME.
# DF_VERIFY_ROLE defaults to 'task' (a gate role) and
# REIFY_NEXTEST_PROBE_RETRY_SLEEP defaults to 0 (no wall-clock cost); both can
# be overridden by a later VAR=val in the caller's env-args (env applies
# repeated assignments left-to-right, last wins). Sets globals:
#   VERIFY_RC      — exit code
#   VERIFY_STDOUT  — captured stdout
#   VERIFY_STDERR  — captured stderr
VERIFY_RC=0
VERIFY_STDOUT=""
VERIFY_STDERR=""

run_verify() {
    local env_args=()
    while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do
        env_args+=("$1"); shift
    done
    [ "$#" -gt 0 ] && shift  # consume the --

    local _stdout_file _stderr_file
    _stdout_file="$(mktemp -p "$WORKDIR" verify-stdout.XXXXXX)"
    _stderr_file="$(mktemp -p "$WORKDIR" verify-stderr.XXXXXX)"

    VERIFY_RC=0
    env DF_VERIFY_ROLE=task REIFY_NEXTEST_PROBE_RETRY_SLEEP=0 \
        "${env_args[@]}" \
        HOME="$TMP_HOME" \
        PATH="$SHIM_DIR:/usr/bin:/bin" \
        bash "$REPO_ROOT/scripts/verify.sh" "$@" \
        >"$_stdout_file" \
        2>"$_stderr_file" \
        || VERIFY_RC=$?

    VERIFY_STDOUT="$(cat "$_stdout_file")"
    VERIFY_STDERR="$(cat "$_stderr_file")"
    rm -f "$_stdout_file" "$_stderr_file"
}

# _plan_header_has <needle> — 0 iff the captured VERIFY_STDOUT's
# `# verify.sh plan` header line contains <needle> (fixed string).
_plan_header_has() {
    local _header
    _header="$(printf '%s\n' "$VERIFY_STDOUT" | grep '^# verify.sh plan')" || return 1
    case "$_header" in
        *"$1"*) return 0 ;;
        *) return 1 ;;
    esac
}

# _plan_lacks <needle> — 0 iff the captured VERIFY_STDOUT (the full
# --print-plan output: header + commands) does NOT contain <needle> (fixed
# string). Mirrors test_verify_offline_partition.sh's _offline_lacks.
_plan_lacks() {
    ! printf '%s\n' "$VERIFY_STDOUT" | grep -qF -- "$1"
}

# _combined_out_has <needle> — 0 iff <needle> (fixed string) appears anywhere
# in the captured VERIFY_STDOUT or VERIFY_STDERR. A hard-fail diagnostic could
# reasonably land on either stream, so the persistent-failure cycle checks
# both at once rather than assuming one.
_combined_out_has() {
    local _combined="$VERIFY_STDOUT
$VERIFY_STDERR"
    case "$_combined" in
        *"$1"*) return 0 ;;
        *) return 1 ;;
    esac
}

make_cargo_shim

# ---------------------------------------------------------------------------
# Cycle (i): transient probe failure (N=2 < 3 retries) recovers -> nextest=1.
# RED on base branch: verify.sh probes cargo-nextest ONCE, sees the first
# (transient) failure, and sets NEXTEST=0 with no retry — the plan header
# shows nextest=0, so the nextest=1 assertion below fails.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle (i): transient failure (N=2 < 3 retries) recovers -> nextest=1 ---"

reset_counter
make_cargo_nextest_stub
run_verify REIFY_SHIM_FAIL_COUNT=2 -- test --scope all --print-plan

assert "(i): verify.sh exits 0 (recovered within the retry budget)" \
    test "$VERIFY_RC" -eq 0
assert "(i): plan header shows nextest=1 (probe recovered after 2 transient failures)" \
    _plan_header_has "nextest=1"

# ---------------------------------------------------------------------------
# Cycle (ii): persistent probe failure (cargo-nextest present) hard-fails.
# RED against step-2 code: the retry loop exhausts its 3 attempts, then falls
# through with NEXTEST still 0 and no hard-fail — verify.sh exits 0 via the
# (wrong, for this case) cargo-test fallback, so the rc!=0 assertion fails and
# no diagnostic is ever emitted.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle (ii): persistent probe failure hard-fails (cargo-nextest present) ---"

reset_counter
make_cargo_nextest_stub
run_verify REIFY_SHIM_FAIL_COUNT=999 -- test --scope all --print-plan

assert "(ii): verify.sh exits NON-ZERO (refuses to silently downgrade the plan)" \
    test "$VERIFY_RC" -ne 0
assert "(ii): diagnostic mentions cargo-nextest" \
    _combined_out_has "cargo-nextest"
# Tightened per review: a bare "probe" substring would also match an
# unrelated cargo/plan error that happens to mention the word. "failed
# persistently" is the fixed fragment from the hard-fail branch itself
# (scripts/verify.sh's ERROR message), so this ties the assertion to the
# intended code path without pinning the full message wording.
assert "(ii): diagnostic mentions the probe failed persistently" \
    _combined_out_has "failed persistently"

# ---------------------------------------------------------------------------
# Cycle (iii): cargo-nextest genuinely absent -> fallback plan unchanged.
# Regression guard: GREEN already against step-2 code (this path was never
# touched — genuine absence still short-circuits before the retry branch).
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle (iii): cargo-nextest genuinely absent -> fallback plan unchanged ---"

reset_counter
remove_cargo_nextest_stub
run_verify REIFY_SHIM_FAIL_COUNT=999 REIFY_GATE_EXCLUDE_HEAVY=1 DF_VERIFY_ROLE=task -- \
    test --scope all --print-plan

assert "(iii): verify.sh exits 0 (genuine absence keeps the graceful fallback)" \
    test "$VERIFY_RC" -eq 0
assert "(iii): plan header shows nextest=0" \
    _plan_header_has "nextest=0"
assert '(iii): plan has NO -E "not (" fragment (cargo-test fallback has no -E support)' \
    _plan_lacks '-E "not ('

# ---------------------------------------------------------------------------
# Cycle (iv): boundary retry recovery — REIFY_SHIM_FAIL_COUNT=3 recovers on
# the LAST permitted retry (the 3rd). Pins the retry-loop's `-lt 3` bound at
# scripts/verify.sh:929/937 from below: a loosened bound (e.g. `-lt 2`) would
# stop retrying one probe early and hard-fail here instead of recovering.
# Neither cycle (i) (N=2, recovers a full retry early) nor cycle (ii) (N=999,
# fails far past the bound) exercises this exact boundary.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle (iv): boundary recovery on the final permitted retry (fail-count=3) ---"

reset_counter
make_cargo_nextest_stub
run_verify REIFY_SHIM_FAIL_COUNT=3 -- test --scope all --print-plan

assert "(iv): verify.sh exits 0 (recovers exactly on the 3rd/final retry)" \
    test "$VERIFY_RC" -eq 0
assert "(iv): plan header shows nextest=1" \
    _plan_header_has "nextest=1"

# ---------------------------------------------------------------------------
# Cycle (v): boundary hard-fail — REIFY_SHIM_FAIL_COUNT=4 fails one probe past
# the retry budget (1 initial probe + 3 retries = 4 total probes), so the
# would-be-recovering probe never happens and verify.sh must hard-fail. Pins
# the `-lt 3` bound from above: a loosened bound (e.g. `-lt 4`) would let this
# recover on a 5th probe instead of hard-failing.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle (v): boundary hard-fail one probe past the retry budget (fail-count=4) ---"

reset_counter
make_cargo_nextest_stub
run_verify REIFY_SHIM_FAIL_COUNT=4 -- test --scope all --print-plan

assert "(v): verify.sh exits NON-ZERO (retry budget exhausted one probe short)" \
    test "$VERIFY_RC" -ne 0
assert "(v): diagnostic mentions the probe failed persistently" \
    _combined_out_has "failed persistently"

test_summary
