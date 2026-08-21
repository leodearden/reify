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

mk_tmpdir() {
    local d
    d="$(mktemp -d "${TMPDIR:-/tmp}/jc-with-serve-XXXXXX")" || return 1
    _TMPDIRS+=("$d")
    printf '%s\n' "$d"
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

test_summary
