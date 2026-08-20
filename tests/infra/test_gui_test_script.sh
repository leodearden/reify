#!/usr/bin/env bash
# Infrastructure tests for the canonical frontend-checkpoint runner
# scripts/gui-test.sh (esc-5232-3).
#
# Validates the script's CONTRACT statically (contents + arg-parsing smoke) so
# the guarantee "an agent can reliably run the gui vitest suites in any lane"
# can't silently regress. Deliberately does NOT execute the full
# `npm ci`/typecheck/vitest path (slow, network-adjacent) — mirroring
# test_run_gui_scripts.sh, which checks the launcher contract without launching
# the GUI.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

GUI_TEST="$REPO_ROOT/scripts/gui-test.sh"

echo "=== gui-test.sh checkpoint-runner tests ==="

# -- Test 1: file exists + is executable -------------------------------------
echo ""
echo "--- Test 1: scripts/gui-test.sh exists and is executable ---"

assert "scripts/gui-test.sh exists" \
    test -f "$GUI_TEST"

assert "scripts/gui-test.sh is executable" \
    test -x "$GUI_TEST"

# -- Test 2: shebang and strict-mode flags -----------------------------------
echo ""
echo "--- Test 2: shebang and 'set -euo pipefail' ---"

assert "scripts/gui-test.sh has '#!/usr/bin/env bash' shebang on line 1" \
    bash -c "head -n1 '$GUI_TEST' | grep -qE '^#!/usr/bin/env bash$'"

assert "scripts/gui-test.sh contains 'set -euo pipefail'" \
    grep -q 'set -euo pipefail' "$GUI_TEST"

assert "scripts/gui-test.sh passes 'bash -n' syntax check" \
    bash -n "$GUI_TEST"

# -- Test 3: self-provisions node_modules BEFORE running vitest --------------
# The whole point of the script: `npm ci` must run first so a lane without
# gui/node_modules can still run vitest (whose pretest->build:grammar needs
# lezer-generator from node_modules/.bin).
echo ""
echo "--- Test 3: 'npm ci' precedes 'npm test' (self-provisioning) ---"

assert "scripts/gui-test.sh runs 'npm ci'" \
    grep -q 'npm ci' "$GUI_TEST"

assert "scripts/gui-test.sh uses '--prefer-offline' (warm ~/.npm cache, offline-fast)" \
    bash -c "grep 'npm ci' '$GUI_TEST' | grep -q -- '--prefer-offline'"

assert "scripts/gui-test.sh runs 'npm test' (vitest run)" \
    grep -q 'npm test' "$GUI_TEST"

assert "scripts/gui-test.sh: 'npm ci' invocation precedes 'npm test' invocation" \
    bash -c "
        # Skip comment lines so the header prose (which mentions 'npm test'
        # before 'npm ci') can't spoof the ordering — match the real commands.
        ci_line=\$(awk '/^[[:space:]]*#/{next} /npm ci/{print NR; exit}' '$GUI_TEST')
        test_line=\$(awk '/^[[:space:]]*#/{next} /npm test/{print NR; exit}' '$GUI_TEST')
        [ -n \"\$ci_line\" ] && [ -n \"\$test_line\" ] && [ \"\$ci_line\" -lt \"\$test_line\" ]
    "

# -- Test 4: resolves gui/ from the script path (works from any cwd) ---------
echo ""
echo "--- Test 4: repo-root resolution via BASH_SOURCE ---"

assert "scripts/gui-test.sh resolves SCRIPT_DIR from \${BASH_SOURCE[0]}" \
    grep -q 'BASH_SOURCE\[0\]' "$GUI_TEST"

assert "scripts/gui-test.sh cd's into the resolved gui/ directory" \
    grep -q 'cd "\$GUI_DIR"' "$GUI_TEST"

# -- Test 5: arg-parsing smoke (no npm / no gui dir needed) -------------------
echo ""
echo "--- Test 5: --help and unknown-arg handling ---"

assert "scripts/gui-test.sh --help exits 0" \
    "$GUI_TEST" --help

assert "scripts/gui-test.sh --help mentions vitest" \
    bash -c "'$GUI_TEST' --help 2>&1 | grep -qi 'vitest'"

assert "scripts/gui-test.sh rejects an unknown flag with a usage error (exit 64)" \
    bash -c "'$GUI_TEST' --definitely-not-a-flag; rc=\$?; [ \"\$rc\" -eq 64 ]"

assert "scripts/gui-test.sh advertises '--no-typecheck' and '--' vitest forwarding" \
    bash -c "grep -q -- '--no-typecheck' '$GUI_TEST' && grep -q 'VITEST_ARGS' '$GUI_TEST'"

test_summary
