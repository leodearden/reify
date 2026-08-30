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
#
# Each stub records "<own basename> <argv...>" to $REIFY_TEST_CALLS_FILE, so
# check 3 below can attribute the assertion-4 fixture round-trips to the
# specific stub that received them.
make_hook_target_stub() {
    local _name="$1"
    cat > "$STUB_DIR/$_name" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$(basename "$0") $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
    chmod +x "$STUB_DIR/$_name"
}
make_hook_target_stub "reify-audit"                      # historical raw-binary wiring
make_hook_target_stub "reify-audit-predone-wrapper.sh"   # correct wrapper wiring

# ── raw reify-audit stub (assertion 4's round-trip target) ───────────────────
# Deliberately named differently from BOTH hook-target stubs so check 3 can tell
# "the raw binary ran the fixtures" from "the wrapper ran the fixtures" by name
# alone. Reproduces just enough of reify-audit's contract for assertions 4a/4b:
#   4a (known-pass fixture) → exit 0 with a JSON array on stderr
#   4b (known-fail fixture) → exit 1 (non-zero, and specifically NOT 125, since
#                              125 is the smoke script's infrastructure-error
#                              sentinel)
make_raw_audit_stub() {
    cat > "$STUB_DIR/reify-audit-raw" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$(basename "$0") $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
for _arg in "$@"; do
    if [ "$_arg" = "smoke-fail-99992" ]; then
        echo '[{"severity":"High","pattern":"P1"}]' >&2
        exit 1
    fi
done
echo '[]' >&2
exit 0
STUB_EOF
    chmod +x "$STUB_DIR/reify-audit-raw"
}
make_raw_audit_stub

# ── curl stub ─────────────────────────────────────────────────────────────────
# Assertion 3 POSTs a JSON-RPC `initialize` to the fused-memory MCP on :8002.
# Stubbed so this suite never depends on (nor pokes) the live fleet-wide MCP
# server. The smoke script captures curl's STDOUT as the HTTP status code
# (-w "%{http_code}") and separately greps the -o file for a JSON-RPC body, so
# the stub must honour BOTH channels.
make_curl_stub() {
    cat > "$STUB_DIR/curl" << 'STUB_EOF'
#!/usr/bin/env bash
echo "curl $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
_out=""
_prev=""
for _arg in "$@"; do
    [ "$_prev" = "-o" ] && _out="$_arg"
    _prev="$_arg"
done
[ -n "$_out" ] && printf '{"jsonrpc":"2.0","id":1,"result":{}}\n' > "$_out"
printf '%s' "${REIFY_TEST_HTTP_CODE:-200}"
exit 0
STUB_EOF
    chmod +x "$STUB_DIR/curl"
}
make_curl_stub

# ── runner ────────────────────────────────────────────────────────────────────
# Sets the globals SMOKE_OUT / SMOKE_RC / SMOKE_CALLS. Called as a bare command
# (never inside a command substitution) so the assignments survive into the
# parent shell, matching test_helpers.sh's no-subshell assert contract.
#
# The calls log is truncated per run so each check reads only its own run's
# stub invocations.
CALLS_FILE="$TMPROOT/calls.log"

run_smoke() {
    local _env_value="$1"
    local _out_file="$TMPROOT/smoke.out"
    local rc=0
    : > "$CALLS_FILE"
    env \
        REIFY_TEST_HOOK_ENV_VALUE="$_env_value" \
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        REIFY_AUDIT_BIN="$STUB_DIR/reify-audit-raw" \
        PATH="$STUB_DIR:$PATH" \
        bash "$SMOKE" >"$_out_file" 2>&1 || rc=$?
    SMOKE_OUT="$(cat "$_out_file")"
    SMOKE_CALLS="$(cat "$CALLS_FILE")"
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

# ==============================================================================
# Check 3: assertion 4's fixture round-trip targets the RAW BINARY
#
# Assertion 4's stated purpose -- in its own comment and in design §11 -- is a
# DIRECT reify-audit round-trip catching re-introduction of the dead
# .taskmaster/tasks/tasks.json default (task 3731). Deriving its target from the
# hook env var was only equivalent to that while the env var named the binary.
# Post-rewire it silently became a WRAPPER round-trip: the wrapper injects its
# own --tasks-file/--runs-db/--project-root before forwarding "$@", so the
# fixture wins only by clap's last-wins precedence, and a fixture round-trip
# now needs a live fused-memory MCP.
#
# So: with the hook correctly pointed at the wrapper, the fixtures must still
# run against $REIFY_AUDIT_BIN -- never against the hook's first token.
# ==============================================================================
echo ""
echo "--- Check 3: assertion 4 round-trips the raw binary, not the hook target ---"

run_smoke "$STUB_DIR/reify-audit-predone-wrapper.sh --task {id} --pre-done"

assert "raw-binary stub ran the 4a known-pass fixture" \
    bash -c 'printf %s "$1" | grep -q "^reify-audit-raw .*smoke-pass-99991"' -- "$SMOKE_CALLS"

assert "raw-binary stub ran the 4b known-fail fixture" \
    bash -c 'printf %s "$1" | grep -q "^reify-audit-raw .*smoke-fail-99992"' -- "$SMOKE_CALLS"

assert "raw-binary stub received the fixture --tasks-file" \
    bash -c 'printf %s "$1" | grep -q "^reify-audit-raw .*--tasks-file"' -- "$SMOKE_CALLS"

# The load-bearing negative: the wrapper is the HOOK target (assertions 2/2.6
# legitimately invoke it with --help), but it must never receive a fixture
# round-trip.
assert "wrapper stub received NO --tasks-file invocation" \
    bash -c '! printf %s "$1" | grep -q "^reify-audit-predone-wrapper.sh .*--tasks-file"' -- "$SMOKE_CALLS"

# End-to-end: with every external dependency stubbed and the hook correctly
# wired, the smoke script must run clean to completion.
assert "fully-stubbed correct wiring exits 0" \
    bash -c '[ "$1" -eq 0 ]' -- "$SMOKE_RC"

test_summary
