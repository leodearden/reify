#!/usr/bin/env bash
# Shared lifecycle for the gui/test/visual e2e smoke runners (task 5596).
#
# SOURCED, never executed.  The sourcing runner owns `set -euo pipefail`; this
# file deliberately sets no shell options of its own so it cannot silently
# change the options of a caller that sourced it mid-script.
#
# WHY THIS EXISTS.  Six runners (run_find_uses_smoke.sh, run_appearance_e2e_smoke.sh,
# run_diagnostics_e2e_smoke.sh, run_multi_pane_e2e_smoke.sh,
# run_surface_finish_viewport_e2e_smoke.sh, run_mesh_count_parity_e2e_smoke.sh)
# each carried a byte-identical copy of this lifecycle, differing in exactly
# three substantive values: the echo prefix, the fixture and the driver .mjs.
# The copies had already drifted — run_find_uses_smoke.sh (the oldest, task-4202)
# lacked the /opt/reify-deps LD_LIBRARY_PATH block and the WebKit DMABuf
# disable that the other five carried.  Consolidation unifies on the SUPERSET of
# the env hygiene: both blocks are conditional/defaulted (the deps block fires
# only when the deps dir is actually present; the WebKit one honours a caller
# override), so neither can regress a host where they were previously absent.
#
# ENTRY POINT:
#   e2e_smoke_run --name <label> --fixture <repo-relative> --driver <repo-relative>
#
# The lifecycle, in order:
#   1. Resolve REIFY_DEBUG_PORT (env if valid 1..65535, else allocate a free port).
#   2. Set DISPLAY="${DISPLAY:-:0}" so the Tauri webview can instantiate.
#  2a. Prepend /opt/reify-deps/lib to LD_LIBRARY_PATH when present.
#  2b. Disable the WebKit GBM/DMABuf renderer unless the caller overrode it.
#   3. Install an EXIT/INT/TERM cleanup trap so the GUI is reaped even on early failure.
#   4. (Unless REIFY_SMOKE_SKIP_PREBUILD=1) run synchronous pre-build steps —
#      sidecar npm install, sidecar build, gui npm install, cargo build reify-gui —
#      so the cold-build cost is paid OUTSIDE the readiness window.
#   5. Background REIFY_SMOKE_LAUNCHER (default: scripts/run-gui-dev.sh) with the
#      fixture + REIFY_DEBUG=1 / REIFY_DEBUG_PORT=$PORT.
#   6. Poll the debug health endpoint up to REIFY_SMOKE_WAIT_MS (default 180000ms)
#      with a kill -0 liveness check on each iteration — abort early if the
#      launcher dies rather than waiting the full budget.
#   7. Run `node <driver>`.
#      Note: each driver contains its own waitForServer(60_000) readiness loop —
#      that loop passes instantly because step 6 already confirmed the server is
#      ready.  Both readiness gates are intentional and serve distinct purposes:
#      step 6 polls with a kill -0 liveness check (bash-side, driver-agnostic);
#      the driver's waitForServer is part of its own unchanged contract.
#      Do NOT remove one believing the other is dead code.
#   8. SIGTERM the launcher so its trap reaps both vite and reify-gui, then exit
#      with the driver's exit code.
#
# Lifecycle design mirrors gui/test/visual/run.ts spawnGui/waitForDebugServer/reapGui.
#
# The launcher-death contract is pinned behaviorally — across ALL SIX runners,
# with a negative control — by tests/infra/test_find_uses_smoke_runner.sh, and
# additionally (for the 5367-specific guard) by
# tests/infra/test_mesh_count_parity_smoke_runner.sh.  Both drive a real runner
# through REIFY_SMOKE_SKIP_PREBUILD / REIFY_SMOKE_LAUNCHER / REIFY_SMOKE_WAIT_MS.

# ---------------------------------------------------------------------------
# Resolve the repo root from THIS file's location rather than the caller's cwd:
# .../gui/test/visual/lib_e2e_smoke.sh -> up 3 -> repo root.
# ---------------------------------------------------------------------------
E2E_SMOKE_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
E2E_SMOKE_REPO_ROOT="$(cd "$E2E_SMOKE_LIB_DIR/../../.." && pwd)"

# ---------------------------------------------------------------------------
# 1. Resolve REIFY_DEBUG_PORT
#    Mirrors endpoint.ts resolveDebugPort / allocateFreePort semantics and
#    the setup-worktree-debug-port.sh contract: strict ^[0-9]+$ pattern,
#    value 1..65535, no whitespace.
# ---------------------------------------------------------------------------
resolve_port() {
    local raw="${REIFY_DEBUG_PORT:-}"
    if [[ "$raw" =~ ^[0-9]+$ ]] && [ "$raw" -ge 1 ] && [ "$raw" -le 65535 ]; then
        echo "$raw"
        return
    fi
    # Fall back to allocating a free ephemeral port (lib_portable.sh pattern).
    # shellcheck source=../../../scripts/lib_portable.sh
    source "$E2E_SMOKE_REPO_ROOT/scripts/lib_portable.sh"
    allocate_free_port
}

# ---------------------------------------------------------------------------
# THE SINGLE EMISSION POINT for launcher death.
#
# Both death sites (readiness-loop abort and post-driver post-mortem) route
# through here, so the marker grammar and the human line exist in exactly one
# place — the anti-duplication point of this whole consolidation.
#
# Two lines are emitted on stderr for two different readers:
#
#   * the HUMAN line retains the exact substring `launcher exited early (rc=`.
#     tests/infra/test_mesh_count_parity_smoke_runner.sh asserts that substring
#     with `grep -qF`; keeping it byte-compatible lets that guard stay
#     functionally UNCHANGED across this consolidation, which is stronger
#     no-regression evidence than editing it to match a new string.
#
#   * the MACHINE marker
#         E2E_SMOKE_LAUNCHER_DEATH phase=<readiness|post-driver> rc=<n> pid=<n>
#     is a token no normal run can produce.  The predicate it replaces
#     (`grep -qiE "launcher|exited|early|died|liveness|kill"`) was MEASURED to
#     match the happy-path startup banner `<name>: launcher PID=<pid>`, so it
#     passed even when the launcher never died.  The `phase=` field is what
#     makes the two death sites separately assertable.
# ---------------------------------------------------------------------------
_e2e_smoke_report_launcher_death() {
    local name="$1" phase="$2" rc="$3" pid="$4"
    echo "${name}: launcher exited early (rc=${rc}) during ${phase} phase (PID=${pid})" >&2
    echo "E2E_SMOKE_LAUNCHER_DEATH phase=${phase} rc=${rc} pid=${pid}" >&2
}

# ---------------------------------------------------------------------------
# e2e_smoke_run --name <label> --fixture <repo-relative> --driver <repo-relative>
#
#   --name     echo prefix, used on every log line this lifecycle emits.  MUST
#              equal the runner's historical prefix so live-run expectations and
#              the infra guards' output matching stay byte-compatible.
#   --fixture  .ri file to open, repo-relative.
#   --driver   node driver .mjs to run once the debug server is ready, repo-relative.
#
# Exits the calling shell — this is a one-shot entry point, not a composable
# helper.  A runner's last statement is its single e2e_smoke_run call.
# ---------------------------------------------------------------------------
e2e_smoke_run() {
    local name="" fixture_rel="" driver_rel=""

    while [ $# -gt 0 ]; do
        case "$1" in
            --name|--fixture|--driver)
                if [ $# -lt 2 ]; then
                    echo "e2e_smoke_run: $1 requires a value" >&2
                    exit 2
                fi
                case "$1" in
                    --name)    name="$2" ;;
                    --fixture) fixture_rel="$2" ;;
                    --driver)  driver_rel="$2" ;;
                esac
                shift 2
                ;;
            *)
                echo "e2e_smoke_run: unknown argument: $1" >&2
                echo "usage: e2e_smoke_run --name <label> --fixture <repo-relative> --driver <repo-relative>" >&2
                exit 2
                ;;
        esac
    done

    if [ -z "$name" ] || [ -z "$fixture_rel" ] || [ -z "$driver_rel" ]; then
        echo "e2e_smoke_run: --name, --fixture and --driver are all required" >&2
        echo "usage: e2e_smoke_run --name <label> --fixture <repo-relative> --driver <repo-relative>" >&2
        exit 2
    fi

    local REPO_ROOT="$E2E_SMOKE_REPO_ROOT"
    cd "$REPO_ROOT"

    # -- 1. Debug port ------------------------------------------------------
    local PORT
    PORT=$(resolve_port)
    export REIFY_DEBUG_PORT="$PORT"
    echo "${name}: using debug port $PORT"

    # -----------------------------------------------------------------------
    # 2. Ensure an X display is available (Tauri webview needs one).
    #    :0 is the host display dispatched agents share.
    # -----------------------------------------------------------------------
    export DISPLAY="${DISPLAY:-:0}"

    # -----------------------------------------------------------------------
    # 2a. Ensure /opt/reify-deps/lib is on LD_LIBRARY_PATH when present.
    #     libopenvdb.so.13+ links against TBB 12.18; on hosts where only
    #     TBB 12.11 is installed at the system path the binary fails with
    #     "undefined symbol" at startup.  This mirrors the .cargo/run-with-occt.sh
    #     logic (which fires for `cargo run` but NOT for direct binary invocations).
    # -----------------------------------------------------------------------
    local REIFY_DEPS_LIB="/opt/reify-deps/lib"
    if [ -d "$REIFY_DEPS_LIB" ] && ls "$REIFY_DEPS_LIB"/libTKernel.so* >/dev/null 2>&1; then
        export LD_LIBRARY_PATH="$REIFY_DEPS_LIB${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    fi

    # -----------------------------------------------------------------------
    # 2b. Disable WebKit GBM/DMABuf renderer on headless or DRI-unavailable hosts.
    #     On systems where the Nvidia driver exposes DRI fds but the mesa EGL
    #     GBM backend cannot create a screen, WebKit crashes with
    #     "Could not create GBM EGL display: EGL_NOT_INITIALIZED".
    #     WEBKIT_DISABLE_DMABUF_RENDERER=1 forces fallback to the GLX/xlib path.
    # -----------------------------------------------------------------------
    export WEBKIT_DISABLE_DMABUF_RENDERER="${WEBKIT_DISABLE_DMABUF_RENDERER:-1}"

    # -----------------------------------------------------------------------
    # 3. Trap for cleanup — SIGTERM the launcher on exit.
    #    GUI_LAUNCHER_PID is deliberately NOT `local`: the trap handler runs in
    #    the shell's context and must see the live value.
    # -----------------------------------------------------------------------
    GUI_LAUNCHER_PID=""
    cleanup() {
        if [ -n "$GUI_LAUNCHER_PID" ]; then
            kill "$GUI_LAUNCHER_PID" 2>/dev/null || true
            wait "$GUI_LAUNCHER_PID" 2>/dev/null || true
            GUI_LAUNCHER_PID=""
        fi
    }
    trap cleanup EXIT
    trap 'cleanup; exit 130' INT
    trap 'cleanup; exit 143' TERM

    # -----------------------------------------------------------------------
    # 4. Synchronous pre-build (unless REIFY_SMOKE_SKIP_PREBUILD=1).
    #    These steps mirror run-gui-dev.sh's own build sequence; they are
    #    idempotent, so the backgrounded launcher re-runs them as fast no-ops.
    #    Paying the cold-build cost HERE moves it OUTSIDE the readiness window.
    # -----------------------------------------------------------------------
    if [ "${REIFY_SMOKE_SKIP_PREBUILD:-0}" != "1" ]; then
        echo "${name}: pre-building sidecar + gui + reify-gui binary…"
        (cd "$REPO_ROOT/gui/sidecar" && npm install --no-audit --no-fund --silent)
        bash "$REPO_ROOT/gui/sidecar/build-sidecar.sh"
        (cd "$REPO_ROOT/gui" && npm install --no-audit --no-fund --silent)
        cargo build -p reify-gui --bin reify-gui --features gui
        echo "${name}: pre-build complete"
    fi

    # -----------------------------------------------------------------------
    # 5. Background the launcher (overridable for testing via REIFY_SMOKE_LAUNCHER).
    #    REIFY_DEBUG=1 enables the MCP debug listener on port $PORT.
    #    The launcher (run-gui-dev.sh) owns reaping of vite+reify-gui via its trap.
    # -----------------------------------------------------------------------
    local LAUNCHER="${REIFY_SMOKE_LAUNCHER:-$REPO_ROOT/scripts/run-gui-dev.sh}"
    local FIXTURE="$REPO_ROOT/$fixture_rel"
    local DRIVER="$REPO_ROOT/$driver_rel"
    echo "${name}: launching GUI with fixture: $FIXTURE"
    REIFY_DEBUG=1 REIFY_DEBUG_PORT="$PORT" \
        bash "$LAUNCHER" "$FIXTURE" &
    GUI_LAUNCHER_PID=$!
    echo "${name}: launcher PID=$GUI_LAUNCHER_PID"

    # -----------------------------------------------------------------------
    # 6. Bash-side readiness + liveness gate.
    #    Poll the debug health endpoint up to REIFY_SMOKE_WAIT_MS (default 180000).
    #    On each iteration also check kill -0 $GUI_LAUNCHER_PID — abort early if
    #    the launcher has died rather than waiting the full budget.
    # -----------------------------------------------------------------------
    local WAIT_MS="${REIFY_SMOKE_WAIT_MS:-180000}"
    # Convert to whole seconds (ceil).
    local WAIT_SEC=$(( (WAIT_MS + 999) / 1000 ))
    local HEALTH_URL="http://127.0.0.1:${PORT}/mcp"
    local HEALTH_BODY='{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"health","arguments":{}}}'

    echo "${name}: waiting up to ${WAIT_SEC}s for debug server on port ${PORT}…"
    local _start=$SECONDS
    local _server_ready=0
    local _launcher_rc _dead_pid
    while [ $(( SECONDS - _start )) -lt "$WAIT_SEC" ]; do
        # Liveness check — abort early if the launcher died.
        if ! kill -0 "$GUI_LAUNCHER_PID" 2>/dev/null; then
            _launcher_rc=0
            _dead_pid="$GUI_LAUNCHER_PID"
            wait "$GUI_LAUNCHER_PID" 2>/dev/null || _launcher_rc=$?
            GUI_LAUNCHER_PID=""
            _e2e_smoke_report_launcher_death "$name" readiness "$_launcher_rc" "$_dead_pid"
            # Treat launcher-exited-before-ready as failure regardless of exit code.
            # A launcher exit-0 without a bound debug server means the driver never
            # ran and readiness was never confirmed — that is a false PASS if we
            # propagate 0.
            exit "$(( _launcher_rc == 0 ? 1 : _launcher_rc ))"
        fi
        # Health poll.
        if curl -fsS -X POST "$HEALTH_URL" \
                -H 'Content-Type: application/json' \
                -d "$HEALTH_BODY" \
                --max-time 2 \
                >/dev/null 2>&1; then
            _server_ready=1
            break
        fi
        sleep 0.5
    done

    # Distinct from the death path on purpose: a readiness TIMEOUT with a live
    # launcher is a different failure and must NOT emit the death marker.
    if [ "$_server_ready" -ne 1 ]; then
        echo "${name}: debug server did not become ready within ${WAIT_SEC}s" >&2
        exit 1
    fi
    echo "${name}: debug server ready ($(( SECONDS - _start ))s elapsed)"

    # -----------------------------------------------------------------------
    # 7. Run the smoke driver.
    #    The driver's own waitForServer passes instantly (server already ready).
    # -----------------------------------------------------------------------
    echo "${name}: running smoke driver…"
    local DRIVER_RC=0
    REIFY_DEBUG_PORT="$PORT" node "$DRIVER" || DRIVER_RC=$?

    # -----------------------------------------------------------------------
    # 8. Reap the GUI launcher (its trap reaps both vite and reify-gui).
    # -----------------------------------------------------------------------
    if [ -n "$GUI_LAUNCHER_PID" ]; then
        echo "${name}: sending SIGTERM to launcher (PID=$GUI_LAUNCHER_PID)"
        kill "$GUI_LAUNCHER_PID" 2>/dev/null || true
        wait "$GUI_LAUNCHER_PID" 2>/dev/null || true
        GUI_LAUNCHER_PID=""
    fi

    exit "$DRIVER_RC"
}
