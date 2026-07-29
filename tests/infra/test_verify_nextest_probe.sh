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
# Hermeticity: tests/infra/nextest_absent_lib.sh (task 5602). HOME is a temp dir
# so verify.sh:626 skips `. ~/.cargo/env` (which would re-prepend ~/.cargo/bin —
# the sole home of the real cargo-nextest — and shadow the shim), and PATH leads
# with a symlink farm mirroring the cargo bin dir MINUS cargo-nextest, with that
# directory filtered out of the inherited PATH. The counter-driven `cargo` shim
# is OVERLAID into that farm, and cargo-nextest's presence is toggled by the
# lib's add/remove pair — so this suite's two states (present for cycles i, ii,
# iv, v; absent for iii) are PARAMETERS of the shared harness rather than a
# reason to fork it.
#
# The shim `cargo` passes any non-`nextest` subcommand through to the real cargo
# (captured from the AMBIENT PATH before the harness is built) so incidental
# non-nextest cargo calls during plan-building would keep working — though
# --scope all never reaches the narrowing path that would call `cargo metadata`
# anyway.
#
# The farm — rather than the naive PATH="$SHIM_DIR:/usr/bin:/bin" this suite
# used before consolidation — also keeps the REST of the toolchain resolvable
# (notably tree-sitter, which lives in ~/.cargo/bin next to cargo-nextest), and
# carries RUSTUP_HOME across so the redirected HOME does not strand the rustup
# shim into downloading a fresh toolchain. See the lib's header.
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

[ -f "$SCRIPT_DIR/nextest_absent_lib.sh" ] || {
    echo "ERROR: nextest_absent_lib.sh not found at $SCRIPT_DIR/nextest_absent_lib.sh"
    exit 1
}
# shellcheck source=tests/infra/nextest_absent_lib.sh
source "$SCRIPT_DIR/nextest_absent_lib.sh"

echo "=== verify.sh nextest-probe retry/hard-fail drift-guard (task 4971) ==="

# Capture the REAL cargo's absolute path from the AMBIENT PATH, BEFORE the
# harness below rewrites it — the shim's pass-through branch execs this
# directly. Must precede nextest_absent_init for the same reason it used to
# precede the hand-rolled PATH assignment.
_REAL_CARGO="$(command -v cargo)" || {
    echo "ERROR: real cargo not found on the ambient PATH"
    exit 1
}

# Builds the farm + temp HOME and registers the cleanup trap (EXIT/INT/TERM/HUP
# — a strict superset of the bare EXIT trap this suite used to install). This is
# the only trap in this file, so there is nothing to compose with.
nextest_absent_init

# Inside the lib's workdir, so the same trap removes it.
COUNTER_FILE="$NX_WORKDIR/nextest_probe_counter"

# ---------------------------------------------------------------------------
# Harness helpers
# ---------------------------------------------------------------------------

# reset_counter — clears the shim's probe-attempt counter so the next
# verify.sh invocation starts counting from 0 again.
reset_counter() {
    rm -f "$COUNTER_FILE"
}

# make_cargo_shim — OVERLAYS a counter-driven `cargo` wrapper into the harness
# farm, where it shadows both the filtered cargo bin dir and any other cargo
# later in PATH (the farm is first). When $1 == "nextest": increments
# COUNTER_FILE; while the resulting count is <= ${REIFY_SHIM_FAIL_COUNT:-0} the
# probe FAILS (stderr marker + exit 1); once the count exceeds the threshold it
# SUCCEEDS (prints a version line, exit 0) — and keeps succeeding on every later
# attempt (the counter only grows). Any other subcommand is passed straight
# through to the real cargo (the $_REAL_CARGO absolute path captured above) so
# incidental non-nextest cargo calls keep working. Static content — call once;
# REIFY_SHIM_FAIL_COUNT is read at shim-RUNTIME, not baked in at creation time,
# so a single shim instance is reused (with reset_counter) across every cycle.
#
# nextest_absent_farm_put removes the existing farm entry before installing,
# which matters here: `cargo` is already in the farm as a mirrored SYMLINK, and
# writing through it would clobber the real binary it points at.
make_cargo_shim() {
    local _shim="$NX_WORKDIR/cargo-shim"
    cat > "$_shim" <<SHIMEOF
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
    nextest_absent_farm_put cargo "$_shim"
}

# The cargo-nextest presence marker is the lib's
# nextest_absent_farm_add_nextest_stub / nextest_absent_farm_rm_nextest_stub
# pair, used directly at the cycle call sites below. Adding it makes
# `command -v cargo-nextest` succeed (binary genuinely present on PATH);
# removing it restores genuine absence for the regression-guard cycle (iii).
# verify.sh's probe never execs the marker directly — it goes through the
# `cargo` wrapper's nextest-subcommand branch above — so its mere presence is
# what the fix's genuine-absence disambiguation at scripts/verify.sh:1412
# checks, which is exactly what the lib's marker provides.

# run_verify [VAR=val ...] -- <verify.sh args...>
# Drives REAL scripts/verify.sh under the harness (nx_run: farm-first PATH,
# temp HOME, CARGO_HOME unset, RUSTUP_HOME carried across).
# DF_VERIFY_ROLE defaults to 'task' (a gate role) and
# REIFY_NEXTEST_PROBE_RETRY_SLEEP defaults to 0 (no wall-clock cost); both can
# be overridden by a later VAR=val in the caller's env-args (env applies
# repeated assignments left-to-right, last wins — and nx_run passes leading
# VAR=val through to the same env(1) invocation, so that ordering still holds
# across the harness's own assignments). Sets globals:
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
    _stdout_file="$(mktemp -p "$NX_WORKDIR" verify-stdout.XXXXXX)"
    _stderr_file="$(mktemp -p "$NX_WORKDIR" verify-stderr.XXXXXX)"

    VERIFY_RC=0
    nx_run DF_VERIFY_ROLE=task REIFY_NEXTEST_PROBE_RETRY_SLEEP=0 \
        "${env_args[@]+"${env_args[@]}"}" \
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
nextest_absent_farm_add_nextest_stub
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
nextest_absent_farm_add_nextest_stub
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
nextest_absent_farm_rm_nextest_stub
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
nextest_absent_farm_add_nextest_stub
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
nextest_absent_farm_add_nextest_stub
run_verify REIFY_SHIM_FAIL_COUNT=4 -- test --scope all --print-plan

assert "(v): verify.sh exits NON-ZERO (retry budget exhausted one probe short)" \
    test "$VERIFY_RC" -ne 0
assert "(v): diagnostic mentions the probe failed persistently" \
    _combined_out_has "failed persistently"

test_summary
