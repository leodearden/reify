#!/usr/bin/env bash
# Infrastructure tests for the single-command GUI launcher scripts (task 2228).
#
# Validates the contents and behavior of:
#   - scripts/run-gui.sh       (release-mode wrapper, no debug)
#   - scripts/run-gui-dev.sh   (debug-mode wrapper, REIFY_DEBUG=1 + vite)

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

# -- Test 23: behavioral — non-existent .ri path is rejected ------------------
echo ""
echo "--- Test 23: run-gui.sh rejects a non-existent .ri path ---"

miss_path="/tmp/reify_nonexistent_$$.ri"
assert "test path for Test 23 does not exist" \
    bash -c '! [ -e "$1" ]' _ "$miss_path"

miss_rc=0
miss_out=$(bash "$RUN_GUI" "$miss_path" 2>&1) || miss_rc=$?

assert "run-gui.sh with non-existent .ri exits non-zero" \
    bash -c '[ "$1" -ne 0 ]' _ "$miss_rc"

assert "run-gui.sh non-existent .ri error message mentions 'not found'" \
    bash -c 'printf "%s\n" "$1" | grep -qF "not found"' _ "$miss_out"

RUN_GUI_DEV="$REPO_ROOT/scripts/run-gui-dev.sh"

echo ""
echo "=== run-gui-dev.sh launcher tests ==="

# -- Test 11: file exists + is executable -----------------------------------
echo ""
echo "--- Test 11: scripts/run-gui-dev.sh exists and is executable ---"

assert "scripts/run-gui-dev.sh exists" \
    test -f "$RUN_GUI_DEV"

assert "scripts/run-gui-dev.sh is executable" \
    test -x "$RUN_GUI_DEV"

# -- Test 12: shebang and strict-mode flags ---------------------------------
echo ""
echo "--- Test 12: dev script shebang and 'set -euo pipefail' ---"

assert "scripts/run-gui-dev.sh has '#!/usr/bin/env bash' shebang on line 1" \
    bash -c "head -n1 '$RUN_GUI_DEV' | grep -qE '^#!/usr/bin/env bash$'"

assert "scripts/run-gui-dev.sh contains 'set -euo pipefail'" \
    grep -q 'set -euo pipefail' "$RUN_GUI_DEV"

# -- Test 13: invokes build-sidecar.sh --------------------------------------
echo ""
echo "--- Test 13: dev script invokes gui/sidecar/build-sidecar.sh ---"

assert "scripts/run-gui-dev.sh invokes 'gui/sidecar/build-sidecar.sh'" \
    grep -q 'gui/sidecar/build-sidecar.sh' "$RUN_GUI_DEV"

# -- Test 14: starts vite as a background process ----------------------------
echo ""
echo "--- Test 14: vite dev server is started in background ---"

assert "scripts/run-gui-dev.sh runs 'npm run dev -- --port \$REIFY_VITE_PORT'" \
    bash -c "grep -qE 'npm run dev -- --port.*REIFY_VITE_PORT' '$RUN_GUI_DEV'"

assert "scripts/run-gui-dev.sh defaults REIFY_VITE_PORT to 1420 when unset" \
    bash -c "grep -qE 'REIFY_VITE_PORT=.*:-1420' '$RUN_GUI_DEV'"

# Look for a line that runs `npm run dev -- --port $REIFY_VITE_PORT` and ends with `&`
# (or `&` followed by whitespace/comment) — i.e. the npm-run-dev invocation
# is backgrounded so the script can continue and poll for readiness.
assert "scripts/run-gui-dev.sh backgrounds the 'npm run dev' invocation (line ends with '&')" \
    bash -c "grep -E 'npm run dev -- --port' '$RUN_GUI_DEV' | grep -qE '\\) *& *(\$|#)|& *(\$|#)'"

# -- Test 15: polling loop for vite readiness on 127.0.0.1:1420 -------------
echo ""
echo "--- Test 15: dev script polls 127.0.0.1:\$REIFY_VITE_PORT for vite readiness ---"

assert "scripts/run-gui-dev.sh references '127.0.0.1:\$REIFY_VITE_PORT' (parameterized port)" \
    bash -c "grep -qE '127[.]0[.]0[.]1:.*REIFY_VITE_PORT' '$RUN_GUI_DEV'"

assert "scripts/run-gui-dev.sh contains a polling loop (curl or nc)" \
    bash -c "grep -qE 'curl|nc -z' '$RUN_GUI_DEV'"

# -- Test 16: trap kills vite background PID on EXIT ------------------------
echo ""
echo "--- Test 16: trap kills vite PID on EXIT ---"

assert "scripts/run-gui-dev.sh installs a trap on EXIT" \
    bash -c "grep -qE '^[[:space:]]*trap .* EXIT' '$RUN_GUI_DEV'"

assert "scripts/run-gui-dev.sh trap references the vite PID variable" \
    bash -c "grep -E '^[[:space:]]*trap ' '$RUN_GUI_DEV' | grep -qE 'VITE_PID|kill|cleanup'"

# -- Test 17: cargo build is DEBUG profile (no --release) ------------------
echo ""
echo "--- Test 17: cargo build line uses DEBUG profile (no --release) ---"

assert "scripts/run-gui-dev.sh invokes 'cargo build -p reify-gui'" \
    grep -q 'cargo build -p reify-gui' "$RUN_GUI_DEV"

assert "scripts/run-gui-dev.sh cargo build line includes '--features gui'" \
    bash -c "grep 'cargo build -p reify-gui' '$RUN_GUI_DEV' | grep -q -- '--features gui'"

assert "scripts/run-gui-dev.sh cargo build line does NOT include '--release'" \
    bash -c "! grep 'cargo build -p reify-gui' '$RUN_GUI_DEV' | grep -q -- '--release'"

# -- Test 18: REIFY_DEBUG=1 is set ------------------------------------------
echo ""
echo "--- Test 18: dev script sets REIFY_DEBUG=1 ---"

assert "scripts/run-gui-dev.sh sets REIFY_DEBUG=1 (export or inline)" \
    bash -c "grep -qE '(export REIFY_DEBUG=1|REIFY_DEBUG=1[[:space:]]+target/)' '$RUN_GUI_DEV'"

# -- Test 19: LD_LIBRARY_PATH OCCT export -----------------------------------
echo ""
echo "--- Test 19: dev script exports OCCT LD_LIBRARY_PATH ---"

assert "scripts/run-gui-dev.sh exports LD_LIBRARY_PATH" \
    bash -c "grep -qE '^[[:space:]]*export LD_LIBRARY_PATH=' '$RUN_GUI_DEV'"

assert "scripts/run-gui-dev.sh LD_LIBRARY_PATH includes '/snap/freecad/current/usr/lib'" \
    grep -qF '/snap/freecad/current/usr/lib' "$RUN_GUI_DEV"

# -- Test 20: target/debug/reify-gui invocation, NOT exec ------------------
echo ""
echo "--- Test 20: dev script runs target/debug/reify-gui WITHOUT 'exec' ---"

assert "scripts/run-gui-dev.sh invokes 'target/debug/reify-gui'" \
    grep -q 'target/debug/reify-gui' "$RUN_GUI_DEV"

# Critical: must NOT exec the binary (exec replaces the shell, killing the
# EXIT trap that reaps vite). Instead run as a child process and propagate
# the exit code.
assert "scripts/run-gui-dev.sh does NOT exec target/debug/reify-gui (trap must fire)" \
    bash -c "! grep -E '^[[:space:]]*exec target/debug/reify-gui' '$RUN_GUI_DEV'"

# -- Test 20b: REIFY_DEBUG_PORT — configured, default 3939, exported ----------
echo ""
echo "--- Test 20b: dev script configures REIFY_DEBUG_PORT (task 4340) ---"

assert "scripts/run-gui-dev.sh references REIFY_DEBUG_PORT" \
    bash -c "grep -q 'REIFY_DEBUG_PORT' '$RUN_GUI_DEV'"

assert "scripts/run-gui-dev.sh defaults REIFY_DEBUG_PORT to 3939 when unset" \
    bash -c "grep -qE 'REIFY_DEBUG_PORT=.*:-3939' '$RUN_GUI_DEV'"

assert "scripts/run-gui-dev.sh exports REIFY_DEBUG_PORT" \
    bash -c "grep -qE 'export REIFY_DEBUG_PORT' '$RUN_GUI_DEV'"

# -- Test 21: behavioral — no args -> usage + non-zero ---------------------
echo ""
echo "--- Test 21: dev script no-args invocation prints usage + exits non-zero ---"

dev_no_args_out=$(bash "$RUN_GUI_DEV" 2>&1 || true)
dev_no_args_rc=0
bash "$RUN_GUI_DEV" >/dev/null 2>&1 || dev_no_args_rc=$?

assert "run-gui-dev.sh with no args exits non-zero" \
    bash -c '[ "$1" -ne 0 ]' _ "$dev_no_args_rc"

assert "run-gui-dev.sh with no args prints usage mentioning '<file>'" \
    bash -c 'printf "%s\n" "$1" | grep -qE "[Uu]sage.*<file>|<file>"' _ "$dev_no_args_out"

# -- Test 22: behavioral — non-.ri rejected --------------------------------
echo ""
echo "--- Test 22: dev script rejects non-.ri file argument ---"

dev_non_ri_out=$(bash "$RUN_GUI_DEV" /tmp/some_random.txt 2>&1 || true)
dev_non_ri_rc=0
bash "$RUN_GUI_DEV" /tmp/some_random.txt >/dev/null 2>&1 || dev_non_ri_rc=$?

assert "run-gui-dev.sh with non-.ri file exits non-zero" \
    bash -c '[ "$1" -ne 0 ]' _ "$dev_non_ri_rc"

assert "run-gui-dev.sh non-.ri error message mentions '.ri'" \
    bash -c 'printf "%s\n" "$1" | grep -qF .ri' _ "$dev_non_ri_out"

# -- Test 24: behavioral — non-existent .ri path is rejected ------------------
echo ""
echo "--- Test 24: run-gui-dev.sh rejects a non-existent .ri path ---"

dev_miss_path="/tmp/reify_nonexistent_$$.ri"
assert "test path for Test 24 does not exist" \
    bash -c '! [ -e "$1" ]' _ "$dev_miss_path"

dev_miss_rc=0
dev_miss_out=$(bash "$RUN_GUI_DEV" "$dev_miss_path" 2>&1) || dev_miss_rc=$?

assert "run-gui-dev.sh with non-existent .ri exits non-zero" \
    bash -c '[ "$1" -ne 0 ]' _ "$dev_miss_rc"

assert "run-gui-dev.sh non-existent .ri error message mentions 'not found'" \
    bash -c 'printf "%s\n" "$1" | grep -qF "not found"' _ "$dev_miss_out"

# -- Test 25: behavioral — vite-process-death early-exit branch ---------------
echo ""
echo "--- Test 25: run-gui-dev.sh vite-process-death early-exit branch ---"

_t25_tmpdir=$(mktemp -d)
_t25_port=$(python3 -c 'import socket;s=socket.socket();s.bind(("",0));print(s.getsockname()[1])')
trap 'rm -rf "$_t25_tmpdir"' EXIT

# Build temp fixture: the script resolves REPO_ROOT from ${BASH_SOURCE[0]}/..
# so we copy run-gui-dev.sh into $tmpdir/scripts/ to make REPO_ROOT=$tmpdir.
mkdir -p "$_t25_tmpdir/scripts" "$_t25_tmpdir/gui/sidecar" "$_t25_tmpdir/bin"
cp "$RUN_GUI_DEV" "$_t25_tmpdir/scripts/run-gui-dev.sh"
chmod +x "$_t25_tmpdir/scripts/run-gui-dev.sh"

# Stub: build-sidecar.sh — no-op so the script reaches the vite spawn.
cat > "$_t25_tmpdir/gui/sidecar/build-sidecar.sh" <<'SIDECAR_STUB'
#!/usr/bin/env bash
exit 0
SIDECAR_STUB
chmod +x "$_t25_tmpdir/gui/sidecar/build-sidecar.sh"

# Minimal package.json so (cd gui && npm install ...) does not crash on missing dir.
printf '{}' > "$_t25_tmpdir/gui/package.json"

# Empty fixture file (the script requires a .ri file that exists).
touch "$_t25_tmpdir/test.ri"

# Stub npm: install → exit 0 (no-op); run dev → exit 1 immediately so the
# polling loop's `kill -0 "$VITE_PID"` branch fires and the early-exit path runs.
cat > "$_t25_tmpdir/bin/npm" <<'NPM_STUB'
#!/usr/bin/env bash
# Stub for run-gui-dev.sh behavioral test (task 2243):
#   - `npm run dev ...` exits 1 immediately so the polling loop's
#     `kill -0 "$VITE_PID"` branch fires within ~0.5s.
#   - `npm install ...` is a no-op so the script reaches the vite spawn.
case "${1:-}" in
    install) exit 0 ;;
    run)
        shift
        case "${1:-}" in
            dev) exit 1 ;;
            *)   exit 0 ;;
        esac
        ;;
    *) exit 0 ;;
esac
NPM_STUB
chmod +x "$_t25_tmpdir/bin/npm"

# Stub curl: always fail so the readiness check never succeeds regardless of
# what happens to be listening on the test's vite port in the environment
# (e.g. an unrelated vite dev server from a concurrent task).  The polling
# loop must reach the `kill -0 "$VITE_PID"` death-detection branch, not the
# curl-success branch.
# Exit 7 = CURLE_COULDNT_CONNECT, mimicking the real "no listener" behaviour.
# The script only checks curl's success/failure (`if curl ...; then`), so any
# non-zero exit works; 7 is chosen for semantic accuracy.
cat > "$_t25_tmpdir/bin/curl" <<'CURL_STUB'
#!/usr/bin/env bash
exit 7
CURL_STUB
chmod +x "$_t25_tmpdir/bin/curl"

# Run the script with the stubbed PATH and an ephemeral port; capture combined
# output + rc in one shot. REIFY_VITE_PORT is set to an ephemeral free port so
# the script's polling loop targets a port unlikely to collide with another
# worktree's vite on :1420 (task 2308). The curl stub above is a redundant
# secondary guard for the same class of failure.
_t25_out=$(REIFY_VITE_PORT="$_t25_port" PATH="$_t25_tmpdir/bin:$PATH" \
    bash "$_t25_tmpdir/scripts/run-gui-dev.sh" "$_t25_tmpdir/test.ri" 2>&1) \
    && _t25_rc=0 || _t25_rc=$?

assert "run-gui-dev.sh: vite-death branch exits non-zero" \
    bash -c '[ "$1" -ne 0 ]' _ "$_t25_rc"

assert "run-gui-dev.sh: vite-death branch emits 'vite process exited'" \
    bash -c 'printf "%s\n" "$1" | grep -qF "vite process exited"' _ "$_t25_out"

assert "run-gui-dev.sh: vite-death branch does NOT hit the 30s timeout message" \
    bash -c '! printf "%s\n" "$1" | grep -qF "did not become ready"' _ "$_t25_out"

test_summary
