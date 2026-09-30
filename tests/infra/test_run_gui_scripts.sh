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

# allocate_free_port — the canonical free-ephemeral-port helper, used by
# _rgs_free_port below.
# shellcheck source=scripts/lib_portable.sh
source "$REPO_ROOT/scripts/lib_portable.sh"

RUN_GUI="$REPO_ROOT/scripts/run-gui.sh"

# Shared launch-environment helpers sourced by BOTH launchers (#7254).
LIB_GUI_LAUNCH="$REPO_ROOT/scripts/lib_gui_launch.sh"

# Hermetic display axis (#7290). reify-gui is a GTK/WebKit app and both
# launchers refuse to build with no display (lib_gui_launch.sh's
# gui_launch_preflight_display, #7254), so a behavioural test that does not
# establish a display passes on a developer box and fails on a headless
# verify host — exactly the split that made Test 25 red only some of the
# time (esc-7014-1). Pin one here, unconditionally, so a future test that
# forgets its own DISPLAY=:99 still behaves identically on both.
# gui_launch_preflight_display() passes if EITHER DISPLAY or WAYLAND_DISPLAY
# is non-empty, so pinning DISPLAY alone would leave this axis half
# host-dependent on a Wayland dev box (ambient WAYLAND_DISPLAY non-empty
# there, empty on a headless verify host) — scrub WAYLAND_DISPLAY too so
# both halves of the gate are hermetic.
# A test that must drive the NO-display gate scrubs both for its CHILD only,
# via `env -u DISPLAY -u WAYLAND_DISPLAY` (Test 27) — never at suite scope;
# Test 32 at the end of this file guards that.
export DISPLAY=:99
unset WAYLAND_DISPLAY

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

# =============================================================================
# Reusable fixture machinery for the behavioural launcher tests
# =============================================================================
# Every behavioural test below runs a COPY of a launcher script out of a
# throwaway tmpdir, because both launchers resolve REPO_ROOT from
# ${BASH_SOURCE[0]}/.. — copying the script into $tmpdir/scripts/ makes
# REPO_ROOT=$tmpdir, so the stubbed gui/, bin/ and target/ trees below are the
# ones the script actually sees.
#
# scripts/lib_gui_launch.sh is copied alongside it for the same reason: both
# launchers `source "$SCRIPT_DIR/lib_gui_launch.sh"`, so it must exist NEXT TO
# the copy. Copying (rather than symlinking to the real one) keeps the fixture
# self-contained and makes the source path resolve exactly as it does in a
# real checkout.
#
# WHY a cumulative array instead of a per-test `trap ... EXIT`: bash keeps ONE
# EXIT trap per shell, so a second `trap 'rm -rf "$dir"' EXIT` silently
# REPLACES the first and leaks the earlier tmpdir. Each fixture registers
# itself in _RGS_TMPDIRS instead, and the single trap installed here reaps all
# of them.

_RGS_TMPDIRS=()

_rgs_cleanup() {
    local _d
    for _d in ${_RGS_TMPDIRS[@]+"${_RGS_TMPDIRS[@]}"}; do
        [ -n "$_d" ] && rm -rf "$_d"
    done
}
trap '_rgs_cleanup' EXIT

# _rgs_mktemp <varname> — mktemp -d, assigned to <varname>, registered for
# cleanup at EXIT.
#
# WHY an out-parameter instead of printing the path: `d=$(_rgs_mktemp)` would
# run the function in a command-substitution SUBSHELL, so the
# `_RGS_TMPDIRS+=(...)` append would be discarded with that subshell and the
# tmpdir would leak past the EXIT trap (measured: one stray tmp.XXXX per run).
# `printf -v` assigns in the CALLER's shell, keeping the registration live.
_rgs_mktemp() {
    local _d
    _d=$(mktemp -d)
    _RGS_TMPDIRS+=("$_d")
    printf -v "$1" '%s' "$_d"
}

# _rgs_free_port — an ephemeral port nothing is bound to, so a test never
# collides with another worktree's vite on :1420 (task 2308).
#
# Delegates to scripts/lib_portable.sh's allocate_free_port rather than
# re-inlining the python3 one-liner: that library's own header (lib_portable.sh
# :29-31) already cites THIS file as the origin of the idiom, and it adds the
# `command -v python3` guard the inline copy lacked — so a host without python3
# gets a named error ("python3 not found on PATH; cannot allocate a free port")
# instead of bash's bare `python3: command not found`.
_rgs_free_port() {
    allocate_free_port
}

# _mk_rungui_dev_fixture <dir>
#
# Builds the parts of the fixture EVERY run-gui-dev.sh behavioural test needs:
# the script copy (so REPO_ROOT=<dir>), a no-op build-sidecar.sh, a minimal
# gui/package.json so `(cd gui && npm install ...)` does not crash on a missing
# dir, an empty test.ri (the script requires the file to exist), and an empty
# bin/ for the caller's stubs. Stub authoring (npm, curl, cargo, the reify-gui
# binary) is deliberately left to the CALLER — each test drives a different
# branch and needs different stub behaviour.
_mk_rungui_dev_fixture() {
    local dir="$1"

    mkdir -p "$dir/scripts" "$dir/gui/sidecar" "$dir/bin"
    cp "$RUN_GUI_DEV" "$dir/scripts/run-gui-dev.sh"
    chmod +x "$dir/scripts/run-gui-dev.sh"
    cp "$LIB_GUI_LAUNCH" "$dir/scripts/lib_gui_launch.sh"

    # Stub: build-sidecar.sh — no-op so the script reaches the vite spawn.
    cat > "$dir/gui/sidecar/build-sidecar.sh" <<'SIDECAR_STUB'
#!/usr/bin/env bash
exit 0
SIDECAR_STUB
    chmod +x "$dir/gui/sidecar/build-sidecar.sh"

    printf '{}' > "$dir/gui/package.json"
    touch "$dir/test.ri"
}

# _mk_rungui_fixture <dir> — the run-gui.sh (release launcher) equivalent.
# Same shape; run-gui.sh has no vite/curl involvement, so it needs no bin/curl.
_mk_rungui_fixture() {
    local dir="$1"

    mkdir -p "$dir/scripts" "$dir/gui/sidecar" "$dir/bin"
    cp "$RUN_GUI" "$dir/scripts/run-gui.sh"
    chmod +x "$dir/scripts/run-gui.sh"
    cp "$LIB_GUI_LAUNCH" "$dir/scripts/lib_gui_launch.sh"

    cat > "$dir/gui/sidecar/build-sidecar.sh" <<'SIDECAR_STUB'
#!/usr/bin/env bash
exit 0
SIDECAR_STUB
    chmod +x "$dir/gui/sidecar/build-sidecar.sh"

    printf '{}' > "$dir/gui/package.json"
    touch "$dir/test.ri"
}

# _rgs_stub_npm_marker <dir> — an npm that drops "$_RGS_NPM_MARKER" on EVERY
# invocation. The marker's ABSENCE is how the preflight tests prove a refusal
# beat the first expensive step.
_rgs_stub_npm_marker() {
    cat > "$1/bin/npm" <<'NPM_STUB'
#!/usr/bin/env bash
: > "${_RGS_NPM_MARKER:?_RGS_NPM_MARKER must be set by the test}"
exit 0
NPM_STUB
    chmod +x "$1/bin/npm"
}

# _rgs_stub_curl_stateful <dir> — a curl whose FIRST call reports the port FREE
# and whose every later call reports it SERVED.
#
# Both answers are needed in one run: call 1 is the §1b port preflight, which
# must pass (exit 7 = CURLE_COULDNT_CONNECT, "no listener"), and calls 2+ are
# the §5 readiness poll, which must succeed so the script proceeds to the
# launch. Callers MUST reset "$_RGS_CURL_COUNTER" between script invocations.
_rgs_stub_curl_stateful() {
    cat > "$1/bin/curl" <<'CURL_STUB'
#!/usr/bin/env bash
_c="${_RGS_CURL_COUNTER:?_RGS_CURL_COUNTER must be set by the test}"
_n=0
[ -f "$_c" ] && _n=$(cat "$_c")
_n=$((_n + 1))
printf '%s' "$_n" > "$_c"
[ "$_n" -eq 1 ] && exit 7
exit 0
CURL_STUB
    chmod +x "$1/bin/curl"
}

# _rgs_stub_cargo <dir> [rc] — a cargo that short-circuits the real build.
_rgs_stub_cargo() {
    printf '#!/usr/bin/env bash\nexit %s\n' "${2:-0}" > "$1/bin/cargo"
    chmod +x "$1/bin/cargo"
}

# _rgs_stub_npm_serving <dir> — an npm whose `run dev` stays alive so the
# readiness poll and the script's `wait` behave like the real thing, and whose
# every other subcommand is a no-op.
#
# `exec sleep` (rather than `sleep` as a child) makes the stub process ITSELF
# the sleeping process, so the cleanup trap's `kill "$VITE_PID"` terminates it
# directly — bash defers a signal while a FOREGROUND child runs, so a plain
# `sleep` would only die via the trap's `pkill -P` fallback.
_rgs_stub_npm_serving() {
    cat > "$1/bin/npm" <<'NPM_STUB'
#!/usr/bin/env bash
case "${1:-}" in
    run)
        shift
        case "${1:-}" in
            dev) exec sleep 30 ;;
            *)   exit 0 ;;
        esac
        ;;
    *) exit 0 ;;
esac
NPM_STUB
    chmod +x "$1/bin/npm"
}

# _rgs_stub_gui_binary <dir> <relpath> — a stand-in for the built reify-gui
# that records the environment the launcher handed it into "$_RGS_ENV_DUMP"
# (one KEY=VALUE per line) and exits 0. Lets the tests observe LD_LIBRARY_PATH
# ordering without launching anything real.
_rgs_stub_gui_binary() {
    mkdir -p "$(dirname "$1/$2")"
    cat > "$1/$2" <<'GUI_STUB'
#!/usr/bin/env bash
{
    printf 'LD_LIBRARY_PATH=%s\n' "${LD_LIBRARY_PATH:-}"
    printf 'WEBKIT_DISABLE_DMABUF_RENDERER=%s\n' "${WEBKIT_DISABLE_DMABUF_RENDERER:-}"
} > "${_RGS_ENV_DUMP:?_RGS_ENV_DUMP must be set by the test}"
exit 0
GUI_STUB
    chmod +x "$1/$2"
}

# _rgs_env_dump_get <dump-file> <key> — the recorded value of <key>, or "".
_rgs_env_dump_get() {
    [ -f "$1" ] || return 0
    sed -n "s/^$2=//p" "$1" | head -1
}

# -- Test 25: behavioral — vite-process-death early-exit branch ---------------
echo ""
echo "--- Test 25: run-gui-dev.sh vite-process-death early-exit branch ---"

_rgs_mktemp _t25_tmpdir
_t25_port=$(_rgs_free_port)
_mk_rungui_dev_fixture "$_t25_tmpdir"

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
# secondary guard for the same class of failure. The ambient display is
# scrubbed and then re-pinned for this child process (task 7290) so this test
# can never again silently depend on whether the verify host has one.
_t25_out=$(env -u DISPLAY -u WAYLAND_DISPLAY DISPLAY=:99 \
    REIFY_VITE_PORT="$_t25_port" PATH="$_t25_tmpdir/bin:$PATH" \
    bash "$_t25_tmpdir/scripts/run-gui-dev.sh" "$_t25_tmpdir/test.ri" 2>&1) \
    && _t25_rc=0 || _t25_rc=$?

assert "run-gui-dev.sh: vite-death branch exits non-zero" \
    bash -c '[ "$1" -ne 0 ]' _ "$_t25_rc"

assert "run-gui-dev.sh: vite-death branch emits 'vite process exited'" \
    bash -c 'printf "%s\n" "$1" | grep -qF "vite process exited"' _ "$_t25_out"

assert "run-gui-dev.sh: vite-death branch does NOT hit the 30s timeout message" \
    bash -c '! printf "%s\n" "$1" | grep -qF "did not become ready"' _ "$_t25_out"

# -- Test 26: behavioral — vite-port preflight refuses an occupied port -------
echo ""
echo "--- Test 26: run-gui-dev.sh refuses an already-served vite port ---"

# The readiness loop (§5) calls curl BEFORE checking `kill -0 "$VITE_PID"`, so
# a FOREIGN listener on the port makes curl succeed on iteration 1 and the
# script happily pairs a fresh reify-gui with a stale, unrelated vite — while
# the vite it just spawned has already died on vite.config.ts's strictPort.
# The only ordering that closes this is a PRE-spawn occupancy check, and it has
# to run before the expensive steps: npm install, the sidecar build, the vite
# spawn and a ~2-minute cargo build.

_rgs_mktemp _t26_tmpdir
_t26_port=$(_rgs_free_port)
_mk_rungui_dev_fixture "$_t26_tmpdir"

_rgs_stub_npm_marker "$_t26_tmpdir"
_rgs_stub_cargo "$_t26_tmpdir" 1

# Stub curl: succeeds unconditionally — something is already answering on the
# port, which is precisely the condition the preflight must refuse. (This is
# the one test that does NOT want the stateful stub: here the port is served
# from the very first probe.)
cat > "$_t26_tmpdir/bin/curl" <<'CURL_STUB'
#!/usr/bin/env bash
exit 0
CURL_STUB
chmod +x "$_t26_tmpdir/bin/curl"

# DISPLAY is pinned so this test isolates the PORT gate: the display preflight
# lives in the same block and would otherwise mask it on a headless runner.
_t26_rc=0
_t26_out=$(DISPLAY=:99 REIFY_VITE_PORT="$_t26_port" \
    _RGS_NPM_MARKER="$_t26_tmpdir/npm-invoked" \
    PATH="$_t26_tmpdir/bin:$PATH" \
    bash "$_t26_tmpdir/scripts/run-gui-dev.sh" "$_t26_tmpdir/test.ri" 2>&1) || _t26_rc=$?

assert "run-gui-dev.sh: occupied vite port exits non-zero" \
    bash -c '[ "$1" -ne 0 ]' _ "$_t26_rc"

assert "run-gui-dev.sh: occupied-port error names the port number" \
    bash -c 'printf "%s\n" "$1" | grep -qF "$2"' _ "$_t26_out" "$_t26_port"

assert "run-gui-dev.sh: occupied-port error says the port is already in use" \
    bash -c 'printf "%s\n" "$1" | grep -qiF "already"' _ "$_t26_out"

assert "run-gui-dev.sh: occupied-port refusal happens BEFORE any npm invocation" \
    bash -c '! [ -e "$1" ]' _ "$_t26_tmpdir/npm-invoked"

# The refusal must NOT offer REIFY_VITE_PORT as the remedy. That knob is
# honoured by the vite spawn and the readiness poll only: reify-gui's frontend
# URL comes from gui/src-tauri/tauri.conf.json's `"devUrl":
# "http://localhost:1420"`, which tauri bakes in at COMPILE time, and nothing
# under gui/src-tauri/src/ reads the variable. Taking that advice would move
# OUR vite off :1420 while the launched binary still loads :1420 — the very
# foreign listener this preflight refused — and the launcher would report
# success the whole way. Freeing the port is the only remedy that works, so
# this guards against the suggestion being reintroduced.
assert "run-gui-dev.sh: occupied-port error does NOT advertise REIFY_VITE_PORT as a remedy" \
    bash -c '! printf "%s\n" "$1" | grep -qF "REIFY_VITE_PORT="' _ "$_t26_out"

assert "run-gui-dev.sh: occupied-port error tells the user to free the port" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "free (it|the port)"' _ "$_t26_out"

# Break-glass: REIFY_GUI_SKIP_PREFLIGHT=1 must get past the port gate.
rm -f "$_t26_tmpdir/npm-invoked"
DISPLAY=:99 REIFY_VITE_PORT="$_t26_port" \
    _RGS_NPM_MARKER="$_t26_tmpdir/npm-invoked" \
    REIFY_GUI_SKIP_PREFLIGHT=1 \
    PATH="$_t26_tmpdir/bin:$PATH" \
    bash "$_t26_tmpdir/scripts/run-gui-dev.sh" "$_t26_tmpdir/test.ri" >/dev/null 2>&1 || true

assert "run-gui-dev.sh: REIFY_GUI_SKIP_PREFLIGHT=1 bypasses the port gate" \
    test -e "$_t26_tmpdir/npm-invoked"

# -- Test 27: behavioral — display preflight fails fast, both launchers ------
echo ""
echo "--- Test 27: launchers refuse to build with no display ---"

# reify-gui is a GTK/WebKit app: with neither DISPLAY nor WAYLAND_DISPLAY it
# cannot open a window at all. Without a gate the user pays npm install plus a
# ~2-minute cargo build before finding that out, so the check must precede
# every expensive step in BOTH launchers.

# --- 27a: run-gui-dev.sh ---
_rgs_mktemp _t27dev_tmpdir
_t27dev_port=$(_rgs_free_port)
_mk_rungui_dev_fixture "$_t27dev_tmpdir"
_rgs_stub_npm_marker "$_t27dev_tmpdir"
_rgs_stub_curl_stateful "$_t27dev_tmpdir"
_rgs_stub_cargo "$_t27dev_tmpdir"

# _t27_run_dev <extra-env-assignment...> — reset both bits of fixture state
# (the npm marker and the stateful-curl counter) and run the dev launcher.
_t27_run_dev() {
    rm -f "$_t27dev_tmpdir/npm-invoked" "$_t27dev_tmpdir/curl-count"
    env -u DISPLAY -u WAYLAND_DISPLAY \
        REIFY_VITE_PORT="$_t27dev_port" \
        _RGS_NPM_MARKER="$_t27dev_tmpdir/npm-invoked" \
        _RGS_CURL_COUNTER="$_t27dev_tmpdir/curl-count" \
        PATH="$_t27dev_tmpdir/bin:$PATH" \
        "$@" \
        bash "$_t27dev_tmpdir/scripts/run-gui-dev.sh" "$_t27dev_tmpdir/test.ri" 2>&1
}

_t27dev_rc=0
_t27dev_out=$(_t27_run_dev) || _t27dev_rc=$?

assert "run-gui-dev.sh: no display exits non-zero" \
    bash -c '[ "$1" -ne 0 ]' _ "$_t27dev_rc"

assert "run-gui-dev.sh: no-display error names DISPLAY on a single line" \
    bash -c 'printf "%s\n" "$1" | grep -F DISPLAY | grep -qiE "display|unset"' _ "$_t27dev_out"

assert "run-gui-dev.sh: no-display refusal happens BEFORE any npm invocation" \
    bash -c '! [ -e "$1" ]' _ "$_t27dev_tmpdir/npm-invoked"

_t27_run_dev DISPLAY=:99 >/dev/null 2>&1 || true
assert "run-gui-dev.sh: DISPLAY=:99 gets past the display gate" \
    test -e "$_t27dev_tmpdir/npm-invoked"

_t27_run_dev REIFY_GUI_SKIP_PREFLIGHT=1 >/dev/null 2>&1 || true
assert "run-gui-dev.sh: REIFY_GUI_SKIP_PREFLIGHT=1 bypasses the display gate" \
    test -e "$_t27dev_tmpdir/npm-invoked"

# --- 27b: run-gui.sh ---
_rgs_mktemp _t27rel_tmpdir
_mk_rungui_fixture "$_t27rel_tmpdir"
_rgs_stub_npm_marker "$_t27rel_tmpdir"
_rgs_stub_cargo "$_t27rel_tmpdir"

_t27_run_rel() {
    rm -f "$_t27rel_tmpdir/npm-invoked"
    env -u DISPLAY -u WAYLAND_DISPLAY \
        _RGS_NPM_MARKER="$_t27rel_tmpdir/npm-invoked" \
        PATH="$_t27rel_tmpdir/bin:$PATH" \
        "$@" \
        bash "$_t27rel_tmpdir/scripts/run-gui.sh" "$_t27rel_tmpdir/test.ri" 2>&1
}

_t27rel_rc=0
_t27rel_out=$(_t27_run_rel) || _t27rel_rc=$?

assert "run-gui.sh: no display exits non-zero" \
    bash -c '[ "$1" -ne 0 ]' _ "$_t27rel_rc"

assert "run-gui.sh: no-display error names DISPLAY on a single line" \
    bash -c 'printf "%s\n" "$1" | grep -F DISPLAY | grep -qiE "display|unset"' _ "$_t27rel_out"

assert "run-gui.sh: no-display refusal happens BEFORE any npm invocation" \
    bash -c '! [ -e "$1" ]' _ "$_t27rel_tmpdir/npm-invoked"

_t27_run_rel DISPLAY=:99 >/dev/null 2>&1 || true
assert "run-gui.sh: DISPLAY=:99 gets past the display gate" \
    test -e "$_t27rel_tmpdir/npm-invoked"

_t27_run_rel REIFY_GUI_SKIP_PREFLIGHT=1 >/dev/null 2>&1 || true
assert "run-gui.sh: REIFY_GUI_SKIP_PREFLIGHT=1 bypasses the display gate" \
    test -e "$_t27rel_tmpdir/npm-invoked"

# -- Test 28: behavioral — the tbb pin dir leads an inherited LD_LIBRARY_PATH -
echo ""
echo "--- Test 28: launchers prepend /opt/reify-deps/tbb-pin ahead of the caller's LD_LIBRARY_PATH ---"

# #5192 pinned oneTBB by putting /opt/reify-deps/tbb-pin FIRST in each binary's
# DT_RUNPATH. But the loader searches LD_LIBRARY_PATH *before* DT_RUNPATH, so a
# caller whose LD_LIBRARY_PATH names /usr/lib/x86_64-linux-gnu silently defeats
# that: the binary binds the system libtbb 12.11 instead of the deps 12.18 and
# dies on a missing symbol. Measured on this host:
#   LD_LIBRARY_PATH=/usr/lib/x86_64-linux-gnu:/opt/reify-deps/lib
#     -> libtbb.so.12 => /usr/lib/x86_64-linux-gnu/libtbb.so.12   (12.11, bad)
#   LD_LIBRARY_PATH=/opt/reify-deps/tbb-pin:<the same>
#     -> libtbb.so.12 => /opt/reify-deps/tbb-pin/libtbb.so.12      (12.18, good)
# So the launchers must lead with the pin dir while PRESERVING what they were
# handed.

# Cheap text drift-guards — they run on every host, including one with no deps
# tree, where the behavioural half below is skipped entirely and these are the
# ONLY coverage of the whole tbb-pin mechanism.
#
# Every guard here is therefore anchored to an EXECUTABLE line. A bare
# `grep -F /opt/reify-deps/tbb-pin` would also be satisfied by the header
# comments that merely mention the path, so deleting the entire pin block could
# leave it green — the same trap Test 31's spawn guard already avoids by
# anchoring to `^[[:space:]]*npm run dev`.
#
# The pin itself now lives in scripts/lib_gui_launch.sh (#7254 amendment), so
# the guards come in two halves: the lib must still implement the pin, and each
# launcher must still source the lib and CALL it. Neither half alone is enough
# — a launcher that dropped the call would otherwise pass on a host where the
# behavioural half is skipped.
assert "scripts/lib_gui_launch.sh assigns GUI_LAUNCH_TBB_PIN_DIR=/opt/reify-deps/tbb-pin" \
    bash -c 'grep -qE "^[[:space:]]*GUI_LAUNCH_TBB_PIN_DIR=\"?/opt/reify-deps/tbb-pin\"?$" "$1"' _ "$LIB_GUI_LAUNCH"

assert "scripts/lib_gui_launch.sh prepends GUI_LAUNCH_TBB_PIN_DIR to LD_LIBRARY_PATH" \
    bash -c 'grep -qE "^[[:space:]]*export LD_LIBRARY_PATH=\"\\\$GUI_LAUNCH_TBB_PIN_DIR" "$1"' _ "$LIB_GUI_LAUNCH"

for _t28_script in "$RUN_GUI_DEV" "$RUN_GUI"; do
    _t28_name="$(basename "$_t28_script")"

    assert "scripts/$_t28_name sources lib_gui_launch.sh" \
        bash -c 'grep -qE "^[[:space:]]*(source|\\.) .*lib_gui_launch\.sh" "$1"' _ "$_t28_script"

    assert "scripts/$_t28_name calls gui_launch_env_pin" \
        bash -c 'grep -qE "^[[:space:]]*gui_launch_env_pin[[:space:]]*$" "$1"' _ "$_t28_script"

    assert "scripts/$_t28_name calls gui_launch_preflight_display" \
        bash -c 'grep -qE "^[[:space:]]*gui_launch_preflight_display\b" "$1"' _ "$_t28_script"
done

# The behavioural half needs the real pin dir: the scripts only prepend a dir
# that EXISTS. Mirrors the tbb_pin_present() host gate in
# gui/src-tauri/tests/rpath_smoke.rs.
_t28_inherited="/usr/lib/x86_64-linux-gnu:/opt/reify-deps/lib"

if [ -d /opt/reify-deps/tbb-pin ]; then
    # --- 28a: run-gui-dev.sh ---
    _rgs_mktemp _t28dev_tmpdir
    _t28dev_port=$(_rgs_free_port)
    _mk_rungui_dev_fixture "$_t28dev_tmpdir"
    _rgs_stub_npm_serving "$_t28dev_tmpdir"
    _rgs_stub_curl_stateful "$_t28dev_tmpdir"
    _rgs_stub_cargo "$_t28dev_tmpdir"
    _rgs_stub_gui_binary "$_t28dev_tmpdir" target/debug/reify-gui

    _t28dev_out=$(DISPLAY=:99 \
        LD_LIBRARY_PATH="$_t28_inherited" \
        REIFY_VITE_PORT="$_t28dev_port" \
        _RGS_CURL_COUNTER="$_t28dev_tmpdir/curl-count" \
        _RGS_ENV_DUMP="$_t28dev_tmpdir/env-dump" \
        PATH="$_t28dev_tmpdir/bin:$PATH" \
        bash "$_t28dev_tmpdir/scripts/run-gui-dev.sh" "$_t28dev_tmpdir/test.ri" 2>&1) || true
    _t28dev_llp=$(_rgs_env_dump_get "$_t28dev_tmpdir/env-dump" LD_LIBRARY_PATH)

    assert "run-gui-dev.sh: launched binary saw a non-empty LD_LIBRARY_PATH" \
        bash -c '[ -n "$1" ]' _ "$_t28dev_llp"

    assert "run-gui-dev.sh: first LD_LIBRARY_PATH entry is exactly /opt/reify-deps/tbb-pin" \
        bash -c '[ "${1%%:*}" = /opt/reify-deps/tbb-pin ]' _ "$_t28dev_llp"

    assert "run-gui-dev.sh: inherited /usr/lib/x86_64-linux-gnu is preserved" \
        bash -c 'printf "%s\n" "$1" | grep -qF /usr/lib/x86_64-linux-gnu' _ "$_t28dev_llp"

    assert "run-gui-dev.sh: inherited /opt/reify-deps/lib is preserved" \
        bash -c 'printf "%s\n" "$1" | grep -qF /opt/reify-deps/lib' _ "$_t28dev_llp"

    assert "run-gui-dev.sh: emits a notice mentioning LD_LIBRARY_PATH" \
        bash -c 'printf "%s\n" "$1" | grep -qF LD_LIBRARY_PATH' _ "$_t28dev_out"

    # --- 28b: run-gui.sh ---
    _rgs_mktemp _t28rel_tmpdir
    _mk_rungui_fixture "$_t28rel_tmpdir"
    _rgs_stub_npm_serving "$_t28rel_tmpdir"
    _rgs_stub_cargo "$_t28rel_tmpdir"
    _rgs_stub_gui_binary "$_t28rel_tmpdir" target/release/reify-gui

    _t28rel_out=$(DISPLAY=:99 \
        LD_LIBRARY_PATH="$_t28_inherited" \
        _RGS_ENV_DUMP="$_t28rel_tmpdir/env-dump" \
        PATH="$_t28rel_tmpdir/bin:$PATH" \
        bash "$_t28rel_tmpdir/scripts/run-gui.sh" "$_t28rel_tmpdir/test.ri" 2>&1) || true
    _t28rel_llp=$(_rgs_env_dump_get "$_t28rel_tmpdir/env-dump" LD_LIBRARY_PATH)

    assert "run-gui.sh: launched binary saw a non-empty LD_LIBRARY_PATH" \
        bash -c '[ -n "$1" ]' _ "$_t28rel_llp"

    assert "run-gui.sh: first LD_LIBRARY_PATH entry is exactly /opt/reify-deps/tbb-pin" \
        bash -c '[ "${1%%:*}" = /opt/reify-deps/tbb-pin ]' _ "$_t28rel_llp"

    assert "run-gui.sh: inherited /usr/lib/x86_64-linux-gnu is preserved" \
        bash -c 'printf "%s\n" "$1" | grep -qF /usr/lib/x86_64-linux-gnu' _ "$_t28rel_llp"

    assert "run-gui.sh: inherited /opt/reify-deps/lib is preserved" \
        bash -c 'printf "%s\n" "$1" | grep -qF /opt/reify-deps/lib' _ "$_t28rel_llp"

    assert "run-gui.sh: emits a notice mentioning LD_LIBRARY_PATH" \
        bash -c 'printf "%s\n" "$1" | grep -qF LD_LIBRARY_PATH' _ "$_t28rel_out"
else
    echo "  SKIP: /opt/reify-deps/tbb-pin absent on this host — behavioural half"
    echo "  SKIP: needs the deps tree (scripts/build-manifold-deps.sh); the text"
    echo "  SKIP: drift-guards above still ran."
fi

# -- Test 29: behavioral — WEBKIT_DISABLE_DMABUF_RENDERER defaults to 1 -------
echo ""
echo "--- Test 29: launchers default WEBKIT_DISABLE_DMABUF_RENDERER=1, caller may override ---"

# On an NVIDIA host, Mesa's EGL cannot create a GBM screen on the NVIDIA GPU and
# WebKitGTK aborts with `Could not create GBM EGL display: EGL_NOT_INITIALIZED`.
# gui/test/visual/lib_e2e_smoke.sh §2b already carries the canonical
# `${WEBKIT_DISABLE_DMABUF_RENDERER:-1}` stanza for exactly this; the launchers
# do not. The `:-1` form matters as much as the value: a caller who knows DMABUF
# works on their host must still be able to turn it back on.
#
# Unlike Test 28 this is host-independent — no deps-tree gate.

# --- 29a: run-gui-dev.sh ---
_rgs_mktemp _t29dev_tmpdir
_t29dev_port=$(_rgs_free_port)
_mk_rungui_dev_fixture "$_t29dev_tmpdir"
_rgs_stub_npm_serving "$_t29dev_tmpdir"
_rgs_stub_curl_stateful "$_t29dev_tmpdir"
_rgs_stub_cargo "$_t29dev_tmpdir"
_rgs_stub_gui_binary "$_t29dev_tmpdir" target/debug/reify-gui

# _t29_run_dev <extra-env...> — reset the stateful-curl counter and the dump,
# then run the dev launcher with WEBKIT_DISABLE_DMABUF_RENDERER unset.
_t29_run_dev() {
    rm -f "$_t29dev_tmpdir/curl-count" "$_t29dev_tmpdir/env-dump"
    env -u WEBKIT_DISABLE_DMABUF_RENDERER \
        DISPLAY=:99 \
        REIFY_VITE_PORT="$_t29dev_port" \
        _RGS_CURL_COUNTER="$_t29dev_tmpdir/curl-count" \
        _RGS_ENV_DUMP="$_t29dev_tmpdir/env-dump" \
        PATH="$_t29dev_tmpdir/bin:$PATH" \
        "$@" \
        bash "$_t29dev_tmpdir/scripts/run-gui-dev.sh" "$_t29dev_tmpdir/test.ri" >/dev/null 2>&1 || true
}

_t29_run_dev
_t29dev_default=$(_rgs_env_dump_get "$_t29dev_tmpdir/env-dump" WEBKIT_DISABLE_DMABUF_RENDERER)

assert "run-gui-dev.sh: launched binary sees WEBKIT_DISABLE_DMABUF_RENDERER=1 by default" \
    bash -c '[ "$1" = 1 ]' _ "$_t29dev_default"

_t29_run_dev WEBKIT_DISABLE_DMABUF_RENDERER=0
_t29dev_override=$(_rgs_env_dump_get "$_t29dev_tmpdir/env-dump" WEBKIT_DISABLE_DMABUF_RENDERER)

assert "run-gui-dev.sh: an explicit WEBKIT_DISABLE_DMABUF_RENDERER=0 is NOT clobbered" \
    bash -c '[ "$1" = 0 ]' _ "$_t29dev_override"

# --- 29b: run-gui.sh ---
_rgs_mktemp _t29rel_tmpdir
_mk_rungui_fixture "$_t29rel_tmpdir"
_rgs_stub_npm_serving "$_t29rel_tmpdir"
_rgs_stub_cargo "$_t29rel_tmpdir"
_rgs_stub_gui_binary "$_t29rel_tmpdir" target/release/reify-gui

_t29_run_rel() {
    rm -f "$_t29rel_tmpdir/env-dump"
    env -u WEBKIT_DISABLE_DMABUF_RENDERER \
        DISPLAY=:99 \
        _RGS_ENV_DUMP="$_t29rel_tmpdir/env-dump" \
        PATH="$_t29rel_tmpdir/bin:$PATH" \
        "$@" \
        bash "$_t29rel_tmpdir/scripts/run-gui.sh" "$_t29rel_tmpdir/test.ri" >/dev/null 2>&1 || true
}

_t29_run_rel
_t29rel_default=$(_rgs_env_dump_get "$_t29rel_tmpdir/env-dump" WEBKIT_DISABLE_DMABUF_RENDERER)

assert "run-gui.sh: launched binary sees WEBKIT_DISABLE_DMABUF_RENDERER=1 by default" \
    bash -c '[ "$1" = 1 ]' _ "$_t29rel_default"

_t29_run_rel WEBKIT_DISABLE_DMABUF_RENDERER=0
_t29rel_override=$(_rgs_env_dump_get "$_t29rel_tmpdir/env-dump" WEBKIT_DISABLE_DMABUF_RENDERER)

assert "run-gui.sh: an explicit WEBKIT_DISABLE_DMABUF_RENDERER=0 is NOT clobbered" \
    bash -c '[ "$1" = 0 ]' _ "$_t29rel_override"

# -- Test 30: behavioral — a failed launch leaves no orphaned vite descendant -
echo ""
echo "--- Test 30: run-gui-dev.sh reaps the whole vite process tree ---"

# `npm run dev` forks an intermediate `sh -c` which forks the node/vite process
# that actually holds the port. cleanup()'s `pkill -P "$VITE_PID"` reaches npm's
# DIRECT children only, so that GRANDCHILD survives every termination path —
# observed twice in the wild squatting on :1420 with cwd <worktree>/gui, which
# is what makes the next dev run fail to start.
#
# The stub below reproduces that exact npm -> sh -> node shape, and the stub
# reify-gui exits 1 to simulate the symbol-lookup/EGL death at exec.

_rgs_mktemp _t30_tmpdir
_t30_port=$(_rgs_free_port)
_mk_rungui_dev_fixture "$_t30_tmpdir"
_rgs_stub_cargo "$_t30_tmpdir"

cat > "$_t30_tmpdir/bin/npm" <<'NPM_STUB'
#!/usr/bin/env bash
case "${1:-}" in
    run)
        shift
        [ "${1:-}" = dev ] || exit 0
        printf '%s' "$$" > "${_RGS_NPM_PID:?_RGS_NPM_PID must be set by the test}"
        # npm -> sh -> node: an intermediate shell that BACKGROUNDS the process
        # actually holding the port, then waits. The backgrounded sleep is the
        # grandchild `pkill -P "$VITE_PID"` cannot see.
        bash -c 'sleep 300 & printf "%s" "$!" > "${_RGS_GRANDCHILD_PID:?}"; wait' &
        wait
        exit 0
        ;;
    *) exit 0 ;;
esac
NPM_STUB
chmod +x "$_t30_tmpdir/bin/npm"

# Readiness must not race the fixture: report the port free on the preflight
# probe, then "up" only once the stub vite tree has fully materialised.
cat > "$_t30_tmpdir/bin/curl" <<'CURL_STUB'
#!/usr/bin/env bash
_c="${_RGS_CURL_COUNTER:?_RGS_CURL_COUNTER must be set by the test}"
_n=0
[ -f "$_c" ] && _n=$(cat "$_c")
_n=$((_n + 1))
printf '%s' "$_n" > "$_c"
[ "$_n" -eq 1 ] && exit 7
[ -s "${_RGS_GRANDCHILD_PID:?}" ] && exit 0
exit 7
CURL_STUB
chmod +x "$_t30_tmpdir/bin/curl"

# Stub reify-gui: dies at exec, the way a libtbb symbol-lookup or EGL abort does.
mkdir -p "$_t30_tmpdir/target/debug"
printf '#!/usr/bin/env bash\nexit 1\n' > "$_t30_tmpdir/target/debug/reify-gui"
chmod +x "$_t30_tmpdir/target/debug/reify-gui"

_t30_rc=0
DISPLAY=:99 REIFY_VITE_PORT="$_t30_port" \
    _RGS_CURL_COUNTER="$_t30_tmpdir/curl-count" \
    _RGS_NPM_PID="$_t30_tmpdir/vite-npm.pid" \
    _RGS_GRANDCHILD_PID="$_t30_tmpdir/vite-grandchild.pid" \
    PATH="$_t30_tmpdir/bin:$PATH" \
    bash "$_t30_tmpdir/scripts/run-gui-dev.sh" "$_t30_tmpdir/test.ri" >/dev/null 2>&1 || _t30_rc=$?

_t30_npm_pid=$(cat "$_t30_tmpdir/vite-npm.pid" 2>/dev/null || true)
_t30_gc_pid=$(cat "$_t30_tmpdir/vite-grandchild.pid" 2>/dev/null || true)

# Bounded settle loop — reaping is asynchronous, so poll for up to ~5s rather
# than sleeping a fixed interval and hoping.
_t30_alive() { [ -n "$1" ] && kill -0 "$1" 2>/dev/null; }
for _ in $(seq 1 50); do
    if ! _t30_alive "$_t30_npm_pid" && ! _t30_alive "$_t30_gc_pid"; then
        break
    fi
    sleep 0.1
done

_t30_npm_survived=0
_t30_gc_survived=0
if _t30_alive "$_t30_npm_pid"; then _t30_npm_survived=1; fi
if _t30_alive "$_t30_gc_pid"; then _t30_gc_survived=1; fi

# Unconditional backstop: a RED run must never leak a process into the pool.
if [ -n "$_t30_gc_pid" ]; then kill -9 "$_t30_gc_pid" 2>/dev/null || true; fi
if [ -n "$_t30_npm_pid" ]; then kill -9 "$_t30_npm_pid" 2>/dev/null || true; fi

assert "run-gui-dev.sh: the failed binary's non-zero rc is propagated" \
    bash -c '[ "$1" -ne 0 ]' _ "$_t30_rc"

assert "run-gui-dev.sh: the stub vite tree was actually built (grandchild pid recorded)" \
    bash -c '[ -n "$1" ]' _ "$_t30_gc_pid"

assert "run-gui-dev.sh: no orphaned vite GRANDCHILD survives the script" \
    bash -c '[ "$1" -eq 0 ]' _ "$_t30_gc_survived"

assert "run-gui-dev.sh: the npm process itself does not survive the script" \
    bash -c '[ "$1" -eq 0 ]' _ "$_t30_npm_survived"

# -- Test 31: behavioral — vite must not inherit the controlling terminal -----
echo ""
echo "--- Test 31: run-gui-dev.sh spawns vite with stdin detached from the tty ---"

# Bash gives an asynchronous command /dev/null stdin only while job control is
# OFF. Test 30's fix put the vite spawn under `set -m`, which silently handed
# the backgrounded npm the launcher's CONTROLLING TERMINAL while leaving it in
# a BACKGROUND process group — the classic recipe for SIGTTIN/SIGTTOU. vite 6
# gates its CLI shortcuts on `process.stdin.isTTY` and then builds a readline
# over stdin, so a dev server started that way can be STOPPED by the kernel:
# `kill -0 "$VITE_PID"` still succeeds on a stopped process, so §5's
# vite-death branch never fires and the launcher waits on a frozen server.
#
# CRITICAL fixture mechanic: this is observable ONLY when the launcher has a
# controlling terminal. The suite otherwise runs with no tty at all — fd 0 is
# already not a terminal — so it is structurally blind to the regression, which
# is exactly why every earlier behavioural test passed while the defect
# shipped. `script` supplies the pty.

# pty-INDEPENDENT drift-guard: runs on every host, including one with no
# util-linux `script`. Anchored to a line STARTING with `npm run dev` so it
# checks the actual spawn and cannot be satisfied by a comment that merely
# mentions the redirect.
assert "scripts/run-gui-dev.sh spawns 'npm run dev' with stdin redirected from /dev/null" \
    bash -c 'grep -E "^[[:space:]]*npm run dev" "$1" | grep -qF "</dev/null"' _ "$RUN_GUI_DEV"

# util-linux `script` is present here (2.39.3) but is NOT in scripts/setup-dev.sh's
# APT_PACKAGES/TAURI_DEPS, so it must never be hard-required. Same host-gate
# shape as Test 28's `[ -d /opt/reify-deps/tbb-pin ]`.
if command -v script >/dev/null 2>&1; then
    _rgs_mktemp _t31_tmpdir
    _t31_port=$(_rgs_free_port)
    _mk_rungui_dev_fixture "$_t31_tmpdir"
    _rgs_stub_cargo "$_t31_tmpdir"

    # npm stub: `run dev` records its OWN stdin identity before staying alive.
    # The command substitution inherits this shell's fd 0, so /proc/self/fd/0
    # resolves to the same file; `> file` redirects stdout only and leaves
    # fd 0 untouched. `exec sleep` for the same reason as
    # _rgs_stub_npm_serving: this pid must BE the long-lived process.
    cat > "$_t31_tmpdir/bin/npm" <<'NPM_STUB'
#!/usr/bin/env bash
case "${1:-}" in
    run)
        shift
        [ "${1:-}" = dev ] || exit 0
        printf '%s' "$$" > "${_RGS_NPM_PID:?_RGS_NPM_PID must be set by the test}"
        {
            printf 'fd0=%s\n' "$(readlink /proc/self/fd/0 2>/dev/null || echo unknown)"
            if [ -t 0 ]; then printf 'isatty=yes\n'; else printf 'isatty=no\n'; fi
        } > "${_RGS_STDIN_DUMP:?_RGS_STDIN_DUMP must be set by the test}"
        exec sleep 30
        ;;
    *) exit 0 ;;
esac
NPM_STUB
    chmod +x "$_t31_tmpdir/bin/npm"

    # Readiness must not race the fixture: report the port FREE on the §1b
    # preflight probe, then "up" only once the stub has recorded its stdin.
    cat > "$_t31_tmpdir/bin/curl" <<'CURL_STUB'
#!/usr/bin/env bash
_c="${_RGS_CURL_COUNTER:?_RGS_CURL_COUNTER must be set by the test}"
_n=0
[ -f "$_c" ] && _n=$(cat "$_c")
_n=$((_n + 1))
printf '%s' "$_n" > "$_c"
[ "$_n" -eq 1 ] && exit 7
[ -s "${_RGS_STDIN_DUMP:?_RGS_STDIN_DUMP must be set by the test}" ] && exit 0
exit 7
CURL_STUB
    chmod +x "$_t31_tmpdir/bin/curl"

    # A reify-gui that exits at once, so the script reaches its own exit and
    # the cleanup trap tears the stub vite down.
    mkdir -p "$_t31_tmpdir/target/debug"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$_t31_tmpdir/target/debug/reify-gui"
    chmod +x "$_t31_tmpdir/target/debug/reify-gui"

    script -qec "env DISPLAY=:99 \
        REIFY_VITE_PORT=$_t31_port \
        _RGS_CURL_COUNTER=$_t31_tmpdir/curl-count \
        _RGS_NPM_PID=$_t31_tmpdir/vite-npm.pid \
        _RGS_STDIN_DUMP=$_t31_tmpdir/vite-stdin \
        PATH=$_t31_tmpdir/bin:$PATH \
        bash $_t31_tmpdir/scripts/run-gui-dev.sh $_t31_tmpdir/test.ri" /dev/null \
        >/dev/null 2>&1 || true

    _t31_npm_pid=$(cat "$_t31_tmpdir/vite-npm.pid" 2>/dev/null || true)
    _t31_fd0=$(_rgs_env_dump_get "$_t31_tmpdir/vite-stdin" fd0)
    _t31_isatty=$(_rgs_env_dump_get "$_t31_tmpdir/vite-stdin" isatty)

    # Unconditional backstop (same shape as Test 30): a RED run must never leak
    # the backgrounded stub npm into the lane pool.
    if [ -n "$_t31_npm_pid" ]; then kill -9 "$_t31_npm_pid" 2>/dev/null || true; fi

    assert "run-gui-dev.sh: the pty fixture actually recorded vite's stdin" \
        bash -c '[ -n "$1" ]' _ "$_t31_fd0"

    assert "run-gui-dev.sh: vite's stdin is exactly /dev/null" \
        bash -c '[ "$1" = /dev/null ]' _ "$_t31_fd0"

    assert "run-gui-dev.sh: vite's stdin is NOT a /dev/pts/* terminal device" \
        bash -c 'case "$1" in /dev/pts/*) exit 1 ;; *) exit 0 ;; esac' _ "$_t31_fd0"

    assert "run-gui-dev.sh: vite observes '[ -t 0 ]' as FALSE" \
        bash -c '[ "$1" = no ]' _ "$_t31_isatty"
else
    echo "  SKIP: util-linux \`script\` absent on this host — the pty half of"
    echo "  SKIP: Test 31 needs a controlling terminal to be observable at all;"
    echo "  SKIP: the text drift-guard above still ran."
fi

# -- Test 32: suite invariant — hermetic DISPLAY pin survives every test -----
echo ""
echo "--- Test 32: suite-level DISPLAY=:99 pin still holds at end of suite ---"

# Placed LAST deliberately: this asserts the hermetic pin (a suite-level
# `export DISPLAY=:99` plus `unset WAYLAND_DISPLAY`, task 7290) still holds
# after every preceding test has run. A test that must drive the NO-display
# gate scrubs both vars for its CHILD process only (env -u DISPLAY -u
# WAYLAND_DISPLAY, as Test 27's `_t27_run_dev` / `_t27_run_rel` helpers do)
# — never at suite scope with a bare `unset DISPLAY` / `export DISPLAY=`. A
# suite-scope scrub would silently re-break every later behavioural launcher
# test on a headless verify host, and nothing else in this suite would
# notice. Asserting the exact value `:99` for DISPLAY (not mere
# non-emptiness) and emptiness for WAYLAND_DISPLAY keeps this guard
# host-independent on both halves of the gate: gui_launch_preflight_display()
# passes if EITHER var is non-empty, so a future edit that scrubs only
# DISPLAY at suite scope would still pass vacuously on a Wayland dev box
# (ambient WAYLAND_DISPLAY non-empty) while reproducing the esc-7014-1 split
# on a headless host — recreated on the other variable.
assert "suite: the hermetic DISPLAY/WAYLAND_DISPLAY pin survives every preceding test" \
    bash -c '[ "${1:-}" = ":99" ] && [ -z "${2:-}" ]' _ "${DISPLAY:-}" "${WAYLAND_DISPLAY:-}"

test_summary
