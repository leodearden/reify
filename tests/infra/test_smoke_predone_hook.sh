#!/usr/bin/env bash
# tests/infra/test_smoke_predone_hook.sh
#
# Regression guard: asserts that scripts/smoke-predone-hook.sh can actually
# DISCRIMINATE the two historical pre-done hook wirings.
#
# Background: FUSED_MEMORY_PREDONE_HOOK_REIFY must route through
# scripts/reify-audit-predone-wrapper.sh, because only the wrapper runs the
# REFUSE-mode freshness guard (scripts/reify-audit-freshness.sh) and
# materializes the TaskMetadata snapshot from the fused-memory MCP. Pointing
# the env var straight at the raw reify-audit binary silently bypasses both.
# That drift went unrecorded for ~3 months precisely because the smoke test was
# green under BOTH wirings: it asserted the first token was executable and
# carried the template tokens, but never that it was the wrapper.
#
# This suite is the hermetic complement to scripts/smoke-predone-hook.sh
# itself: the smoke script needs a live systemd user session, a running
# fused-memory MCP on :8002 and an installed binary, so it can only ever be an
# on-host operator check and is NOT referenced by scripts/verify.sh. This test
# PATH-stubs systemctl (and the hook target) so the smoke script's LOGIC is
# gated on every merge, on any host.
#
# See: docs/architecture-audit/f-infra-design.md §11.1, §11.1.3
#      task 3731 (root-cause: dead .taskmaster/tasks/tasks.json default)
#      task 6345 (drift measurement), task 6939 (idempotent deploy script)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

SMOKE="$REPO_ROOT/scripts/smoke-predone-hook.sh"

# The exact substring the wrapper-identity assertion (2.6) must emit on
# failure. Pinned here so a reworded diagnostic cannot silently un-gate this
# suite.
WRAPPER_FAIL_MARKER="hook first token is not the pre-done wrapper"

echo "=== smoke-predone-hook.sh wiring-discrimination guard ==="

TMPROOT="$(mktemp -d "${TMPDIR:-/tmp}/test-smoke-predone-XXXXXX")"
trap 'rm -rf "$TMPROOT"' EXIT

STUB_DIR="$TMPROOT/stubs"
mkdir -p "$STUB_DIR"

# ── systemctl stub ────────────────────────────────────────────────────────────
# Lifted in shape from tests/infra/test_agent_cache_redirect.sh's
# make_systemctl_stub(). Reproduces systemd's REAL `show --property=Environment`
# output shape: values containing spaces are DOUBLE-QUOTED, and other env vars
# share the line. The smoke script's parser tries the quoted form FIRST, so a
# stub that omitted the quotes would exercise the wrong parse path.
make_systemctl_stub() {
    cat > "$STUB_DIR/systemctl" << 'STUB_EOF'
#!/usr/bin/env bash
echo "systemctl $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
for _arg in "$@"; do
    if [ "$_arg" = "--property=Environment" ]; then
        printf 'Environment=PATH=/usr/bin PYTHONUNBUFFERED=1 "%s=%s"\n' \
            "FUSED_MEMORY_PREDONE_HOOK_REIFY" "${REIFY_TEST_HOOK_ENV_VALUE:-}"
        exit 0
    fi
done
exit 0
STUB_EOF
    chmod +x "$STUB_DIR/systemctl"
}
make_systemctl_stub

# ── hook-target stubs ─────────────────────────────────────────────────────────
# Both wirings get an executable stub under $STUB_DIR carrying the BASENAME the
# real thing would have, so pre-existing assertion 2 (executable + survives
# --help) passes in both controls. The only property that then differs between
# the negative and positive control is the basename itself -- which is exactly
# what assertion 2.6 must key on.
make_hook_target_stub() {
    local _name="$1"
    cat > "$STUB_DIR/$_name" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$0 $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
    chmod +x "$STUB_DIR/$_name"
}
make_hook_target_stub "reify-audit"                      # historical raw-binary wiring
make_hook_target_stub "reify-audit-predone-wrapper.sh"   # correct wrapper wiring

# ── runner ────────────────────────────────────────────────────────────────────
# Sets the globals SMOKE_OUT / SMOKE_RC. Called as a bare command (never inside
# a command substitution) so the assignments survive into the parent shell,
# matching test_helpers.sh's no-subshell assert contract.
run_smoke() {
    local _env_value="$1"
    local _out_file="$TMPROOT/smoke.out"
    local rc=0
    env \
        REIFY_TEST_HOOK_ENV_VALUE="$_env_value" \
        REIFY_TEST_CALLS_FILE="$TMPROOT/calls.log" \
        PATH="$STUB_DIR:$PATH" \
        bash "$SMOKE" >"$_out_file" 2>&1 || rc=$?
    SMOKE_OUT="$(cat "$_out_file")"
    SMOKE_RC=$rc
}

# ==============================================================================
# Check 0: the smoke script exists and is readable
# ==============================================================================
echo ""
echo "--- Check 0: smoke script present ---"

assert "scripts/smoke-predone-hook.sh exists" \
    bash -c '[ -f "$1" ]' -- "$SMOKE"

# ==============================================================================
# Check 1: NEGATIVE CONTROL -- historical raw-binary wiring must be REFUSED
#
# This is the wiring that shipped live for ~3 months. It bypasses the wrapper's
# REFUSE-mode freshness guard entirely, so the smoke test must reject it.
# ==============================================================================
echo ""
echo "--- Check 1: raw-binary wiring is refused (negative control) ---"

run_smoke "$STUB_DIR/reify-audit --task {id} --pre-done"

assert "raw-binary wiring exits non-zero" \
    bash -c '[ "$1" -ne 0 ]' -- "$SMOKE_RC"

assert "raw-binary wiring failure names reify-audit-predone-wrapper.sh" \
    bash -c 'printf %s "$1" | grep -q "reify-audit-predone-wrapper.sh"' -- "$SMOKE_OUT"

assert "raw-binary wiring emits the wrapper-identity diagnostic" \
    bash -c 'printf %s "$1" | grep -qF "$2"' -- "$SMOKE_OUT" "$WRAPPER_FAIL_MARKER"

# Fail-fast property: a mis-wired host must be rejected BEFORE the network
# probe, so the diagnostic is not buried behind an unrelated connection error.
assert "raw-binary wiring fails before the MCP probe" \
    bash -c '! printf %s "$1" | grep -q "probing MCP endpoint"' -- "$SMOKE_OUT"

# ==============================================================================
# Check 2: POSITIVE CONTROL -- correct wrapper wiring must NOT be refused
#
# Guards against a vacuous check-1 (an assertion that rejects everything).
# Asserts progress PAST the new wrapper-identity check rather than overall
# exit 0: assertion 3 (MCP probe) and assertion 4 (fixture round-trips) are not
# stubbed at this point, so the run legitimately stops later on.
# ==============================================================================
echo ""
echo "--- Check 2: wrapper wiring passes the identity check (positive control) ---"

run_smoke "$STUB_DIR/reify-audit-predone-wrapper.sh --task {id} --pre-done"

assert "wrapper wiring does NOT emit the wrapper-identity diagnostic" \
    bash -c '! printf %s "$1" | grep -qF "$2"' -- "$SMOKE_OUT" "$WRAPPER_FAIL_MARKER"

assert "wrapper wiring reaches the MCP probe" \
    bash -c 'printf %s "$1" | grep -q "probing MCP endpoint"' -- "$SMOKE_OUT"

test_summary
