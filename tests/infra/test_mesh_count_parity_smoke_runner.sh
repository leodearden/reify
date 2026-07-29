#!/usr/bin/env bash
# Infrastructure guard for gui/test/visual/run_mesh_count_parity_e2e_smoke.sh
# (task 5367).
#
# Pins the launcher-death liveness contract *behaviorally*: it runs the real
# runner with a launcher stub that dies immediately and asserts the liveness
# guard aborts early (non-zero, fast) rather than blocking until the full
# readiness deadline.
#
# NOTE: deliberately NO source-text/grep assertions. Grepping the runner for
# literal fragments (`kill -0`, `REIFY_SMOKE_WAIT_MS`, the `&` launcher line)
# matches the runner's own header COMMENTS as well as its executable code, so a
# regression that deletes the liveness logic but leaves the descriptive comment
# would keep such contracts green — passing on the very failure they claim to
# pin. The behavioral contract below is the only one that proves behavior.
# (Rationale carried over verbatim from tests/infra/test_find_uses_smoke_runner.sh.)
#
# ── SCOPE AFTER THE 5596 CONSOLIDATION ───────────────────────────────────────
# The lifecycle this file drives no longer lives in the runner: task 5596
# extracted it into gui/test/visual/lib_e2e_smoke.sh, shared by all six e2e
# smoke runners.  Two things follow.
#
# 1. The assertions below are deliberately left UNCHANGED.  The shared library
#    retains the exact `launcher exited early (rc=` substring this file matches
#    with `grep -qF`, so this guard passing byte-for-byte unmodified is the
#    no-regression evidence for the extraction — evidence that editing it to
#    match a new string could not have produced.  Do not "modernize" these
#    assertions to the newer E2E_SMOKE_LAUNCHER_DEATH marker; that would throw
#    away the only unmodified-witness this consolidation has.
#
# 2. This file stays the 5367-SPECIFIC guard rather than growing into a
#    lockstep duplicate of its sibling.  The runner-generic contract for the
#    shared lifecycle — all six runners, the negative-control arm proving the
#    death predicate is not vacuous, and the post-driver post-mortem — lives in
#    tests/infra/test_find_uses_smoke_runner.sh.  New lifecycle-wide contracts
#    belong there, not here.
#
# Auto-discovered by tests/infra/run_all.sh (matches test_*.sh pattern), and
# separately required to appear in tests/infra/run-all-classification.manifest.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

RUNNER="$REPO_ROOT/gui/test/visual/run_mesh_count_parity_e2e_smoke.sh"

echo "=== test_mesh_count_parity_smoke_runner: launcher-liveness contract ==="

assert "runner exists" \
    test -f "$RUNNER"

# -- Behavioral contract: launcher-death causes early abort -------------------
echo ""
echo "--- Contract: launcher-death causes early non-zero exit (not a timeout hang) ---"

_mcp_tmpdir=$(mktemp -d)
# shellcheck disable=SC2064
trap "rm -rf '$_mcp_tmpdir'" EXIT INT TERM

# Stub launcher: exits 1 immediately.
mkdir -p "$_mcp_tmpdir/bin"
cat > "$_mcp_tmpdir/bin/stub_launcher.sh" <<'STUB'
#!/usr/bin/env bash
exit 1
STUB
chmod +x "$_mcp_tmpdir/bin/stub_launcher.sh"

# Stub node: should not be reached; exits 1 if called.
cat > "$_mcp_tmpdir/bin/node" <<'NODE_STUB'
#!/usr/bin/env bash
echo "STUB_ERROR: node driver should not be reached when launcher dies" >&2
exit 1
NODE_STUB
chmod +x "$_mcp_tmpdir/bin/node"

# Run the runner with:
#   REIFY_SMOKE_SKIP_PREBUILD=1  (skip cargo/npm build steps)
#   REIFY_SMOKE_LAUNCHER=<stub>  (stub launcher that exits 1 immediately)
#   REIFY_SMOKE_WAIT_MS=600000   (6-minute budget — runner must abort FAR sooner)
#   REIFY_DEBUG_PORT=59999       (valid port, so resolve_port doesn't try to allocate)
#   DISPLAY=:99                  (dummy display, must not actually open a window)
#
# The runner should exit non-zero well within 15 seconds, proving the liveness
# guard detected launcher death and aborted instead of waiting the full 600s budget.

_mcp_start=$SECONDS
_mcp_rc=0
_mcp_out=$(
    REIFY_SMOKE_SKIP_PREBUILD=1 \
    REIFY_SMOKE_LAUNCHER="$_mcp_tmpdir/bin/stub_launcher.sh" \
    REIFY_SMOKE_WAIT_MS=600000 \
    REIFY_DEBUG_PORT=59999 \
    DISPLAY=:99 \
    PATH="$_mcp_tmpdir/bin:$PATH" \
    bash "$RUNNER" 2>&1
) || _mcp_rc=$?
_mcp_elapsed=$(( SECONDS - _mcp_start ))

assert "runner exits non-zero when launcher dies immediately" \
    bash -c '[ "$1" -ne 0 ]' _ "$_mcp_rc"

assert "runner aborts within 15s (liveness guard, not 600s deadline)" \
    bash -c '[ "$1" -lt 15 ]' _ "$_mcp_elapsed" # wallclock:allow — liveness: discriminated by rc!=0 + launcher-death message, not by elapsed magnitude

# Match the liveness guard's OWN diagnostic, not a broad alternation. The runner
# unconditionally announces `launcher PID=<pid>` before the readiness loop is even
# entered, so a pattern with a bare `launcher` branch matches on EVERY run —
# including one where the liveness guard was deleted outright. Only the
# abort-on-launcher-death path prints this string, so it is the one that
# discriminates.
assert "runner emits the launcher-death diagnostic (not just its startup banner)" \
    bash -c 'printf "%s\n" "$1" | grep -qF "launcher exited early (rc="' _ "$_mcp_out"

test_summary
