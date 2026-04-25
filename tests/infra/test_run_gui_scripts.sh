#!/usr/bin/env bash
# Infrastructure tests for the single-command GUI launcher scripts (task 2228).
#
# Validates the contents and behavior of:
#   - scripts/run-gui.sh       (release-mode wrapper, no debug)
#   - scripts/run-gui-dev.sh   (debug-mode wrapper, REIFY_DEBUG=1 + vite)
#
# Plus a minimal CLAUDE.md documentation grep check.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

RUN_GUI="$REPO_ROOT/scripts/run-gui.sh"

echo "=== run-gui.sh launcher tests ==="

# -- Test 1: file exists + is executable -------------------------------------
echo ""
echo "--- Test 1: scripts/run-gui.sh exists and is executable ---"

assert "scripts/run-gui.sh exists" \
    test -f "$RUN_GUI"

assert "scripts/run-gui.sh is executable" \
    test -x "$RUN_GUI"

# -- Test 2: shebang and strict-mode flags ------------------------------------
echo ""
echo "--- Test 2: shebang and 'set -euo pipefail' ---"

assert "scripts/run-gui.sh has '#!/usr/bin/env bash' shebang on line 1" \
    bash -c "head -n1 '$RUN_GUI' | grep -qE '^#!/usr/bin/env bash$'"

assert "scripts/run-gui.sh contains 'set -euo pipefail'" \
    grep -q 'set -euo pipefail' "$RUN_GUI"

# -- Test 3: ordered build-sidecar.sh -> cargo build --------------------------
echo ""
echo "--- Test 3: build-sidecar.sh runs BEFORE cargo build ---"

assert "scripts/run-gui.sh invokes 'gui/sidecar/build-sidecar.sh'" \
    grep -q 'gui/sidecar/build-sidecar.sh' "$RUN_GUI"

assert "scripts/run-gui.sh invokes 'cargo build -p reify-gui'" \
    grep -q 'cargo build -p reify-gui' "$RUN_GUI"

assert "scripts/run-gui.sh: build-sidecar line precedes cargo build line" \
    bash -c "
        sidecar_line=\$(grep -n 'gui/sidecar/build-sidecar.sh' '$RUN_GUI' | head -1 | cut -d: -f1)
        cargo_line=\$(grep -n 'cargo build -p reify-gui' '$RUN_GUI' | head -1 | cut -d: -f1)
        [ -n \"\$sidecar_line\" ] && [ -n \"\$cargo_line\" ] && [ \"\$sidecar_line\" -lt \"\$cargo_line\" ]
    "

# -- Test 4: npm install + npm run build for the gui frontend ----------------
echo ""
echo "--- Test 4: gui frontend dependency install + build ---"

assert "scripts/run-gui.sh runs 'npm install' (or 'npm ci')" \
    bash -c "grep -qE 'npm (install|ci)' '$RUN_GUI'"

assert "scripts/run-gui.sh runs 'npm run build' to produce gui/dist" \
    grep -q 'npm run build' "$RUN_GUI"

# -- Test 5: cargo build uses --release + --features gui ----------------------
echo ""
echo "--- Test 5: cargo build flags ---"

assert "scripts/run-gui.sh cargo build line includes '--features gui'" \
    bash -c "grep 'cargo build -p reify-gui' '$RUN_GUI' | grep -q -- '--features gui'"

assert "scripts/run-gui.sh cargo build line includes '--release'" \
    bash -c "grep 'cargo build -p reify-gui' '$RUN_GUI' | grep -q -- '--release'"

assert "scripts/run-gui.sh cargo build line includes '--bin reify-gui'" \
    bash -c "grep 'cargo build -p reify-gui' '$RUN_GUI' | grep -q -- '--bin reify-gui'"

# -- Test 6: LD_LIBRARY_PATH export for OCCT ----------------------------------
echo ""
echo "--- Test 6: LD_LIBRARY_PATH export for OCCT shared libraries ---"

assert "scripts/run-gui.sh exports LD_LIBRARY_PATH" \
    bash -c "grep -qE '^[[:space:]]*export LD_LIBRARY_PATH=' '$RUN_GUI'"

assert "scripts/run-gui.sh LD_LIBRARY_PATH includes '/snap/freecad/current/usr/lib'" \
    grep -qF '/snap/freecad/current/usr/lib' "$RUN_GUI"

# -- Test 7: launches target/release/reify-gui --------------------------------
echo ""
echo "--- Test 7: launches target/release/reify-gui ---"

assert "scripts/run-gui.sh invokes 'target/release/reify-gui'" \
    grep -q 'target/release/reify-gui' "$RUN_GUI"

# -- Test 8: NO debug-mode contamination -------------------------------------
echo ""
echo "--- Test 8: run-gui.sh does NOT mention REIFY_DEBUG or 'npm run dev' ---"

assert "scripts/run-gui.sh does NOT contain 'REIFY_DEBUG'" \
    bash -c "! grep -q 'REIFY_DEBUG' '$RUN_GUI'"

assert "scripts/run-gui.sh does NOT contain 'npm run dev'" \
    bash -c "! grep -q 'npm run dev' '$RUN_GUI'"

# -- Test 9: behavioral — no args -> usage + non-zero exit -------------------
echo ""
echo "--- Test 9: no-args invocation prints usage and exits non-zero ---"

# Capture stderr+stdout combined; the usage message may go to either stream.
no_args_out=$(bash "$RUN_GUI" 2>&1 || true)
no_args_rc=0
bash "$RUN_GUI" >/dev/null 2>&1 || no_args_rc=$?

assert "run-gui.sh with no args exits non-zero" \
    bash -c '[ "$1" -ne 0 ]' _ "$no_args_rc"

assert "run-gui.sh with no args prints usage mentioning '<file>'" \
    bash -c 'printf "%s\n" "$1" | grep -qE "[Uu]sage.*<file>|<file>"' _ "$no_args_out"

# -- Test 10: behavioral — non-.ri file is rejected --------------------------
echo ""
echo "--- Test 10: non-.ri file argument is rejected ---"

# The wrapper must validate the extension before doing any expensive build
# step, otherwise users will wait minutes for a typo to be caught.
non_ri_out=$(bash "$RUN_GUI" /tmp/some_random.txt 2>&1 || true)
non_ri_rc=0
bash "$RUN_GUI" /tmp/some_random.txt >/dev/null 2>&1 || non_ri_rc=$?

assert "run-gui.sh with non-.ri file exits non-zero" \
    bash -c '[ "$1" -ne 0 ]' _ "$non_ri_rc"

assert "run-gui.sh non-.ri error message mentions '.ri'" \
    bash -c 'printf "%s\n" "$1" | grep -qF .ri' _ "$non_ri_out"

test_summary
