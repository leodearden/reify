#!/usr/bin/env bash
# Infrastructure test for task 4449.
# Validates the linker-config contract (outcome-independent):
#   (a) TARGET-SCOPING — any -fuse-ld rustflag lives ONLY under
#       [target.x86_64-unknown-linux-gnu], never in a global [build]
#       (so wasm32/emscripten keep their toolchain default).
#   (b) RESOLVABILITY — the effective x86_64 linker resolves on host:
#       mold → `command -v mold`; rust-lld/default → sysroot binary exists.
#   (c) DECISION-CONSISTENCY — docs/notes/linker-rustlld-vs-mold-bench.md
#       exists and its single machine-readable `chosen-linker:` token
#       (value exactly `mold` or `rust-lld`) equals the effective linker
#       computed from .cargo/config.toml.
#
# RED state:  bench doc absent → assert (c) file-exists fails.
# GREEN state: bench doc present with correct chosen-linker: token.
# Valid in BOTH benchmark outcome branches (mold-wins or rust-lld-tie/win).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== linker-config contract tests ==="

# Preflight: tomllib is stdlib on Python ≥3.11; if unavailable, skip cleanly
# rather than aborting mid-test with an opaque ImportError traceback.
python3 -c 'import tomllib' 2>/dev/null || {
    echo "SKIP: python3 tomllib not available (requires Python ≥3.11); skipping linker-config tests"
    exit 0
}

CONFIG="$REPO_ROOT/.cargo/config.toml"
BENCH_DOC="$REPO_ROOT/docs/notes/linker-rustlld-vs-mold-bench.md"

# Write a Python helper to a temp file so each assert() call can invoke it
# directly (assert only accepts a command + args, not heredocs).
# Uses tomllib (stdlib >=3.11) to parse .cargo/config.toml robustly:
#   - [target.x86_64-unknown-linux-gnu] is the x86_64 table
#   - [target.x86_64-unknown-linux-gnu.manifold] is a nested dict under that
#     table (key 'manifold'), so .get('rustflags', []) on the x86_64 table
#     never picks up manifold's rustc-link-lib/rustc-link-search directives
#   - [build] is a top-level sibling — completely separate
_PARSE_PY="$(mktemp /tmp/linker_config_parse_XXXXXX.py)"
trap 'rm -f "$_PARSE_PY"' EXIT

cat > "$_PARSE_PY" << 'PYEOF'
"""Validate .cargo/config.toml linker-config contract.
Usage:
  python3 <script> <config_path> check_build_scope   # exit 1 if -fuse-ld in [build]
  python3 <script> <config_path> effective_linker    # print 'mold' or 'rust-lld'
"""
import sys, tomllib

config_path, action = sys.argv[1], sys.argv[2]

with open(config_path, 'rb') as f:
    cfg = tomllib.load(f)

if action == 'check_build_scope':
    # Verify no -fuse-ld appears in [build].rustflags.
    build_flags = cfg.get('build', {}).get('rustflags', [])
    if isinstance(build_flags, str):
        build_flags = [build_flags]
    for flag in build_flags:
        if '-fuse-ld' in str(flag):
            print(f'FAIL: -fuse-ld found in [build].rustflags: {flag}', file=sys.stderr)
            sys.exit(1)
    sys.exit(0)

elif action == 'check_target_fuse_ld':
    # Verify -Clink-arg=-fuse-ld=mold is explicitly in [target.x86_64-unknown-linux-gnu].rustflags.
    # This closes the positive-placement loop: it is not enough that [build] lacks the flag;
    # when mold is the chosen linker the flag MUST be present in the target table.
    target_flags = (
        cfg.get('target', {})
           .get('x86_64-unknown-linux-gnu', {})
           .get('rustflags', [])
    )
    if isinstance(target_flags, str):
        target_flags = [target_flags]
    for flag in target_flags:
        if '-Clink-arg=-fuse-ld=mold' in str(flag):
            sys.exit(0)
    print('FAIL: -Clink-arg=-fuse-ld=mold not found in [target.x86_64-unknown-linux-gnu].rustflags', file=sys.stderr)
    sys.exit(1)

elif action == 'effective_linker':
    # Read [target.x86_64-unknown-linux-gnu].rustflags (NOT the .manifold sub-table).
    target_flags = (
        cfg.get('target', {})
           .get('x86_64-unknown-linux-gnu', {})
           .get('rustflags', [])
    )
    if isinstance(target_flags, str):
        target_flags = [target_flags]
    linker = 'rust-lld'  # toolchain default when no -fuse-ld flag is present
    for flag in target_flags:
        if flag.startswith('-Clink-arg=-fuse-ld='):
            val = flag.split('=', 2)[2]
            linker = 'mold' if val == 'mold' else 'rust-lld'
            break
    print(linker)
    sys.exit(0)

else:
    print(f'Unknown action: {action}', file=sys.stderr)
    sys.exit(2)
PYEOF

# Determine the effective x86_64 linker from .cargo/config.toml before the asserts.
EFFECTIVE_LINKER="$(python3 "$_PARSE_PY" "$CONFIG" effective_linker)"

# -- Test 1 (a): TARGET-SCOPING -----------------------------------------------
echo ""
echo "--- Test 1 (a): no -fuse-ld in [build].rustflags (target-scoped only) ---"

assert "no -Clink-arg=-fuse-ld= in [build].rustflags" \
    python3 "$_PARSE_PY" "$CONFIG" check_build_scope

# Positive placement: when mold is the chosen linker the flag must be present
# in [target.x86_64-unknown-linux-gnu].rustflags — not merely absent from [build].
# This catches a misplaced flag under a wrong table (e.g. a cfg() table) that
# would satisfy the negative check above but violate the target-scoping contract.
if [ "$EFFECTIVE_LINKER" = "mold" ]; then
    assert "-Clink-arg=-fuse-ld=mold present in [target.x86_64-unknown-linux-gnu].rustflags" \
        python3 "$_PARSE_PY" "$CONFIG" check_target_fuse_ld
fi

# -- Test 2 (b): RESOLVABILITY ------------------------------------------------
echo ""
echo "--- Test 2 (b): effective x86_64 linker resolves on host ---"

if [ "$EFFECTIVE_LINKER" = "mold" ]; then
    assert "effective linker is mold: mold binary present on PATH" \
        command -v mold
else
    SYSROOT="$(rustc --print sysroot 2>/dev/null)"
    LLD_PATH="$SYSROOT/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld"
    assert "effective linker is rust-lld: sysroot rust-lld binary exists" \
        test -f "$LLD_PATH"
fi

# -- Test 3 (c): DECISION-CONSISTENCY ----------------------------------------
echo ""
echo "--- Test 3 (c): bench doc exists with correct chosen-linker token ---"

assert "docs/notes/linker-rustlld-vs-mold-bench.md exists" \
    test -f "$BENCH_DOC"

if [ -f "$BENCH_DOC" ]; then
    # Extract the single machine-readable token: "chosen-linker: <mold|rust-lld>"
    CHOSEN="$(grep -oE 'chosen-linker: [a-z-]+' "$BENCH_DOC" | head -1 | sed 's/chosen-linker: //')"
    assert "chosen-linker token is 'mold' or 'rust-lld'" \
        bash -c "[ '$CHOSEN' = 'mold' ] || [ '$CHOSEN' = 'rust-lld' ]"
    assert "chosen-linker token matches effective linker from .cargo/config.toml" \
        bash -c "[ '$CHOSEN' = '$EFFECTIVE_LINKER' ]"
fi

test_summary
