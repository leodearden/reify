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

test_summary
