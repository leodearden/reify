#!/usr/bin/env bash
# Infrastructure tests for the transient-serve wrapper
# scripts/with-jcodemunch-serve.sh (task 6109, δ).
#
# Design: docs/prds/jcodemunch-substrate-restoration.md §4 (δ)
#         docs/prds/jcodemunch-substrate-restoration.capability-manifest.md §2/δ
#
# Validates the wrapper's CONTRACT executably rather than by grepping its prose
# — PRD §2.4 names exactly that vacuous-evidence shape as the disease (`L-SMOKE`
# cited a script that did not exist; `jcodemunch_live.rs` was PASS-shaped whether
# or not the chain worked). Two test-only seams keep the whole lifecycle
# hermetic, needing no `uvx`, no PyPI and no network:
#
#   --dry-run             prints the CONSTRUCTED serve argv, so the invocation
#                         contract (pin / `serve` / transport / `--watcher=false`,
#                         and the `--paths-from` + `index`/`watch` bans) binds to
#                         the real command rather than to a comment.
#   REIFY_JC_SERVE_CMD    a test-only serve override, so spawn, readiness,
#                         transparency and teardown are all driven against a
#                         stdlib-only stub serve this suite writes itself.
#
# Deliberately does NOT execute the real `uvx … serve` path: it needs PyPI and
# takes tens of seconds even warm, neither of which belongs on a merge gate.
# That one end-to-end run is discharged once by the implementer as recorded
# acceptance evidence — the same reasoning the capability manifest's
# `capstone-must-not-become-gate-resident` resolution applies to ε, and the same
# split β's tests/infra/test_jcodemunch_index_reify.sh already makes.
#
# COST: ~101 s wall, almost all of it sleeping rather than burning CPU — the
# readiness poll interval, the two never-ready deadlines and the two leak runs'
# 10 s teardown free-waits. Measured against siblings in the same bucket:
# test_warm_lane_audit.sh 143 s, test_jcodemunch_index_reify.sh 37 s. If this
# needs to come down further, the leak runs are the place to look, and the cost
# there is real coverage: the -KILL escalation fires at the 5 s mark, so a
# shortened window would stop exercising it.
#
# EVERY PORT THIS SUITE BINDS IS A FREE EPHEMERAL PORT IT PICKS ITSELF. This
# file is a `pool` member (tests/infra/run-all-classification.manifest), so
# ~149 siblings run concurrently with it; binding the wrapper's fixed 8901
# default would make it both flaky and host-global. `--port` is therefore
# load-bearing to this guard, not a nicety.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

JC_SERVE="$REPO_ROOT/scripts/with-jcodemunch-serve.sh"

# The wrapper's OWN readiness token, emitted on stderr the moment readiness is
# PROVEN and before the wrapped command runs. Every refusal path below asserts
# its ABSENCE, so a wrapper that refused after already claiming a live serve
# cannot pass. It is the δ analogue of β's `INDEX-OK`, and it lives on stderr
# rather than stdout because stdout belongs entirely to the wrapped command
# (see the trailing-stderr invariant block).
READY_MARKER="JC-SERVE-READY"

# Greppable refusal markers. Named as constants here so a rename in the script
# fails this suite loudly instead of silently loosening every assertion.
M_PORT_BUSY="E_JC_SERVE_PORT_BUSY"
M_NOT_READY="E_JC_SERVE_NOT_READY"
M_SPAWN_FAILED="E_JC_SERVE_SPAWN_FAILED"
M_LEAKED="E_JC_SERVE_LEAKED"

_TMPDIRS=()
cleanup() {
    local d
    for d in ${_TMPDIRS+"${_TMPDIRS[@]}"}; do
        [ -n "$d" ] && rm -rf "$d"
    done
}
trap cleanup EXIT

# The ONE run-private root every fixture dir is nested under. Registered in
# _TMPDIRS here, in the MAIN shell, exactly once.
_RUN_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/jc-with-serve-root-XXXXXX")"
_TMPDIRS+=("$_RUN_ROOT")

# mk_tmpdir — a fixture dir, echoed on stdout.
#
# SUBSHELL-SAFE BY CONSTRUCTION, and that is the whole reason it nests under
# $_RUN_ROOT rather than appending to _TMPDIRS itself. Every call site reads
# `X="$(mk_tmpdir)"`, so this body runs in a command-substitution SUBSHELL, and
# a `_TMPDIRS+=("$d")` performed here would be silently DISCARDED when that
# subshell exits — leaking every fixture dir this suite ever mints. (Measured:
# the sibling suite tests/infra/test_jcodemunch_index_reify.sh has exactly that
# shape and had left 1476 dirs in /tmp on this host; filed separately, since it
# is outside this task's scope.) Anchoring cleanup on the one root means the
# caller's existing `rm -rf` reclaims everything in one shot — the same
# reasoning test_helpers.sh's make_isolated_lane documents.
mk_tmpdir() {
    mktemp -d "$_RUN_ROOT/fixture-XXXXXX"
}

# require_nonempty <label> <value> — refuse to run a substring assertion whose
# needle is empty. An empty needle matches EVERYTHING, so a fixture that failed
# to produce a value would otherwise turn every downstream assertion into a
# guaranteed pass. Used before each `case`-based haystack test below.
require_nonempty() {
    local label="$1" value="$2"
    if [ -z "$value" ]; then
        echo "ERROR: empty $label — a substring assertion on it would pass vacuously" >&2
        return 1
    fi
    return 0
}

# ─────────────────────────────────────────────────────────────────────────────
# Block 1 — the script's shape contract
#
# Cheapest possible pole: if the file is absent, unparseable, or has lost its
# `set -euo pipefail`, every behavioural block below would fail in a way that
# reads as a behaviour regression rather than as "the script is gone".
# ─────────────────────────────────────────────────────────────────────────────
echo "--- Block 1: script shape contract ---"

b1_exists()      { [ -f "$JC_SERVE" ]; }
b1_executable()  { [ -x "$JC_SERVE" ]; }
b1_shebang()     { [ "$(head -n1 "$JC_SERVE")" = "#!/usr/bin/env bash" ]; }
b1_strict_mode() { grep -q '^set -euo pipefail$' "$JC_SERVE"; }
b1_parses()      { bash -n "$JC_SERVE"; }

b1_help_ok() {
    local out
    out="$("$JC_SERVE" --help 2>&1)" || return 1
    require_nonempty "--help output" "$out" || return 1
    case "$out" in *"Usage:"*) return 0 ;; esac
    printf '%s\n' "--help printed no Usage: block:" "$out"
    return 1
}

# The refusal markers must be DOCUMENTED, not merely emitted: an operator who
# greps a marker out of a log needs `--help` to tell them what it means.
b1_help_names_markers() {
    local out m
    out="$("$JC_SERVE" --help 2>&1)" || return 1
    for m in "$M_PORT_BUSY" "$M_NOT_READY" "$M_SPAWN_FAILED" "$M_LEAKED"; do
        case "$out" in
            *"$m"*) ;;
            *) printf '%s\n' "--help does not document the refusal marker $m"; return 1 ;;
        esac
    done
    return 0
}

assert "scripts/with-jcodemunch-serve.sh exists"            b1_exists
assert "the script is executable"                           b1_executable
assert "line 1 is #!/usr/bin/env bash"                      b1_shebang
assert "the script sets -euo pipefail"                      b1_strict_mode
assert "the script parses (bash -n)"                        b1_parses
assert "--help exits 0 and prints a usage block"            b1_help_ok
assert "--help documents all four refusal markers"          b1_help_names_markers

# ─────────────────────────────────────────────────────────────────────────────
# Shared fixtures for every behavioural block below
# ─────────────────────────────────────────────────────────────────────────────

# pick_free_port — a port nothing is listening on RIGHT NOW.
#
# Binds an ephemeral port, reads it back, and releases it. Inherently racy in
# the abstract — another process can claim it in the window — which is exactly
# why the wrapper's readiness probe is an IDENTITY check rather than a liveness
# check (see the script header): a squatter that grabbed the port cannot answer
# `initialize` as jcodemunch-mcp, so it surfaces as E_JC_SERVE_NOT_READY rather
# than as a silent wrong-endpoint pass.
pick_free_port() {
    python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
}

# port_is_free <port> — the suite's own probe, deliberately NOT the wrapper's.
# A shared implementation could agree with the script while both were wrong.
port_is_free() {
    local port="$1"
    ! python3 - "$port" <<'PY'
import socket, sys
s = socket.socket()
s.settimeout(0.5)
try:
    s.connect(("127.0.0.1", int(sys.argv[1])))
except OSError:
    sys.exit(1)
finally:
    s.close()
sys.exit(0)
PY
}

# argv_has_in_order <haystack> <needle>... — every needle appears, in this
# order, in the haystack. Order matters here because argv IS ordered: a
# `--port` that lands before `serve`, or an `env` prefix that lands after the
# binary, is a different command with different behaviour even though every
# individual substring is present.
argv_has_in_order() {
    local hay="$1"; shift
    local rest="$hay" needle
    require_nonempty "argv haystack" "$hay" || return 1
    for needle in "$@"; do
        require_nonempty "argv needle" "$needle" || return 1
        case "$rest" in
            *"$needle"*)
                # Consume up to and including this needle so the NEXT needle is
                # searched only in what follows.
                rest="${rest#*"$needle"}" ;;
            *)
                printf '%s\n' "argv is missing '$needle' (or it appears out of order)" "  argv: $hay"
                return 1 ;;
        esac
    done
    return 0
}

argv_lacks() {
    local hay="$1"; shift
    local needle
    require_nonempty "argv haystack" "$hay" || return 1
    for needle in "$@"; do
        require_nonempty "argv needle" "$needle" || return 1
        case "$hay" in
            *"$needle"*)
                printf '%s\n' "argv unexpectedly contains '$needle'" "  argv: $hay"
                return 1 ;;
        esac
    done
    return 0
}

# ─────────────────────────────────────────────────────────────────────────────
# Block 2 — the --dry-run invocation contract
#
# Binds the constructed serve argv to the REAL command rather than to a comment
# about it. This is the same seam β's suite uses, and it is what makes the pin,
# the transport, the identity lever and the two BANS falsifiable offline.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 2: --dry-run invocation contract ---"

# The dry-run argv, with the script's own `with-jcodemunch-serve: ` prefix left
# intact — the assertions below are substring tests, and stripping the prefix
# would need a second parser that could drift from the script's output shape.
dry_argv() { "$JC_SERVE" --dry-run "$@" 2>&1; }

DRY_DEFAULT="$(dry_argv || true)"
DRY_PORTED="$(dry_argv --port 8917 || true)"

b2_pin_and_shape() {
    argv_has_in_order "$DRY_DEFAULT" \
        "env" "JCODEMUNCH_GIT_ROOT_IDENTITY=0" \
        "--python" \
        "--from" "jcodemunch-mcp==1.108.54" \
        "jcodemunch-mcp" \
        "serve" \
        "--transport" "streamable-http" \
        "--host" "127.0.0.1" \
        "--port" "8901" \
        "--watcher=false"
}

# THE TWO BANS. `--paths-from` short-circuits discovery and then DELETEs every
# previously-indexed file absent from the list (index_folder.py:1505-1511,
# sqlite_store.py:1698) — β bans it for that reason and δ must never resurrect
# it. `watch`/`index` are β's territory: δ owns serve LIFECYCLE only and must
# never index anything, or two producers would be writing the same index with
# no coordination.
b2_bans() {
    argv_lacks "$DRY_DEFAULT" "--paths-from" " watch " " index " "--once" "--no-ai-summaries"
}

# `--port` must MOVE the port, not merely be accepted. Both halves asserted:
# 8917 present and the 8901 default gone — a script that appended rather than
# replaced would pass the first half alone.
b2_port_moves() {
    argv_has_in_order "$DRY_PORTED" "--port" "8917" || return 1
    argv_lacks "$DRY_PORTED" "8901"
}

b2_dry_run_exits_zero() { "$JC_SERVE" --dry-run >/dev/null 2>&1; }

# --dry-run must SPAWN NOTHING. Checked on the port it would have used, so a
# script that printed the argv and then also ran it is caught.
b2_dry_run_spawns_nothing() {
    local port
    port="$(pick_free_port)" || return 1
    require_nonempty "picked port" "$port" || return 1
    "$JC_SERVE" --dry-run --port "$port" >/dev/null 2>&1 || return 1
    port_is_free "$port"
}

# ...and must NOT run the wrapped command. Witness-file shaped rather than
# output-shaped: a wrapped command whose output was merely swallowed would still
# have RUN, and running an arbitrary command under a flag documented as
# "prints, does not run" is the defect.
b2_dry_run_skips_wrapped_command() {
    local d witness
    d="$(mk_tmpdir)" || return 1
    witness="$d/wrapped-ran"
    "$JC_SERVE" --dry-run touch "$witness" >/dev/null 2>&1 || return 1
    if [ -e "$witness" ]; then
        echo "--dry-run RAN the wrapped command (witness $witness exists)"
        return 1
    fi
    return 0
}

assert "the dry-run argv carries the identity lever, pin, transport and port in order" b2_pin_and_shape
assert "the dry-run argv contains neither --paths-from nor the watch/index subcommands" b2_bans
assert "--port 8917 moves the port in the argv (and drops the 8901 default)" b2_port_moves
assert "--dry-run exits 0" b2_dry_run_exits_zero
assert "--dry-run spawns nothing (its port is still free afterwards)" b2_dry_run_spawns_nothing
assert "--dry-run does not run the wrapped command" b2_dry_run_skips_wrapped_command

# ─────────────────────────────────────────────────────────────────────────────
# Block 3 — preflight refusals
#
# All three refusals must land BEFORE any spawn, so a refusal costs no `uvx`
# resolve and no process to reap. Every fixture here is hermetic: this suite
# binds its own ephemeral ports and drives its own stub binaries. There is
# deliberately NO `command -v uvx || exit 0` anywhere in this file — a whole-
# file skip on a host without uv would bank a vacuous green, which is precisely
# PRD §2.4's disease.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 3: preflight refusals ---"

# Background pids this suite owns, reaped by cleanup(). Kept in a file-free
# array because every append happens in the MAIN shell (never in a subshell).
_BGPIDS=()
_reap_bgpids() {
    local pid
    for pid in ${_BGPIDS+"${_BGPIDS[@]}"}; do
        [ -n "$pid" ] || continue
        kill -TERM "$pid" 2>/dev/null || true
    done
}
# Compose with the existing EXIT trap rather than installing a second one —
# bash EXIT traps do not stack, so a `trap … EXIT` here would silently clobber
# the _TMPDIRS cleanup installed above.
cleanup() {
    _reap_bgpids
    local d
    for d in ${_TMPDIRS+"${_TMPDIRS[@]}"}; do
        [ -n "$d" ] && rm -rf "$d"
    done
}

# start_squatter <dir> — bind and HOLD an ephemeral port in a background
# process. This is the foreign listener the wrapper must refuse to adopt and
# must never kill.
#
# MAIN-SHELL ONLY, and it reports through the globals SQ_PORT/SQ_PID rather than
# on stdout, precisely so callers cannot reach for `$(start_squatter …)` or
# `< <(start_squatter …)`. Either form runs the body in a SUBSHELL, where the
# `_BGPIDS+=` registration below is discarded on exit — leaking a listening
# process out of a `pool` member for the squatter's whole lifetime. The same
# subshell hazard test_helpers.sh's make_isolated_lane documents.
SQ_PORT=""
SQ_PID=""
start_squatter() {
    local dir="$1"
    local portfile="$dir/squatter.port"
    local i
    SQ_PORT=""
    SQ_PID=""
    python3 - "$portfile" <<'PY' &
import socket, sys, time
s = socket.socket()
s.bind(("127.0.0.1", 0))
s.listen(8)
with open(sys.argv[1], "w") as fh:
    fh.write(str(s.getsockname()[1]))
time.sleep(600)
PY
    SQ_PID=$!
    _BGPIDS+=("$SQ_PID")
    for i in $(seq 1 100); do
        [ -s "$portfile" ] && break
        sleep 0.05
    done
    SQ_PORT="$(cat "$portfile" 2>/dev/null || true)"
    [ -n "$SQ_PORT" ] || { echo "start_squatter: the squatter never reported a port" >&2; return 1; }
    return 0
}

# mk_min_path <dir> [hidden-tool] — a PATH holding symlinks to exactly the
# wrapper's external dependency surface, minus <hidden-tool>.
#
# WHY A CURATED DIR RATHER THAN A SHADOWING PREFIX: you cannot hide a binary by
# putting a directory FIRST on PATH — `command -v jq` still resolves to the one
# further down. Hiding requires a PATH from which the tool is genuinely absent,
# so this mints one. The whitelist doubles as an executable statement of the
# wrapper's dependency surface: a wrapper that grows a new external dependency
# without declaring it here fails this block loudly instead of silently
# depending on the ambient PATH.
_MIN_PATH_TOOLS=(env bash sh cat cut grep sed head tail tr wc sort mktemp rm
                 mkdir touch sleep timeout setsid curl jq python3 ss ps
                 awk dirname basename readlink date kill)
mk_min_path() {
    local dir="$1" hidden="${2:-}" tool real
    mkdir -p "$dir" || return 1
    for tool in "${_MIN_PATH_TOOLS[@]}"; do
        [ "$tool" = "$hidden" ] && continue
        real="$(command -v "$tool" 2>/dev/null)" || continue
        ln -sf "$real" "$dir/$tool" 2>/dev/null || true
    done
    # Anti-vacuity: if the tool we were asked to hide is not on the real PATH
    # either, the whole fixture proves nothing.
    if [ -n "$hidden" ] && ! command -v "$hidden" >/dev/null 2>&1; then
        echo "mk_min_path: '$hidden' is not on the real PATH, so hiding it proves nothing" >&2
        return 1
    fi
    return 0
}

# -- 3(a) the port is already occupied ---------------------------------------
#
# REFUSE, never adopt and never kill. An adopted serve carries an unknown pin
# and an unknown identity lever — it may answer for `leodearden/reify` instead
# of `local/reify-4ae45bbd`, the exact wrong-identity class this PRD exists to
# eliminate — and tearing down a process this script did not spawn is out of
# scope.
B3A_DIR="$(mk_tmpdir)"
B3A_PORT=""
B3A_PID=""
B3A_OUT=""
B3A_RC=0
B3A_WITNESS="$B3A_DIR/wrapped-ran"
if [ -n "$B3A_DIR" ]; then
    start_squatter "$B3A_DIR" || true
    B3A_PORT="$SQ_PORT"
    B3A_PID="$SQ_PID"
    if [ -n "$B3A_PORT" ]; then
        B3A_OUT="$("$JC_SERVE" --port "$B3A_PORT" touch "$B3A_WITNESS" 2>&1)" || B3A_RC=$?
    fi
fi

b3a_fixture_live()  { [ -n "$B3A_PORT" ] && [ -n "$B3A_PID" ] && ! port_is_free "$B3A_PORT"; }
b3a_refuses()       {
    require_nonempty "port-busy output" "$B3A_OUT" || return 1
    [ "$B3A_RC" -ne 0 ] || { echo "wrapper exited 0 over an occupied port"; return 1; }
    case "$B3A_OUT" in *"$M_PORT_BUSY"*) return 0 ;; esac
    printf '%s\n' "no $M_PORT_BUSY in:" "$B3A_OUT"; return 1
}
b3a_no_ready_claim() {
    require_nonempty "port-busy output" "$B3A_OUT" || return 1
    case "$B3A_OUT" in *"$READY_MARKER"*) printf '%s\n' "wrapper claimed $READY_MARKER before refusing:" "$B3A_OUT"; return 1 ;; esac
    return 0
}
b3a_skips_wrapped()  { [ ! -e "$B3A_WITNESS" ]; }
# THE LOAD-BEARING HALF: the foreign listener survives untouched.
b3a_squatter_alive() {
    kill -0 "$B3A_PID" 2>/dev/null || { echo "the wrapper KILLED a serve it did not spawn (pid $B3A_PID is gone)"; return 1; }
    port_is_free "$B3A_PORT" && { echo "the foreign listener stopped accepting — the wrapper tore down a port it did not own"; return 1; }
    return 0
}
# Negative control: the marker is caused by the OCCUPIED port, not emitted
# unconditionally.
b3a_negative_control() {
    local port out
    port="$(pick_free_port)" || return 1
    out="$("$JC_SERVE" --port "$port" true 2>&1 || true)"
    case "$out" in *"$M_PORT_BUSY"*) printf '%s\n' "$M_PORT_BUSY fired on a FREE port:" "$out"; return 1 ;; esac
    return 0
}

assert "the port-busy fixture really holds a port (anti-vacuity)"          b3a_fixture_live
assert "an occupied port refuses $M_PORT_BUSY and exits non-zero"          b3a_refuses
assert "the port-busy refusal never claims $READY_MARKER"                  b3a_no_ready_claim
assert "the port-busy refusal does not run the wrapped command"            b3a_skips_wrapped
assert "the wrapper neither kills nor adopts the foreign listener"         b3a_squatter_alive
assert "on a FREE port the same invocation does not emit $M_PORT_BUSY"     b3a_negative_control

# -- 3(b) the serve binary is missing ----------------------------------------
#
# Must refuse PROMPTLY and name the fix, rather than crashing or hanging until
# the 180 s readiness deadline — a missing binary is not a slow start.
B3B_OUT=""
B3B_RC=0
B3B_PORT="$(pick_free_port || true)"
if [ -n "$B3B_PORT" ]; then
    B3B_OUT="$(REIFY_JC_SERVE_CMD="/nonexistent/dir/no-such-serve" \
        "$JC_SERVE" --port "$B3B_PORT" true 2>&1)" || B3B_RC=$?
fi

b3b_refuses()       { require_nonempty "missing-serve output" "$B3B_OUT" || return 1; [ "$B3B_RC" -ne 0 ]; }
b3b_names_missing() {
    require_nonempty "missing-serve output" "$B3B_OUT" || return 1
    case "$B3B_OUT" in *"/nonexistent/dir/no-such-serve"*) return 0 ;; esac
    printf '%s\n' "the refusal does not name the missing command:" "$B3B_OUT"; return 1
}
b3b_names_the_fix() {
    require_nonempty "missing-serve output" "$B3B_OUT" || return 1
    case "$B3B_OUT" in *"/home/leo/.local/bin/uvx"*) ;; *) printf '%s\n' "the refusal does not name where uvx lives on this host:" "$B3B_OUT"; return 1 ;; esac
    case "$B3B_OUT" in *"--dry-run"*) ;; *) printf '%s\n' "the refusal does not offer --dry-run:" "$B3B_OUT"; return 1 ;; esac
    return 0
}
b3b_no_ready_claim() {
    require_nonempty "missing-serve output" "$B3B_OUT" || return 1
    case "$B3B_OUT" in *"$READY_MARKER"*) return 1 ;; esac
    return 0
}
# The refusal must be preflight, not a burnt readiness deadline: nothing may be
# left listening on the port it was given.
b3b_port_untouched() { [ -n "$B3B_PORT" ] && port_is_free "$B3B_PORT"; }

assert "a missing serve binary refuses and exits non-zero"                 b3b_refuses
assert "the missing-serve refusal names the command it could not find"     b3b_names_missing
assert "the missing-serve refusal names uvx's location and --dry-run"      b3b_names_the_fix
assert "the missing-serve refusal never claims $READY_MARKER"              b3b_no_ready_claim
assert "the missing-serve refusal leaves nothing listening"                b3b_port_untouched

# -- 3(c) curl / jq are missing ----------------------------------------------
#
# The readiness probe cannot be built out of either one alone, so both are
# preflight requirements rather than lazily-discovered failures partway into a
# poll loop.
B3C_DIR="$(mk_tmpdir)"
b3c_missing_tool_named() {
    local hidden="$1" dir out port fake
    [ -n "$B3C_DIR" ] || return 1
    dir="$B3C_DIR/no-$hidden"
    mk_min_path "$dir" "$hidden" || return 1
    # A real, resolvable serve command, so the refusal under test is about the
    # hidden tool and not about a missing uvx.
    fake="$B3C_DIR/fake-serve"
    printf '#!/bin/sh\nexit 0\n' > "$fake" && chmod +x "$fake"
    port="$(pick_free_port)" || return 1
    out="$(PATH="$dir" REIFY_JC_SERVE_CMD="$fake" "$JC_SERVE" --port "$port" "$fake" 2>&1 || true)"
    require_nonempty "missing-$hidden output" "$out" || return 1
    case "$out" in
        *"'$hidden'"*|*" $hidden "*) return 0 ;;
    esac
    printf '%s\n' "the refusal does not name the missing '$hidden':" "$out"
    return 1
}
b3c_negative_control() {
    local dir out port fake
    [ -n "$B3C_DIR" ] || return 1
    dir="$B3C_DIR/complete"
    mk_min_path "$dir" || return 1
    fake="$B3C_DIR/fake-serve"
    printf '#!/bin/sh\nexit 0\n' > "$fake" && chmod +x "$fake"
    port="$(pick_free_port)" || return 1
    out="$(PATH="$dir" REIFY_JC_SERVE_CMD="$fake" "$JC_SERVE" --port "$port" "$fake" 2>&1 || true)"
    case "$out" in
        *"is not on PATH"*) printf '%s\n' "a COMPLETE PATH still produced a missing-tool refusal:" "$out"; return 1 ;;
    esac
    return 0
}

assert "a PATH without curl refuses and names curl"                        b3c_missing_tool_named curl
assert "a PATH without jq refuses and names jq"                            b3c_missing_tool_named jq
assert "a complete PATH produces no missing-tool refusal (control)"        b3c_negative_control

# ─────────────────────────────────────────────────────────────────────────────
# The hermetic stub serve
#
# A stdlib-only responder driven through the wrapper's REIFY_JC_SERVE_CMD seam,
# so spawn, readiness, transparency and teardown are all exercised with no uvx,
# no PyPI and no network. Modelled on β's mk_stub_indexer.
#
# The wrapper replaces only the `uvx … jcodemunch-mcp` prefix, so the stub is
# handed the REST of the constructed argv verbatim:
#     serve --transport streamable-http --host 127.0.0.1 --port <N> --watcher=false
# It parses --port out of that argv rather than being told the port some other
# way, which is what makes "the constructed argv actually reaches the child" an
# observation rather than an assumption.
#
# EVERY mode records, into $REIFY_JC_SERVE_WITNESS: the identity lever AS THE
# CHILD RECEIVED IT, its own pid/pgid, the port it parsed, its whole argv, and
# (once serving) every request path it is asked for.
# ─────────────────────────────────────────────────────────────────────────────

# mk_stub_serve <path> <healthy|foreign|slow|die|grandchild>
#
# Writes TWO files: <path> (the bash entry point) and <path>.py (the responder).
# The responder is a separate file rather than a heredoc so the entry point can
# `exec` it — with `exec`, the process-group LEADER is the server itself, which
# is what makes the `grandchild` mode structurally different rather than merely
# differently named.
mk_stub_serve() {
    local path="$1" mode="$2"

    cat > "$path.py" <<'STUBPY'
"""Stub jcodemunch serve: stdlib only. argv = <port> <serverInfo.name>."""
import http.server
import json
import os
import sys

PORT = int(sys.argv[1])
NAME = sys.argv[2]
WITNESS = os.environ["REIFY_JC_SERVE_WITNESS"]


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):  # keep the stub silent on stderr
        pass

    def do_POST(self):
        # Recorded BEFORE the response, so a probe that never got an answer is
        # still visible in the witness. The exact path matters: a real serve
        # 307-redirects `/mcp/` and the redirect drops mcp-session-id.
        with open(WITNESS, "a") as fh:
            fh.write("request-path=[%s]\n" % self.path)
        length = int(self.headers.get("Content-Length") or 0)
        if length:
            self.rfile.read(length)
        body = json.dumps(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "serverInfo": {"name": NAME, "version": "1.108.54"},
                },
            }
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Mcp-Session-Id", "stub-session-0001")
        self.end_headers()
        self.wfile.write(body)

    do_GET = do_POST


http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
STUBPY

    cat > "$path" <<'STUB'
#!/usr/bin/env bash
# Test stub serve — tests/infra/test_with_jcodemunch_serve.sh.
set -uo pipefail

# HARD SAFETY PROPERTY (β's shape): refuse to run unredirected. Without a
# witness this stub has nowhere to record what it observed, and every
# witness-based assertion in the suite would pass vacuously.
: "${REIFY_JC_SERVE_WITNESS:?stub serve refuses to run unredirected}"

STUB_PORT=""
_prev=""
for _a in "$@"; do
    [ "$_prev" = "--port" ] && STUB_PORT="$_a"
    case "$_a" in --port=*) STUB_PORT="${_a#*=}" ;; esac
    _prev="$_a"
done

{
    printf 'identity-env=[%s]\n' "${JCODEMUNCH_GIT_ROOT_IDENTITY-<unset>}"
    printf 'stub-pid=[%s]\n' "$$"
    printf 'stub-pgid=[%s]\n' "$(ps -o pgid= -p $$ | tr -d ' ')"
    printf 'stub-port=[%s]\n' "$STUB_PORT"
    printf 'stub-argv=[%s]\n' "$*"
} >> "$REIFY_JC_SERVE_WITNESS"
STUB

    case "$mode" in
        healthy)
            # exec, so the process-group LEADER is the listener itself.
            echo 'exec python3 "$0.py" "$STUB_PORT" jcodemunch-mcp' >> "$path" ;;
        foreign)
            # Well-formed JSON-RPC, 200, a session header — everything a
            # LIVENESS check would accept — under somebody else's name.
            echo 'exec python3 "$0.py" "$STUB_PORT" not-jcodemunch' >> "$path" ;;
        slow)
            # Refuses connections outright for a few polls, then becomes
            # healthy. A cold `uvx` resolve looks exactly like this.
            echo 'sleep "${REIFY_JC_STUB_SLOW_SECS:-4}"' >> "$path"
            echo 'exec python3 "$0.py" "$STUB_PORT" jcodemunch-mcp' >> "$path" ;;
        die)
            echo 'printf "stub-die=[3]\n" >> "$REIFY_JC_SERVE_WITNESS"' >> "$path"
            echo 'exit 3' >> "$path" ;;
        grandchild)
            # The leak shape `uvx` produces: the thing the wrapper spawns is NOT
            # the thing holding the port. A teardown that signalled only the
            # direct child would leave this python listening forever.
            echo 'python3 "$0.py" "$STUB_PORT" jcodemunch-mcp &' >> "$path"
            echo 'printf "stub-grandchild-pid=[%s]\n" "$!" >> "$REIFY_JC_SERVE_WITNESS"' >> "$path"
            echo 'exec sleep 600' >> "$path" ;;
        escapee)
            # A LEAK, deliberately constructed. The listener is put in a brand
            # new SESSION, so it is outside the process group the wrapper
            # signals and survives both -TERM and -KILL. You cannot build this
            # fixture out of a signal handler — SIGKILL is uncatchable — so the
            # only honest leak is a process that escaped the group, which is
            # also the realistic one. Records its own pgid under a distinct key
            # so this suite's cleanup can still reap it.
            echo 'setsid bash -c '"'"'printf "stub-escapee-pgid=[%s]\n" "$$" >> "$REIFY_JC_SERVE_WITNESS"; exec python3 "$1" "$2" "$3"'"'"' _ "$0.py" "$STUB_PORT" jcodemunch-mcp &' >> "$path"
            echo 'exec sleep 600' >> "$path" ;;
        *)
            echo "mk_stub_serve: unknown mode '$mode'" >&2; return 1 ;;
    esac
    chmod +x "$path"
    return 0
}

# Witness files this suite has produced, swept by cleanup(). A stub serve is
# torn down by the WRAPPER on every path the wrapper implements — but the TDD
# commits between here and the teardown step do not implement it yet, and a
# `pool` member must not leak a listener either way. The sweep is precise
# rather than port-shaped: it reaps only process groups this suite's own stubs
# recorded, never "whatever is listening on that port".
_WITNESSES=()
_reap_stub_groups() {
    local w pgid
    for w in ${_WITNESSES+"${_WITNESSES[@]}"}; do
        [ -f "$w" ] || continue
        while IFS= read -r pgid; do
            [ -n "$pgid" ] || continue
            kill -TERM -- "-$pgid" 2>/dev/null || true
            kill -KILL -- "-$pgid" 2>/dev/null || true
        done < <(sed -n -e 's/^stub-pgid=\[\([0-9]\{1,\}\)\]$/\1/p' \
                        -e 's/^stub-escapee-pgid=\[\([0-9]\{1,\}\)\]$/\1/p' "$w" | sort -u)
    done
}
cleanup() {
    _reap_stub_groups
    _reap_bgpids
    local d
    for d in ${_TMPDIRS+"${_TMPDIRS[@]}"}; do
        [ -n "$d" ] && rm -rf "$d"
    done
}

# rw_run <mode> [wrapped command...] — one wrapper invocation against a stub.
#
# MAIN-SHELL ONLY (it registers into _WITNESSES; see start_squatter's note).
# Sets: RW_DIR RW_PORT RW_WITNESS RW_OUT RW_ERR RW_RC RW_ELAPSED.
# Reads:  RW_SERVE_PREFIX  extra words prepended to the stub inside
#                          REIFY_JC_SERVE_CMD — the identity negative control.
#         RW_TIMEOUT       the wrapper's readiness deadline, in seconds.
#         RW_EXTRA_ENV     extra NAME=VALUE settings placed in the WRAPPER's own
#                          environment (not the serve's), for the inheritance
#                          negative controls.
RW_SERVE_PREFIX=""
RW_TIMEOUT=20
RW_EXTRA_ENV=()
rw_run() {
    local mode="$1"; shift
    local stub t0
    RW_DIR="$(mk_tmpdir)" || return 1
    RW_WITNESS="$RW_DIR/witness"
    : > "$RW_WITNESS"
    _WITNESSES+=("$RW_WITNESS")
    RW_PORT="$(pick_free_port)" || return 1
    stub="$RW_DIR/stub-serve"
    mk_stub_serve "$stub" "$mode" || return 1
    RW_OUT="$RW_DIR/stdout"
    RW_ERR="$RW_DIR/stderr"
    RW_RC=0
    t0=$SECONDS
    env ${RW_EXTRA_ENV+"${RW_EXTRA_ENV[@]}"} \
        REIFY_JC_SERVE_WITNESS="$RW_WITNESS" \
        REIFY_JC_SERVE_READY_TIMEOUT="$RW_TIMEOUT" \
        REIFY_JC_SERVE_CMD="${RW_SERVE_PREFIX:+$RW_SERVE_PREFIX }$stub" \
        "$JC_SERVE" --port "$RW_PORT" "$@" >"$RW_OUT" 2>"$RW_ERR" || RW_RC=$?
    RW_ELAPSED=$((SECONDS - t0))
    return 0
}

# has_line <file> <needle> / lacks_line <file> <needle>
has_line() {
    local f="$1" needle="$2"
    require_nonempty "needle" "$needle" || return 1
    [ -f "$f" ] || { echo "no such file: $f"; return 1; }
    grep -qF -- "$needle" "$f" && return 0
    printf '%s\n' "'$needle' not found in $f:" "$(cat "$f")"
    return 1
}
lacks_line() {
    local f="$1" needle="$2"
    require_nonempty "needle" "$needle" || return 1
    [ -f "$f" ] || return 0
    grep -qF -- "$needle" "$f" || return 0
    printf '%s\n' "'$needle' unexpectedly present in $f:" "$(cat "$f")"
    return 1
}

# ─────────────────────────────────────────────────────────────────────────────
# Block 4 — spawn and readiness
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 4: spawn and readiness ---"

rw_run healthy true
B4H_ERR="$RW_ERR"; B4H_WITNESS="$RW_WITNESS"

b4_healthy_ready()      { has_line "$B4H_ERR" "$READY_MARKER"; }
b4_child_saw_argv()     { has_line "$B4H_WITNESS" "stub-argv=[serve --transport streamable-http --host 127.0.0.1 --port"; }
b4_child_saw_watcher()  { has_line "$B4H_WITNESS" "--watcher=false]"; }

# THE 307 GOTCHA, bound to an OBSERVATION rather than to a grep of the source:
# what path did the server actually receive?
b4_probe_path()         { has_line "$B4H_WITNESS" "request-path=[/mcp]"; }
b4_probe_no_slash()     { lacks_line "$B4H_WITNESS" "request-path=[/mcp/]"; }

# THE IDENTITY LEVER REACHES THE CHILD (β Test 12's behavioural half): the value
# is read out of the CHILD's environment, not out of the argv the parent printed.
b4_child_identity()     { has_line "$B4H_WITNESS" "identity-env=[0]"; }

assert "a healthy stub serve reaches readiness ($READY_MARKER on stderr)"  b4_healthy_ready
assert "the constructed argv reaches the child verbatim"                   b4_child_saw_argv
assert "the child is told --watcher=false"                                 b4_child_saw_watcher
assert "the readiness probe requests /mcp"                                 b4_probe_path
assert "the readiness probe never requests /mcp/ (307 drops the session id)" b4_probe_no_slash
assert "the spawned child really runs with JCODEMUNCH_GIT_ROOT_IDENTITY=0"  b4_child_identity

# β Test 14's NEGATIVE CONTROL: drive the lever to 1 through the same seam and
# prove the assertion above can fail. Without this, `identity-env=[0]` could be
# a constant the stub prints regardless.
RW_SERVE_PREFIX="env JCODEMUNCH_GIT_ROOT_IDENTITY=1"
rw_run healthy true
B4N_WITNESS="$RW_WITNESS"
RW_SERVE_PREFIX=""

b4_identity_control()   { has_line "$B4N_WITNESS" "identity-env=[1]"; }
assert "negative control: forcing the lever to 1 is observed as identity-env=[1]" b4_identity_control

# -- slow: waited out, not failed ---------------------------------------------
rw_run slow true
B4S_ERR="$RW_ERR"

b4_slow_ready()    { has_line "$B4S_ERR" "$READY_MARKER"; }
b4_slow_no_refuse(){ lacks_line "$B4S_ERR" "$M_NOT_READY"; }
assert "a slow-starting serve is waited out, not failed"                   b4_slow_ready
assert "a slow-starting serve produces no $M_NOT_READY"                    b4_slow_no_refuse

# -- foreign: identity, not liveness ------------------------------------------
#
# The never-ready cases are the only ones that burn the WHOLE readiness
# deadline, so theirs is trimmed from the suite default. 10 s is ~9 polls,
# ample headroom for a stdlib http.server to bind even with ~150 pool members
# running concurrently — and if it somehow is not, `b4_foreign_answered` fails
# loudly rather than letting an unreachable port masquerade as the identity
# finding.
RW_TIMEOUT=10
rw_run foreign true
B4F_ERR="$RW_ERR"; B4F_WITNESS="$RW_WITNESS"; B4F_RC="$RW_RC"

# ANTI-VACUITY FIRST: the foreign stub really did answer. Without this, an
# E_JC_SERVE_NOT_READY caused by an unreachable port would masquerade as the
# identity finding.
b4_foreign_answered() { has_line "$B4F_WITNESS" "request-path=[/mcp]"; }
b4_foreign_refuses()  { has_line "$B4F_ERR" "$M_NOT_READY"; }
b4_foreign_not_ready(){ lacks_line "$B4F_ERR" "$READY_MARKER"; }
b4_foreign_nonzero()  { [ "$B4F_RC" -ne 0 ]; }
RW_TIMEOUT=20

assert "the foreign stub really answered the probe (anti-vacuity)"         b4_foreign_answered
assert "a serve answering under another name refuses $M_NOT_READY"         b4_foreign_refuses
assert "the foreign refusal never claims $READY_MARKER"                    b4_foreign_not_ready
assert "the foreign refusal exits non-zero"                                b4_foreign_nonzero

# -- die: a spawn failure, reported promptly and as itself --------------------
RW_TIMEOUT=120
rw_run die true
B4D_ERR="$RW_ERR"; B4D_RC="$RW_RC"; B4D_ELAPSED="$RW_ELAPSED"
RW_TIMEOUT=20

b4_die_marker()  { has_line "$B4D_ERR" "$M_SPAWN_FAILED"; }
# A specific needle, not a bare "3": the port, the pin and the deadline all
# carry digits, so a loose match would pass on prose that names no status.
b4_die_status()  { has_line "$B4D_ERR" "status 3"; }
b4_die_nonzero() { [ "$B4D_RC" -ne 0 ]; }
# The deadline is 120 s for this run precisely so "prompt" is falsifiable: a
# wrapper that blamed a timeout for a spawn failure would take at least that
# long. 30 s is generous headroom over the ~1 s the correct behaviour costs.
b4_die_prompt()  {
    [ "$B4D_ELAPSED" -lt 30 ] && return 0
    echo "the die-stub run took ${B4D_ELAPSED}s against a 120s readiness deadline — the spawn failure was reported as a timeout"
    return 1
}
b4_die_not_ready() { lacks_line "$B4D_ERR" "$READY_MARKER"; }

assert "a serve child that dies refuses $M_SPAWN_FAILED"                   b4_die_marker
assert "the spawn-failure refusal names the child's exit status"           b4_die_status
assert "the spawn-failure refusal exits non-zero"                          b4_die_nonzero
assert "the spawn failure is reported promptly, not at the deadline"       b4_die_prompt
assert "the spawn-failure refusal never claims $READY_MARKER"              b4_die_not_ready

# ─────────────────────────────────────────────────────────────────────────────
# Block 5 — transparency of the wrapped command
#
# The wrapper's whole value is that a caller can prefix it onto an existing
# command and observe NO difference except that a serve now exists. Every way
# that can silently stop being true is pinned here: status, stdout bytes, argv,
# and the one variable the wrapped command actually needs.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 5: transparency of the wrapped command ---"

# mk_echo_args <path> — a wrapped command that reports its own argv and the
# JCODEMUNCH_URL it was handed, one record per line, on STDOUT.
mk_echo_args() {
    cat > "$1" <<'ECHOARGS'
#!/usr/bin/env bash
set -u
printf 'url=[%s]\n' "${JCODEMUNCH_URL-<unset>}"
i=0
for a in "$@"; do
    i=$((i + 1))
    printf 'arg%d=[%s]\n' "$i" "$a"
done
printf 'argc=[%d]\n' "$#"
ECHOARGS
    chmod +x "$1"
}

# -- 5(a) exit status is propagated verbatim ----------------------------------
#
# Three values, not one: 0 proves the happy path, 3 proves a non-zero status is
# not flattened to 1, and 42 proves it is not being confused with a signal or a
# marker of the wrapper's own. 127 for a missing command is the shell's own
# convention and must survive too.
b5_status_is() {
    local want="$1"; shift
    rw_run healthy "$@" || return 1
    [ "$RW_RC" = "$want" ] && return 0
    printf '%s\n' "wrapped command exiting $want produced wrapper status $RW_RC" "stderr: $(cat "$RW_ERR")"
    return 1
}

assert "a wrapped command exiting 0 gives wrapper status 0"     b5_status_is 0  bash -c 'exit 0'
assert "a wrapped command exiting 3 gives wrapper status 3"     b5_status_is 3  bash -c 'exit 3'
assert "a wrapped command exiting 42 gives wrapper status 42"   b5_status_is 42 bash -c 'exit 42'
assert "a wrapped command that does not exist gives 127"        b5_status_is 127 /nonexistent/dir/no-such-command

# -- 5(b) stdout purity -------------------------------------------------------
#
# BYTE EQUALITY, deliberately: any wrapper chatter that leaked onto stdout — a
# banner, a readiness line, a teardown note — fails this. That is the property
# that keeps the wrapper generic rather than reify-audit-specific, and it is
# the half of the stderr-discipline rule that a `grep -v` could not express.
b5_stdout_pure() {
    local want
    rw_run healthy bash -c 'printf "alpha\nbeta\n"' || return 1
    want="$(printf 'alpha\nbeta\n')"
    [ "$(cat "$RW_OUT")" = "$want" ] && return 0
    printf '%s\n' "wrapper stdout was not byte-identical to the wrapped command's:" \
        "--- got ---" "$(cat "$RW_OUT")" "--- want ---" "$want"
    return 1
}
# ...and the empty case, where a stray banner is most likely to hide.
b5_stdout_empty_stays_empty() {
    rw_run healthy true || return 1
    [ ! -s "$RW_OUT" ] && return 0
    printf '%s\n' "a silent wrapped command still produced wrapper stdout:" "$(cat "$RW_OUT")"
    return 1
}

assert "wrapper stdout is byte-identical to the wrapped command's"  b5_stdout_pure
assert "a silent wrapped command leaves wrapper stdout empty"       b5_stdout_empty_stays_empty

# -- 5(c) argv fidelity -------------------------------------------------------
#
# One argument containing a SPACE and one containing a GLOB metacharacter: the
# two ways a naive `$*`/unquoted-`$@` pass-through corrupts an argv. The count
# is asserted alongside the values, because a split argument still produces the
# right substrings.
B5C_DIR="$(mk_tmpdir)"
b5_argv_fidelity() {
    local echoer="$B5C_DIR/echo-args"
    mk_echo_args "$echoer" || return 1
    rw_run healthy "$echoer" "two words" '*.rs' 'plain' || return 1
    has_line "$RW_OUT" 'arg1=[two words]' || return 1
    has_line "$RW_OUT" 'arg2=[*.rs]' || return 1
    has_line "$RW_OUT" 'arg3=[plain]' || return 1
    has_line "$RW_OUT" 'argc=[3]'
}
assert "the wrapped command's argv survives spaces and glob metacharacters" b5_argv_fidelity

# -- 5(d) JCODEMUNCH_URL ------------------------------------------------------
#
# The one thing the wrapped command actually needs: reify-audit reads
# JCODEMUNCH_URL (reify-audit.rs:213) to learn which endpoint to talk to. It
# must name THE CHOSEN PORT — not the 8901 default — and carry no trailing
# slash.
B5D_DIR="$(mk_tmpdir)"
b5_url_is_the_chosen_port() {
    local echoer="$B5D_DIR/echo-args"
    mk_echo_args "$echoer" || return 1
    rw_run healthy "$echoer" || return 1
    has_line "$RW_OUT" "url=[http://127.0.0.1:$RW_PORT/mcp]" || return 1
    # And not the default, unless the ephemeral port happened to BE the default
    # — in which case the assertion above already carries the whole claim.
    [ "$RW_PORT" = "8901" ] && return 0
    lacks_line "$RW_OUT" "url=[http://127.0.0.1:8901/mcp]"
}
b5_url_has_no_trailing_slash() {
    local echoer="$B5D_DIR/echo-args"
    mk_echo_args "$echoer" || return 1
    rw_run healthy "$echoer" || return 1
    lacks_line "$RW_OUT" "/mcp/]"
}
# NEGATIVE CONTROL: the value is PRODUCED, not inherited. A wrapper that merely
# passed the ambient environment through would hand the wrapped command a
# foreign endpoint and every assertion above would still pass on a host where
# JCODEMUNCH_URL happened to be unset.
b5_url_overrides_ambient() {
    local echoer="$B5D_DIR/echo-args"
    mk_echo_args "$echoer" || return 1
    RW_EXTRA_ENV=(JCODEMUNCH_URL=http://198.51.100.7:1/foreign)
    rw_run healthy "$echoer" || { RW_EXTRA_ENV=(); return 1; }
    RW_EXTRA_ENV=()
    has_line "$RW_OUT" "url=[http://127.0.0.1:$RW_PORT/mcp]" || return 1
    lacks_line "$RW_OUT" "198.51.100.7"
}

assert "the wrapped command is handed JCODEMUNCH_URL for the CHOSEN port"   b5_url_is_the_chosen_port
assert "the exported JCODEMUNCH_URL carries no trailing slash"              b5_url_has_no_trailing_slash
assert "a pre-set foreign JCODEMUNCH_URL is overridden, not inherited"      b5_url_overrides_ambient

# ─────────────────────────────────────────────────────────────────────────────
# Block 6 — teardown on EVERY exit path
#
# A leaked serve keeps holding the port, so the NEXT invocation refuses
# E_JC_SERVE_PORT_BUSY — a leak here is not a tidiness problem, it breaks the
# following run. Five exits are driven, and every one of them is driven against
# the `grandchild` stub (except the never-ready case, which needs a listener
# that answers under the wrong name):
#
#   THE GRANDCHILD IS WHAT MAKES THIS NON-VACUOUS. In that mode the process the
#   wrapper spawned is a `sleep`, and a SEPARATE forked python holds the port.
#   A teardown that signalled only the direct child would leave that python
#   listening — precisely the `uvx`-fronts-python leak α's signal_process_group
#   exists to prevent. Against a single-process stub, group logic and
#   child-only logic are indistinguishable.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 6: teardown on every exit path ---"

# witness_field <witness> <name> — the LAST recorded value of `<name>=[…]`.
witness_field() {
    local w="$1" name="$2"
    [ -f "$w" ] || return 1
    sed -n "s/^${name}=\[\(.*\)\]$/\1/p" "$w" | tail -n1
}

# group_gone <pgid> — is the whole process group gone? `kill -0 -- -<pgid>`
# tests the GROUP, not one pid, which is the property that matters: the leader
# can be reaped while a grandchild in the same group still holds the port.
group_gone() {
    local pgid="$1"
    require_nonempty "pgid" "$pgid" || return 1
    ! kill -0 -- "-$pgid" 2>/dev/null
}

# wait_gone <pgid> <port> <secs> — bounded, to absorb scheduler jitter ONLY.
# The wrapper's own teardown already waits for the port before exiting, so by
# the time control reaches here the answer should already be yes; a generous
# window here would mask a teardown that returns before it has finished.
wait_gone() {
    local pgid="$1" port="$2" secs="$3" i
    for i in $(seq 1 $((secs * 10))); do
        if group_gone "$pgid" && port_is_free "$port"; then
            return 0
        fi
        sleep 0.1
    done
    group_gone "$pgid" || printf '%s\n' "process group $pgid is still alive after the wrapper exited:" "$(ps -eo pid,pgid,cmd | awk -v g="$pgid" '$2==g')"
    port_is_free "$port" || printf '%s\n' "port $port is still accepting after the wrapper exited"
    return 1
}

# -- exits 1-3: the wrapped command returned (0 / non-zero / 127) -------------
b6_teardown_after_exit() {
    local want="$1"; shift
    local pgid
    rw_run grandchild "$@" || return 1
    [ "$RW_RC" = "$want" ] || { printf '%s\n' "expected status $want, got $RW_RC" "$(cat "$RW_ERR")"; return 1; }
    pgid="$(witness_field "$RW_WITNESS" stub-pgid)"
    require_nonempty "recorded stub pgid" "$pgid" || return 1
    wait_gone "$pgid" "$RW_PORT" 2
}

assert "a successful wrapped command still tears the serve group down"      b6_teardown_after_exit 0   true
assert "a failing wrapped command still tears the serve group down"         b6_teardown_after_exit 7   bash -c 'exit 7'
assert "a missing wrapped command (127) still tears the serve group down"   b6_teardown_after_exit 127 /nonexistent/dir/no-such-command

# -- exit 4: readiness was never achieved ------------------------------------
#
# The `foreign` stub, not `die`: a child that never bound anything has nothing
# to leak, so it cannot distinguish a real teardown from no teardown at all.
# A live listener that never becomes ready is where teardown actually matters.
b6_teardown_after_not_ready() {
    local pgid rc
    RW_TIMEOUT=10   # see Block 4's note: only the never-ready cases pay the deadline
    rw_run foreign true || { RW_TIMEOUT=20; return 1; }
    RW_TIMEOUT=20
    [ "$RW_RC" -ne 0 ] || { echo "the never-ready run exited 0"; return 1; }
    has_line "$RW_ERR" "$M_NOT_READY" || return 1
    pgid="$(witness_field "$RW_WITNESS" stub-pgid)"
    require_nonempty "recorded stub pgid" "$pgid" || return 1
    wait_gone "$pgid" "$RW_PORT" 2
}
assert "a never-ready serve is torn down rather than left listening"        b6_teardown_after_not_ready

# -- exits 5-6: the wrapper itself is signalled -------------------------------
#
# rw_run_bg launches the wrapper in the BACKGROUND so the suite can signal it
# mid-run. MAIN-SHELL ONLY, same registration reason as start_squatter.
RWB_PID=""
rw_run_bg() {
    local mode="$1"; shift
    local stub
    RW_DIR="$(mk_tmpdir)" || return 1
    RW_WITNESS="$RW_DIR/witness"
    : > "$RW_WITNESS"
    _WITNESSES+=("$RW_WITNESS")
    RW_PORT="$(pick_free_port)" || return 1
    stub="$RW_DIR/stub-serve"
    mk_stub_serve "$stub" "$mode" || return 1
    RW_OUT="$RW_DIR/stdout"
    RW_ERR="$RW_DIR/stderr"
    # JOB CONTROL IS LOAD-BEARING HERE, and only for the SIGINT case.
    #
    # POSIX: with job control OFF (the default in a script), the shell sets
    # SIGINT and SIGQUIT to IGNORE in a background command — and a signal
    # IGNORED on entry to a shell cannot subsequently be trapped or reset. So a
    # wrapper launched with a bare `&` has an INERT `trap … INT`, and a
    # `kill -INT` at it does nothing at all. MEASURED on this host with a
    # reduced fixture (trap INT -> exit 130, trap TERM -> exit 143, backgrounded,
    # signalled after 0.4s):
    #
    #     set +m  INT   rc=0    trap never ran, the script fell through
    #     set -m  INT   rc=130  trap ran
    #     set +m  TERM  rc=143  trap ran
    #     set -m  TERM  rc=143  trap ran
    #
    # i.e. without `set -m` the SIGINT assertion would be testing the FIXTURE's
    # inherited disposition and would fail no matter what the wrapper does. The
    # real invocation this models is an interactive Ctrl-C, where SIGINT is not
    # ignored. Monitor mode is scoped to the launch alone and switched straight
    # back off. The same run also confirms bash defers a trap until the running
    # FOREGROUND child returns: TRAP-INT printed and FELL-THROUGH did not.
    set -m
    env REIFY_JC_SERVE_WITNESS="$RW_WITNESS" \
        REIFY_JC_SERVE_READY_TIMEOUT="$RW_TIMEOUT" \
        REIFY_JC_SERVE_CMD="$stub" \
        "$JC_SERVE" --port "$RW_PORT" "$@" >"$RW_OUT" 2>"$RW_ERR" &
    RWB_PID=$!
    set +m
    _BGPIDS+=("$RWB_PID")
    return 0
}

wait_for_ready() {
    local errfile="$1" secs="$2" i
    for i in $(seq 1 $((secs * 10))); do
        grep -qF "$READY_MARKER" "$errfile" 2>/dev/null && return 0
        sleep 0.1
    done
    printf '%s\n' "the wrapper never reported $READY_MARKER within ${secs}s:" "$(cat "$errfile" 2>/dev/null)"
    return 1
}

# b6_signalled <SIG> <expected status>
#
# The wrapped command is a SHORT sleep on purpose. bash defers a trap until the
# running FOREGROUND child returns (the same property run-gui-dev.sh:169-171
# names), and the wrapper deliberately keeps the wrapped command in the
# foreground so its stdin stays inherited — so the handler fires when the sleep
# ends, not during it. A short sleep keeps that deterministic rather than slow.
b6_signalled() {
    local sig="$1" want="$2"
    local pgid leader gpid rc=0
    rw_run_bg grandchild sleep 3 || return 1
    wait_for_ready "$RW_ERR" 25 || return 1

    # ANTI-VACUITY, checked while the fixture is still LIVE: the port really is
    # held, and it is held by a grandchild rather than by the process the
    # wrapper spawned. Without this the "group gone" assertion below could pass
    # against a teardown that only ever killed the leader.
    pgid="$(witness_field "$RW_WITNESS" stub-pgid)"
    leader="$(witness_field "$RW_WITNESS" stub-pid)"
    gpid="$(witness_field "$RW_WITNESS" stub-grandchild-pid)"
    require_nonempty "recorded stub pgid" "$pgid" || return 1
    require_nonempty "recorded grandchild pid" "$gpid" || return 1
    [ "$gpid" != "$leader" ] || { echo "the grandchild fixture is degenerate: listener pid == leader pid"; return 1; }
    port_is_free "$RW_PORT" && { echo "the grandchild stub is not holding the port — the fixture proves nothing"; return 1; }
    kill -0 "$gpid" 2>/dev/null || { echo "the grandchild listener is already gone before the signal"; return 1; }

    kill -"$sig" "$RWB_PID" 2>/dev/null || { echo "could not signal the wrapper (pid $RWB_PID)"; return 1; }
    wait "$RWB_PID" || rc=$?
    [ "$rc" = "$want" ] || { printf '%s\n' "a SIG$sig-ed wrapper exited $rc (expected $want)" "$(cat "$RW_ERR")"; return 1; }
    wait_gone "$pgid" "$RW_PORT" 5
}

assert "a SIGTERMed wrapper exits 143 and tears the serve group down"       b6_signalled TERM 143
assert "a SIGINTed wrapper exits 130 and tears the serve group down"        b6_signalled INT 130

# ─────────────────────────────────────────────────────────────────────────────
# Block 7 — the trailing-stderr invariant
#
# THIS IS THE ASSERTION THAT PROTECTS THIS TASK'S OWN USER-OBSERVABLE SIGNAL.
# `reify-audit` writes its JSON findings array to STDERR
# (crates/reify-audit/src/bin/reify-audit.rs:689-695) and every consumer
# extracts it as the TRAILING block — scripts/smoke-predone-hook.sh:233 pipes
# through the awk below, and crates/reify-audit/tests/cli.rs uses
# `rfind("\n[")`. So a single line of wrapper chatter appended to stderr AFTER
# the wrapped command silently breaks that extraction, and δ's whole PRD signal
# ("emits a findings array") fails with the wrapper still reporting success.
#
# Silent on success, loud on a leak: both halves are pinned here, and the leak
# half is what stops "silent on success" being satisfied by simply deleting the
# teardown diagnostic.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 7: the trailing-stderr invariant ---"

# mk_findings_emitter <path> <exit-code> — a reify-audit-shaped wrapped command:
# diagnostic lines, then a JSON array, all on STDERR, then the given status.
mk_findings_emitter() {
    cat > "$1" <<EMITTER
#!/usr/bin/env bash
printf 'reify-audit: resolving jcodemunch identity local/reify-4ae45bbd\n' >&2
printf 'reify-audit: running detector P1 over 3 changed symbols\n' >&2
printf '[\n' >&2
printf '  {"pattern":"P1","severity":"high","symbol":"foo::bar"}\n' >&2
printf ']\n' >&2
exit $2
EMITTER
    chmod +x "$1"
}

# THE CANONICAL CONSUMER EXTRACTOR, byte-for-byte as smoke-predone-hook.sh:233
# spells it. Copied rather than approximated: an extractor that differed from
# the real one could pass here while the real consumer broke.
extract_trailing_array() {
    awk 'BEGIN{p=0} /^\[/{p=1} p{print}' "$1" | jq -e 'type == "array"' >/dev/null
}

B7_DIR="$(mk_tmpdir)"

b7_trailing_array() {
    local want_rc="$1"
    local emitter="$B7_DIR/emit-$want_rc"
    mk_findings_emitter "$emitter" "$want_rc" || return 1
    rw_run healthy "$emitter" || return 1
    [ "$RW_RC" = "$want_rc" ] || { printf '%s\n' "wrapper status $RW_RC, expected $want_rc"; return 1; }
    extract_trailing_array "$RW_ERR" && return 0
    printf '%s\n' "the trailing findings array no longer extracts from the wrapper's stderr:" "$(cat "$RW_ERR")"
    return 1
}

# The direct form of "silent on success": the wrapped command's last stderr byte
# is still the LAST thing on the wrapper's stderr. Strictly stronger than the
# extractor test — the extractor would survive a trailing line that happened to
# parse as part of the array.
b7_last_line_is_the_array() {
    local emitter="$B7_DIR/emit-tail" last
    mk_findings_emitter "$emitter" 0 || return 1
    rw_run healthy "$emitter" || return 1
    last="$(tail -n1 "$RW_ERR")"
    [ "$last" = "]" ] && return 0
    printf '%s\n' "the last line of the wrapper's stderr is [$last], not the array's closing bracket:" "$(cat "$RW_ERR")"
    return 1
}

assert "a successful reify-audit-shaped run still yields a trailing array"  b7_trailing_array 0
assert "a FAILING reify-audit-shaped run still yields a trailing array"     b7_trailing_array 2
assert "teardown appends nothing after the wrapped command's last byte"     b7_last_line_is_the_array

# -- the LEAK half ------------------------------------------------------------
#
# Without this, "silent on success" is satisfiable by deleting the teardown
# diagnostic entirely — and a leaked serve keeps holding the port, so the very
# next invocation would refuse E_JC_SERVE_PORT_BUSY with nothing in the log to
# say why. Each leak run costs the wrapper's full 10 s free-wait, so exactly two
# are driven and every assertion reads their captured output rather than
# re-running.
B7L_EMITTER="$B7_DIR/emit-leak"
mk_findings_emitter "$B7L_EMITTER" 0
rw_run escapee "$B7L_EMITTER"
B7L_ERR="$RW_ERR"; B7L_RC="$RW_RC"; B7L_PORT="$RW_PORT"
# Observed IMMEDIATELY after the run, before anything else can disturb it.
B7L_STILL_HELD=0
port_is_free "$B7L_PORT" || B7L_STILL_HELD=1

B7F_EMITTER="$B7_DIR/emit-leak-fail"
mk_findings_emitter "$B7F_EMITTER" 9
rw_run escapee "$B7F_EMITTER"
B7F_ERR="$RW_ERR"; B7F_RC="$RW_RC"

# ANTI-VACUITY, and it is a check on the FIXTURE, not on the wrapper: did the
# escapee really escape? If the wrapper's group signal had reached it there
# would be no leak to report, and every assertion below would be about a case
# that never happened. Measured independently of anything the wrapper printed.
b7_leak_fixture_escaped() {
    [ "$B7L_STILL_HELD" = "1" ] && return 0
    echo "the escapee stub did NOT survive teardown, so there is no leak to report and this block is vacuous"
    return 1
}

# leak_report_has <errfile> <needle> — the needle must appear in the LEAK
# REPORT, i.e. at or after the marker line, not merely somewhere on stderr. The
# port number in particular already appears in the readiness line, so a
# whole-file search would pass without any report existing at all.
leak_report_has() {
    local f="$1" needle="$2"
    require_nonempty "needle" "$needle" || return 1
    [ -f "$f" ] || { echo "no such file: $f"; return 1; }
    awk -v m="$M_LEAKED" 'BEGIN{p=0} index($0,m){p=1} p{print}' "$f" | grep -qF -- "$needle" && return 0
    printf '%s\n' "'$needle' not found in the leak report portion of $f:" "$(cat "$f")"
    return 1
}

b7_leak_fixture_real() { has_line "$B7L_ERR" "$M_LEAKED"; }
b7_leak_names_port()   { leak_report_has "$B7L_ERR" "$B7L_PORT"; }
b7_leak_names_ss()     { leak_report_has "$B7L_ERR" "ss -ltnp"; }
# A leak on an otherwise-successful run must NOT report success: the leaked
# serve keeps holding the port and would make the next invocation refuse.
b7_leak_nonzero()      {
    [ "$B7L_RC" -ne 0 ] && return 0
    echo "a run that leaked a serve still exited 0"
    return 1
}
# ...but when the wrapped command already failed, its status is the one that
# matters and must not be masked. α's unwinding case (jcodemunch_session_live.rs
# :325-331) makes the same distinction.
b7_leak_preserves_status() {
    [ "$B7F_RC" = "9" ] && return 0
    printf '%s\n' "a leak masked the wrapped command's status: got $B7F_RC, expected 9" "$(cat "$B7F_ERR")"
    return 1
}
b7_leak_still_reported() { has_line "$B7F_ERR" "$M_LEAKED"; }

assert "the escapee stub really survives the group signal (anti-vacuity)"     b7_leak_fixture_escaped
assert "a leaked serve is reported $M_LEAKED"                                b7_leak_fixture_real
assert "the leak report names the port"                                      b7_leak_names_port
assert "the leak report names 'ss -ltnp' as the reclaim instruction"         b7_leak_names_ss
assert "a leak on an otherwise-successful run exits non-zero"                b7_leak_nonzero
assert "a leak does not mask the wrapped command's own failing status"       b7_leak_preserves_status
assert "a leak is still reported when the wrapped command failed"            b7_leak_still_reported

test_summary
