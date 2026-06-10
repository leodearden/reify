#!/usr/bin/env bash
# Infrastructure guard for gui/test/visual/run_find_uses_smoke.sh (task 4456).
#
# Pins the readiness-race fix (reviewer finding: flaky_test_readiness_race):
#   1. Structural — synchronous cargo build precedes the backgrounded launcher.
#   2. Structural — that cargo-build line precedes the '&' background line.
#   3. Structural — kill -0 liveness check + REIFY_SMOKE_WAIT_MS are present.
#   4. Behavioral/hermetic — launcher-death aborts early (not at deadline).
#
# Auto-discovered by tests/infra/run_all.sh (matches test_*.sh pattern).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

RUNNER="$REPO_ROOT/gui/test/visual/run_find_uses_smoke.sh"

echo "=== test_find_uses_smoke_runner: readiness-race-fix contracts ==="

# -- Contract 1: synchronous 'cargo build -p reify-gui' present ---------------
echo ""
echo "--- Contract 1: runner invokes 'cargo build -p reify-gui' (synchronous pre-build) ---"

assert "runner exists" \
    test -f "$RUNNER"

assert "runner contains 'cargo build -p reify-gui'" \
    grep -q 'cargo build -p reify-gui' "$RUNNER"

# -- Contract 2: cargo-build line precedes the backgrounded launcher ('&') ----
echo ""
echo "--- Contract 2: cargo build precedes the backgrounded launcher ('&') ---"

assert "runner contains a backgrounded launcher line (ends with '&')" \
    bash -c "grep -qE 'run-gui-dev\.sh.*[[:space:]]&[[:space:]]*$|LAUNCHER.*[[:space:]]&[[:space:]]*$|bash.*\\.sh.*[[:space:]]&[[:space:]]*$' '$RUNNER'"

assert "cargo build line precedes the background-launcher line" \
    bash -c "
        cargo_line=\$(grep -n 'cargo build -p reify-gui' '$RUNNER' | head -1 | cut -d: -f1)
        bg_line=\$(grep -nE '\\\$.*LAUNCHER.*[[:space:]]&[[:space:]]*$|\\\$.*LAUNCHER.*[[:space:]]&$|run-gui-dev\.sh.*[[:space:]]&[[:space:]]*$|run-gui-dev\.sh.*[[:space:]]&$' '$RUNNER' | head -1 | cut -d: -f1)
        [ -n \"\$cargo_line\" ] && [ -n \"\$bg_line\" ] && [ \"\$cargo_line\" -lt \"\$bg_line\" ]
    "

# -- Contract 3: kill -0 liveness check + REIFY_SMOKE_WAIT_MS present --------
echo ""
echo "--- Contract 3: kill -0 liveness check and REIFY_SMOKE_WAIT_MS present ---"

assert "runner contains 'kill -0' (launcher liveness guard)" \
    grep -q 'kill -0' "$RUNNER"

assert "runner reads REIFY_SMOKE_WAIT_MS (env-driven readiness budget)" \
    grep -q 'REIFY_SMOKE_WAIT_MS' "$RUNNER"

# -- Contract 4: behavioral — launcher-death causes early abort ---------------
echo ""
echo "--- Contract 4: launcher-death causes early non-zero exit (not a timeout hang) ---"

_t4_tmpdir=$(mktemp -d)
# shellcheck disable=SC2064
trap "rm -rf '$_t4_tmpdir'" EXIT INT TERM

# Stub launcher: exits 1 immediately.
mkdir -p "$_t4_tmpdir/bin"
cat > "$_t4_tmpdir/bin/stub_launcher.sh" <<'STUB'
#!/usr/bin/env bash
exit 1
STUB
chmod +x "$_t4_tmpdir/bin/stub_launcher.sh"

# Stub node: should not be reached; exits 1 if called.
cat > "$_t4_tmpdir/bin/node" <<'NODE_STUB'
#!/usr/bin/env bash
echo "STUB_ERROR: node driver should not be reached when launcher dies" >&2
exit 1
NODE_STUB
chmod +x "$_t4_tmpdir/bin/node"

# Run the runner with:
#   REIFY_SMOKE_SKIP_PREBUILD=1  (skip cargo/npm build steps)
#   REIFY_SMOKE_LAUNCHER=<stub>  (stub launcher that exits 1 immediately)
#   REIFY_SMOKE_WAIT_MS=600000   (6-minute budget — runner must abort FAR sooner)
#   REIFY_DEBUG_PORT=59999       (valid port, so resolve_port doesn't try to allocate)
#   DISPLAY=:99                  (dummy display, must not actually open a window)
#
# The runner should exit non-zero well within 15 seconds, proving the liveness
# guard detected launcher death and aborted instead of waiting the full 600s budget.

_t4_start=$SECONDS
_t4_rc=0
_t4_out=$(
    REIFY_SMOKE_SKIP_PREBUILD=1 \
    REIFY_SMOKE_LAUNCHER="$_t4_tmpdir/bin/stub_launcher.sh" \
    REIFY_SMOKE_WAIT_MS=600000 \
    REIFY_DEBUG_PORT=59999 \
    DISPLAY=:99 \
    PATH="$_t4_tmpdir/bin:$PATH" \
    bash "$RUNNER" 2>&1
) || _t4_rc=$?
_t4_elapsed=$(( SECONDS - _t4_start ))

assert "runner exits non-zero when launcher dies immediately" \
    bash -c '[ "$1" -ne 0 ]' _ "$_t4_rc"

assert "runner aborts within 15s (liveness guard, not 600s deadline)" \
    bash -c '[ "$1" -lt 15 ]' _ "$_t4_elapsed"

assert "runner emits a message about launcher death or early exit" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "launcher|exited|early|died|liveness|kill"' _ "$_t4_out"

test_summary
