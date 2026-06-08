#!/usr/bin/env bash
# Meta-tests for scripts/setup-worktree-debug-port.sh and the allocate_free_port()
# helper in scripts/lib_portable.sh.
#
# TDD structure:
#   step-1/step-2: allocate_free_port() unit tests
#   step-3/step-4: script-contract (existence, shebang, set -euo, error on missing .mcp.json)
#   step-5/step-6: core patch behavior (port printed, .mcp.json patched, siblings preserved)
#   step-7/step-8: REIFY_DEBUG_PORT resolution semantics
#   step-9/step-10: git skip-worktree hygiene

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LIB_PORTABLE="$REPO_ROOT/scripts/lib_portable.sh"
SETUP_SCRIPT="$REPO_ROOT/scripts/setup-worktree-debug-port.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== setup-worktree-debug-port / allocate_free_port tests ==="

# ============================================================
# Part 1: allocate_free_port() unit tests  (step-1 / step-2)
# ============================================================

echo ""
echo "--- Test 1: allocate_free_port is defined after sourcing lib_portable.sh ---"

assert "lib_portable.sh is sourceable" \
    bash -c "source '$LIB_PORTABLE'"

assert "allocate_free_port function is defined after sourcing" \
    bash -c "source '$LIB_PORTABLE' && declare -F allocate_free_port >/dev/null"

echo ""
echo "--- Test 2: allocate_free_port prints a single line matching ^[0-9]+$ ---"

assert "allocate_free_port output is exactly one line" \
    bash -c "
        source '$LIB_PORTABLE'
        out=\$(allocate_free_port)
        lines=\$(printf '%s\n' \"\$out\" | wc -l | tr -d ' ')
        [ \"\$lines\" -eq 1 ]
    "

assert "allocate_free_port output matches ^[0-9]+\$" \
    bash -c "
        source '$LIB_PORTABLE'
        out=\$(allocate_free_port)
        [[ \"\$out\" =~ ^[0-9]+$ ]]
    "

echo ""
echo "--- Test 3: allocate_free_port returns a value in 1..65535 ---"

assert "allocate_free_port returns a port in 1..65535" \
    bash -c "
        source '$LIB_PORTABLE'
        port=\$(allocate_free_port)
        [ \"\$port\" -ge 1 ] && [ \"\$port\" -le 65535 ]
    "

echo ""
echo "--- Test 4: allocate_free_port returns a bindable port ---"

assert "port returned by allocate_free_port is actually bindable" \
    bash -c "
        source '$LIB_PORTABLE'
        port=\$(allocate_free_port)
        python3 -c \"
import socket
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(('', \$port))
s.close()
\"
    "

# ============================================================
# Part 2: script-contract tests  (step-3 / step-4)
# ============================================================

echo ""
echo "--- Test 5: setup-worktree-debug-port.sh exists and is executable ---"

assert "setup-worktree-debug-port.sh file exists" \
    test -f "$SETUP_SCRIPT"

assert "setup-worktree-debug-port.sh is executable" \
    test -x "$SETUP_SCRIPT"

echo ""
echo "--- Test 6: script has correct shebang and strict mode ---"

assert "line 1 is #!/usr/bin/env bash" \
    bash -c "head -1 '$SETUP_SCRIPT' | grep -qF '#!/usr/bin/env bash'"

assert "script contains set -euo pipefail" \
    bash -c "grep -q 'set -euo pipefail' '$SETUP_SCRIPT'"

echo ""
echo "--- Test 7: script errors when .mcp.json is missing ---"

_t7_tmpdir=$(mktemp -d)
trap 'rm -rf "$_t7_tmpdir"' EXIT

# No .mcp.json in the tmpdir — script must fail with a useful message.
_t7_out=$("$SETUP_SCRIPT" "$_t7_tmpdir" 2>&1) && _t7_rc=0 || _t7_rc=$?

assert "script exits non-zero when .mcp.json is missing" \
    bash -c '[ "$1" -ne 0 ]' _ "$_t7_rc"

assert "error message mentions .mcp.json" \
    bash -c 'printf "%s\n" "$1" | grep -qF ".mcp.json"' _ "$_t7_out"

# ============================================================
# Part 3: core patch-behavior tests  (step-5 / step-6)
# ============================================================

echo ""
echo "--- Test 8: script prints a bare integer to stdout only ---"

_t8_tmpdir=$(mktemp -d)
trap 'rm -rf "$_t8_tmpdir"' EXIT

cp "$REPO_ROOT/.mcp.json" "$_t8_tmpdir/.mcp.json"

# Run WITHOUT REIFY_DEBUG_PORT so free allocation fires.
unset REIFY_DEBUG_PORT 2>/dev/null || true
_t8_stdout=$(REIFY_DEBUG_PORT= "$SETUP_SCRIPT" "$_t8_tmpdir" 2>/dev/null)
_t8_rc=$?

assert "script exits 0 when .mcp.json exists" \
    bash -c '[ "$1" -eq 0 ]' _ "$_t8_rc"

assert "stdout is exactly one line" \
    bash -c '[ "$(printf "%s\n" "$1" | wc -l | tr -d " ")" -eq 1 ]' _ "$_t8_stdout"

assert "stdout matches ^[0-9]+\$" \
    bash -c '[[ "$1" =~ ^[0-9]+$ ]]' _ "$_t8_stdout"

echo ""
echo "--- Test 9: .mcp.json reify-debug entry is patched correctly ---"

_t9_port="$_t8_stdout"

assert "patched .mcp.json has correct reify-debug url" \
    bash -c 'jq -r ".mcpServers[\"reify-debug\"].url" "$1" | grep -qF "http://127.0.0.1:$2/mcp"' \
    _ "$_t8_tmpdir/.mcp.json" "$_t9_port"

assert "patched .mcp.json has type http for reify-debug" \
    bash -c 'jq -r ".mcpServers[\"reify-debug\"].type" "$1" | grep -qF "http"' \
    _ "$_t8_tmpdir/.mcp.json"

echo ""
echo "--- Test 10: sibling entries preserved after patch ---"

assert "fused-memory entry preserved in patched .mcp.json" \
    bash -c 'jq -e ".mcpServers[\"fused-memory\"]" "$1" >/dev/null' \
    _ "$_t8_tmpdir/.mcp.json"

assert "escalation entry preserved in patched .mcp.json" \
    bash -c 'jq -e ".mcpServers.escalation" "$1" >/dev/null' \
    _ "$_t8_tmpdir/.mcp.json"

assert "patched .mcp.json is valid JSON" \
    bash -c 'jq . "$1" >/dev/null' _ "$_t8_tmpdir/.mcp.json"

# ============================================================
# Part 4: REIFY_DEBUG_PORT resolution semantics (step-7 / step-8)
# ============================================================

echo ""
echo "--- Test 11: valid REIFY_DEBUG_PORT is honored ---"

_t11_tmpdir=$(mktemp -d)
trap 'rm -rf "$_t11_tmpdir"' EXIT
cp "$REPO_ROOT/.mcp.json" "$_t11_tmpdir/.mcp.json"

_t11_stdout=$(REIFY_DEBUG_PORT=4500 "$SETUP_SCRIPT" "$_t11_tmpdir" 2>/dev/null)

assert "valid REIFY_DEBUG_PORT=4500 echoed on stdout" \
    bash -c '[ "$1" = "4500" ]' _ "$_t11_stdout"

assert "patched url contains :4500 when REIFY_DEBUG_PORT=4500" \
    bash -c 'jq -r ".mcpServers[\"reify-debug\"].url" "$1" | grep -qF ":4500/"' \
    _ "$_t11_tmpdir/.mcp.json"

echo ""
echo "--- Test 12: empty REIFY_DEBUG_PORT falls back to allocation ---"

_t12_tmpdir=$(mktemp -d)
trap 'rm -rf "$_t12_tmpdir"' EXIT
cp "$REPO_ROOT/.mcp.json" "$_t12_tmpdir/.mcp.json"

_t12_stdout=$(REIFY_DEBUG_PORT="" "$SETUP_SCRIPT" "$_t12_tmpdir" 2>/dev/null)

assert "empty REIFY_DEBUG_PORT falls back: stdout is digits" \
    bash -c '[[ "$1" =~ ^[0-9]+$ ]]' _ "$_t12_stdout"

assert "empty REIFY_DEBUG_PORT falls back: result != empty string" \
    bash -c '[ -n "$1" ]' _ "$_t12_stdout"

echo ""
echo "--- Test 13: non-digit REIFY_DEBUG_PORT falls back to allocation ---"

_t13_tmpdir=$(mktemp -d)
trap 'rm -rf "$_t13_tmpdir"' EXIT
cp "$REPO_ROOT/.mcp.json" "$_t13_tmpdir/.mcp.json"

_t13_stdout=$(REIFY_DEBUG_PORT="abc" "$SETUP_SCRIPT" "$_t13_tmpdir" 2>/dev/null)

assert "non-digit REIFY_DEBUG_PORT falls back: stdout is digits" \
    bash -c '[[ "$1" =~ ^[0-9]+$ ]]' _ "$_t13_stdout"

assert "non-digit REIFY_DEBUG_PORT falls back: not abc" \
    bash -c '[ "$1" != "abc" ]' _ "$_t13_stdout"

echo ""
echo "--- Test 14: whitespace-padded REIFY_DEBUG_PORT falls back (strict, no strip) ---"

_t14_tmpdir=$(mktemp -d)
trap 'rm -rf "$_t14_tmpdir"' EXIT
cp "$REPO_ROOT/.mcp.json" "$_t14_tmpdir/.mcp.json"

_t14_stdout=$(REIFY_DEBUG_PORT=" 4500 " "$SETUP_SCRIPT" "$_t14_tmpdir" 2>/dev/null)

assert "whitespace-padded REIFY_DEBUG_PORT falls back: stdout is digits" \
    bash -c '[[ "$1" =~ ^[0-9]+$ ]]' _ "$_t14_stdout"

assert "whitespace-padded REIFY_DEBUG_PORT falls back: not ' 4500 '" \
    bash -c '[ "$1" != " 4500 " ]' _ "$_t14_stdout"

echo ""
echo "--- Test 15: REIFY_DEBUG_PORT=0 (out-of-range) falls back ---"

_t15_tmpdir=$(mktemp -d)
trap 'rm -rf "$_t15_tmpdir"' EXIT
cp "$REPO_ROOT/.mcp.json" "$_t15_tmpdir/.mcp.json"

_t15_stdout=$(REIFY_DEBUG_PORT="0" "$SETUP_SCRIPT" "$_t15_tmpdir" 2>/dev/null)

assert "REIFY_DEBUG_PORT=0 falls back: stdout is digits" \
    bash -c '[[ "$1" =~ ^[0-9]+$ ]]' _ "$_t15_stdout"

assert "REIFY_DEBUG_PORT=0 falls back: not 0" \
    bash -c '[ "$1" != "0" ]' _ "$_t15_stdout"

echo ""
echo "--- Test 16: REIFY_DEBUG_PORT=70000 (out-of-range) falls back ---"

_t16_tmpdir=$(mktemp -d)
trap 'rm -rf "$_t16_tmpdir"' EXIT
cp "$REPO_ROOT/.mcp.json" "$_t16_tmpdir/.mcp.json"

_t16_stdout=$(REIFY_DEBUG_PORT="70000" "$SETUP_SCRIPT" "$_t16_tmpdir" 2>/dev/null)

assert "REIFY_DEBUG_PORT=70000 falls back: stdout is in 1..65535" \
    bash -c '[ "$1" -ge 1 ] && [ "$1" -le 65535 ]' _ "$_t16_stdout"

# ============================================================
# Part 5: git skip-worktree hygiene  (step-9 / step-10)
# ============================================================

echo ""
echo "--- Test 17: git skip-worktree is set after patching ---"

_t17_tmpdir=$(mktemp -d)
trap 'rm -rf "$_t17_tmpdir"' EXIT

# Set up a throwaway git repo.
git -C "$_t17_tmpdir" init -q
git -C "$_t17_tmpdir" commit --allow-empty -q -m "init"
cp "$REPO_ROOT/.mcp.json" "$_t17_tmpdir/.mcp.json"
git -C "$_t17_tmpdir" add .mcp.json
git -C "$_t17_tmpdir" commit -q -m "add .mcp.json"

# Run setup — patches .mcp.json and sets skip-worktree.
"$SETUP_SCRIPT" "$_t17_tmpdir" >/dev/null 2>&1

# git status --porcelain should show no modification (skip-worktree hides the change).
assert "git status shows no modification after skip-worktree" \
    bash -c 'status=$(git -C "$1" status --porcelain .mcp.json); [ -z "$status" ]' \
    _ "$_t17_tmpdir"

assert "skip-worktree bit is set on .mcp.json" \
    bash -c 'git -C "$1" ls-files -v .mcp.json | grep -q "^S"' \
    _ "$_t17_tmpdir"

echo ""
echo "--- Test 18: script succeeds outside a git work tree (no-op) ---"

_t18_tmpdir=$(mktemp -d)
trap 'rm -rf "$_t18_tmpdir"' EXIT
cp "$REPO_ROOT/.mcp.json" "$_t18_tmpdir/.mcp.json"

# _t18_tmpdir is not a git repo — skip-worktree step should be a guarded no-op.
_t18_rc=0
"$SETUP_SCRIPT" "$_t18_tmpdir" >/dev/null 2>&1 || _t18_rc=$?

assert "script succeeds (rc=0) outside a git work tree" \
    bash -c '[ "$1" -eq 0 ]' _ "$_t18_rc"

# ============================================================
# Summary
# ============================================================

test_summary
