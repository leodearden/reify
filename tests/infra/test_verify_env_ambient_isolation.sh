#!/usr/bin/env bash
# Regression guard for task 4966: knob-sensitive tests/infra self-suites must
# behave identically under the production dark-factory-orchestrator.yaml verify_env
# ambient, not just in a clean default shell.
#
# Root cause this guards against: dark-factory-orchestrator.yaml's verify_env block
# exports REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 and REIFY_GATE_EXCLUDE_HEAVY=1
# (plus sccache/incremental/semaphore/PSI knobs) into the whole verify.sh
# process tree by design. Deploy 65b8412206 flipped both knobs to "1" and
# broke two infra self-suites in one night -- each caught only at L2 after a
# multi-hour debugger loop, because both suites were GREEN standalone (a bare
# invocation never sets the ambient) and only RED in-pipeline (where the
# ambient is genuinely present). Task 4961 guarded test_run_all.sh
# (test_run_all_ambient_isolation.sh). Task 4965 fixed the resulting
# shell-quoting bug in test_occt_flock_gate.sh's T1/T3-T7 asserts
# (5af93e53c0). This file is the general drift-guard: it extracts the FULL
# verify_env export set directly from dark-factory-orchestrator.yaml -- the single source
# the orchestrator itself injects from, so there is no second manifest to
# drift out of sync -- and re-runs a knob-sensitive suite once under that
# exact ambient, so a FUTURE knob flip that breaks ANY suite is caught here
# first instead of at L2.
#
# This file cannot live inside test_occt_flock_gate.sh itself: it drives the
# REAL suite exactly once, as a subprocess, under a hostile ambient export --
# the same shape dark-factory-orchestrator.yaml's verify_env produces -- and asserts the
# nested suite still exits 0 with 0 failed. Mirrors the run-the-real-suite-
# once idiom from test_run_all_ambient_isolation.sh (task 4961).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$REPO_ROOT/dark-factory-orchestrator.yaml" ] || { echo "ERROR: dark-factory-orchestrator.yaml not found at $REPO_ROOT/dark-factory-orchestrator.yaml"; exit 1; }

echo "=== verify_env ambient-isolation drift-guard (task 4966) ==="

# ---------------------------------------------------------------------------
# verify_env_exports <yaml_file>
#
# Extracts dark-factory-orchestrator.yaml's `verify_env:` block as a stream of
# `KEY=VALUE` lines (one per line). Values are simple tokens
# (0/1/2/sccache/unlimited), so callers can `export "KEY=VALUE"` each
# emitted line directly with no further quoting/escaping.
#
# Single awk pass: enter the block on a top-level `verify_env:` line; a
# later top-level, non-comment line (anything NOT starting with whitespace
# or `#`) ends the block, so trailing top-level `#` comments (e.g. the
# jobserver-wiring note preceding dark-factory-orchestrator.yaml's `jobserver:` block)
# do not end it early. While in-block, blank lines and `#` comment lines
# (indented or not) are skipped. Each `  KEY: VALUE` line emits `KEY=VALUE`:
# a double-quoted value is captured verbatim between the first pair of
# quotes; a bare value is reduced to its first whitespace-delimited token
# (dropping any trailing inline comment).
# ---------------------------------------------------------------------------
verify_env_exports() {
    local yaml_file="$1"
    awk '
        /^verify_env:[[:space:]]*$/ { in_block = 1; next }
        in_block && /^[^[:space:]#]/ { in_block = 0 }
        !in_block { next }
        /^[[:space:]]*#/ { next }
        /^[[:space:]]*$/ { next }
        /^[[:space:]]+[A-Za-z_][A-Za-z0-9_]*:/ {
            line = $0
            sub(/^[[:space:]]+/, "", line)
            colon = index(line, ":")
            key = substr(line, 1, colon - 1)
            rest = substr(line, colon + 1)
            sub(/^[[:space:]]+/, "", rest)
            if (substr(rest, 1, 1) == "\"") {
                tail = substr(rest, 2)
                q = index(tail, "\"")
                val = (q > 0) ? substr(tail, 1, q - 1) : tail
            } else {
                n = split(rest, toks, /[[:space:]]+/)
                val = (n >= 1) ? toks[1] : ""
            }
            print key "=" val
        }
    ' "$yaml_file"
}

# ---------------------------------------------------------------------------
# Extractor correctness (a): synthetic fixture.
#
# Exercises block-entry, block-exit, quoted values, bare values, an in-block
# comment, and a blank line -- independent of whatever dark-factory-orchestrator.yaml
# happens to contain today.
# ---------------------------------------------------------------------------
echo ""
echo "--- Extractor correctness (a): synthetic fixture ---"

_FIXTURE="$(mktemp)"
trap 'rm -f "$_FIXTURE"' EXIT

cat > "$_FIXTURE" <<'FIXTURE_EOF'
pre_block_key: pre_block_value
verify_env:
  ALPHA: "1"
  BETA: bare_value
  # in-block comment line, must be skipped
  GAMMA: "unlimited"
next_block:
  child_key: child_value
FIXTURE_EOF

_FIXTURE_ACTUAL="$(verify_env_exports "$_FIXTURE" | sort)"
_FIXTURE_EXPECTED="$(printf '%s\n' 'ALPHA=1' 'BETA=bare_value' 'GAMMA=unlimited' | sort)"

assert "fixture: verify_env_exports emits exactly ALPHA=1, BETA=bare_value, GAMMA=unlimited (sort-compare)" \
    test "$_FIXTURE_ACTUAL" = "$_FIXTURE_EXPECTED"

assert "fixture: verify_env_exports does not leak the pre-block top-level key" \
    bash -c '! printf "%s\n" "$1" | grep -qF "pre_block"' _ "$_FIXTURE_ACTUAL"

assert "fixture: verify_env_exports does not leak the next_block child key" \
    bash -c '! printf "%s\n" "$1" | grep -qF "child_key"' _ "$_FIXTURE_ACTUAL"

assert "fixture: verify_env_exports does not leak the in-block comment line" \
    bash -c '! printf "%s\n" "$1" | grep -qF "in-block comment"' _ "$_FIXTURE_ACTUAL"

# ---------------------------------------------------------------------------
# Extractor correctness (b): real dark-factory-orchestrator.yaml.
#
# Non-vacuity: proves the emitted set actually carries the two knobs that
# caused the escapes, and that block-exit correctly stops before the
# following `jobserver:` block's `enabled: true` (a real adjacent top-level
# key in dark-factory-orchestrator.yaml today).
# ---------------------------------------------------------------------------
echo ""
echo "--- Extractor correctness (b): real dark-factory-orchestrator.yaml ---"

_REAL_ACTUAL="$(verify_env_exports "$REPO_ROOT/dark-factory-orchestrator.yaml")"

assert "dark-factory-orchestrator.yaml: verify_env_exports output contains REIFY_GATE_EXCLUDE_HEAVY=1" \
    bash -c 'printf "%s\n" "$1" | grep -qxF "REIFY_GATE_EXCLUDE_HEAVY=1"' _ "$_REAL_ACTUAL"

assert "dark-factory-orchestrator.yaml: verify_env_exports output contains REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1" \
    bash -c 'printf "%s\n" "$1" | grep -qxF "REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1"' _ "$_REAL_ACTUAL"

assert "dark-factory-orchestrator.yaml: verify_env_exports output does not leak jobserver's enabled=true (block-end guard)" \
    bash -c '! printf "%s\n" "$1" | grep -qxF "enabled=true"' _ "$_REAL_ACTUAL"

assert "dark-factory-orchestrator.yaml: verify_env_exports output is non-empty and every line is well-formed KEY=... " \
    bash -c '[ -n "$1" ] && ! printf "%s\n" "$1" | grep -vE "^[A-Za-z_][A-Za-z0-9_]*="' _ "$_REAL_ACTUAL"

# ---------------------------------------------------------------------------
# The nested run is BOUNDED and its outcomes are DISTINGUISHABLE (task 6247)
#
# This file drives BOTH of its end-to-end assertions from ONE invocation of a
# real suite. Unbounded, that has two costs. A wedge inside the nested suite
# wedges THIS file too, until the outer `timeout --kill-after=60 30m` envelope
# kills the whole of run_all.sh -- at which point the attribution is gone and
# the failure reads as "run_all was interrupted", not "the nested suite hung".
# And a wedge is reported through the same `$amb_rc -ne 0` channel as a suite
# that ran to completion and failed an assertion, which are opposite diagnoses:
# one is an infrastructure hang, the other a real regression.
#
# _amb_run_under_ambient is the whole nested run behind one interface -- extract
# the ambient, prove it applied, bound the child -- so the wedge path can be
# exercised here against the REAL code path with a tiny fixture suite, rather
# than being asserted about a stub or left untested until it happens for real.
#
# The budget is a BROKEN-INFRA BACKSTOP, not a timing assertion: every case
# below asserts an exit CODE, never a measured magnitude. The live budget is
# generous enough never to discriminate (the nested suite measured 38-246s on
# this host) while still firing well inside the 30m outer envelope, which is the
# only way the attribution survives.
# ---------------------------------------------------------------------------
# _amb_run_under_ambient YAML SUITE BUDGET_SECS
# Run SUITE under the production verify_env ambient extracted from YAML, bounded
# by BUDGET_SECS, with the child's stderr merged into stdout. Echoes that
# combined output.
#
# Returns the suite's OWN exit code, or one of two codes of its own:
#   99   the ambient was not applied, so the run proves nothing (the
#        non-vacuity preflight -- see the section comment above);
#   124  the backstop fired, i.e. the suite WEDGED.
#
# The export loop and the preflight run inside this function's own subshell, so
# no ambient export leaks back to the caller. verify_env_exports is a shell
# function and is inherited by that subshell like any other.
#
# BUDGET_SECS is a BROKEN-INFRA BACKSTOP, not a timing assertion: nothing here
# or at any call site compares a measured magnitude. Its only job is to end a
# hang early enough that the outcome is still attributable to THIS suite.
_amb_run_under_ambient() {
    local _yaml="$1" _suite="$2" _budget="$3"
    (
        while IFS= read -r _kv; do
            export "$_kv"
        done < <(verify_env_exports "$_yaml")
        [ "${REIFY_GATE_EXCLUDE_HEAVY:-}" = "1" ] || { echo "AMBIENT-NOT-APPLIED"; exit 99; }
        timeout "$_budget" bash "$_suite" 2>&1
    )
}

# _amb_nested_verdict RC SUITE
# Echo one line saying what RC means for SUITE; return 0 only for a clean run.
#
# The three non-zero outcomes are three DIFFERENT diagnoses and must never be
# read as one another: a wedge is an infrastructure hang, a completed failure is
# a regression, and a missing ambient means the run settled nothing. Reported as
# a structured `outcome=` token rather than prose, so a reader (or a later
# classifier) reaches the diagnosis without parsing a sentence.
_amb_nested_verdict() {
    local _rc="$1" _suite="$2"
    case "$_rc" in
        0)
            echo "nested=$_suite outcome=passed rc=0" ;;
        124)
            echo "nested=$_suite outcome=wedged rc=124 -- it never finished and the anti-hang backstop ended it. That is a HANG in $_suite, not a failed assertion inside it: look for a barrier that never released, not for a regression." ;;
        99)
            echo "nested=$_suite outcome=ambient-not-applied rc=99 -- the hostile verify_env ambient was not in effect, so this run proves nothing either way." ;;
        *)
            echo "nested=$_suite outcome=failed rc=$_rc -- it ran to completion and reported failures. That is a REGRESSION in $_suite, not a hang." ;;
    esac
    [ "$_rc" -eq 0 ]
}

echo ""
echo "--- Nested-run backstop: bounded, and wedge distinguishable from failure ---"

_AMB_YAML="$REPO_ROOT/dark-factory-orchestrator.yaml"
_AMB_SUITE_NAME="test_occt_flock_gate.sh"
# Generous by design: the nested suite has measured 16-246s on this host, and
# the whole of run_all.sh runs under a 30m outer envelope. 900s never
# discriminates against a slow-but-alive run, yet still fires far enough inside
# that envelope for the outcome to be attributed to this suite rather than
# surfacing as "run_all was interrupted".
_AMB_NESTED_BACKSTOP_SECS=900

_AMB_TMPDIRS=()
trap '[ "${#_AMB_TMPDIRS[@]}" -gt 0 ] && rm -rf "${_AMB_TMPDIRS[@]}"' EXIT

# _amb_fixture_suite LINE... -- a throwaway stand-in for the nested suite.
_amb_fixture_suite() {
    local _d; _d="$(mktemp -d)"; _AMB_TMPDIRS+=("$_d")
    local _l
    printf '#!/usr/bin/env bash\n' > "$_d/suite.sh"
    for _l in "$@"; do printf '%s\n' "$_l" >> "$_d/suite.sh"; done
    echo "$_d/suite.sh"
}

# _amb_nested_rc BUDGET SUITE -- echo _amb_run_under_ambient's exit code.
_amb_nested_rc() {
    local _rc=0
    _amb_run_under_ambient "$_AMB_YAML" "$2" "$1" >/dev/null 2>&1 || _rc=$?
    echo "$_rc"
}

_amb_wedge_suite="$(_amb_fixture_suite 'sleep 600')"
assert "a WEDGED nested suite surfaces as the backstop's own exit code 124, instead of hanging this file until the outer envelope kills run_all" \
    test "$(_amb_nested_rc 2 "$_amb_wedge_suite")" -eq 124

# The discriminator. Without it the backstop could "pass" by mapping every
# non-zero outcome onto 124, which would erase the distinction it exists to make.
_amb_fail_suite="$(_amb_fixture_suite 'exit 3')"
assert "a nested suite that RAN and failed passes its own exit code through unchanged (3, not the backstop's 124)" \
    test "$(_amb_nested_rc 60 "$_amb_fail_suite")" -eq 3

_amb_pass_suite="$(_amb_fixture_suite 'echo "Results: 1 passed, 0 failed"')"
assert "a nested suite that passed reports 0 through the backstop" \
    test "$(_amb_nested_rc 60 "$_amb_pass_suite")" -eq 0

# The existing exit-99 preflight, now proven through the extracted seam: the
# hostile ambient must genuinely reach the nested child, or every verdict above
# is about a run that proves nothing.
_amb_probe_suite="$(_amb_fixture_suite 'echo "HEAVY=${REIFY_GATE_EXCLUDE_HEAVY:-unset}"')"
# Run in THIS shell, not via `bash -c`: _amb_run_under_ambient is not exported,
# and a fresh shell would report command-not-found -- which grep would then read
# as a plain absence, making the case pass or fail for the wrong reason.
_amb_ambient_reaches_child() {
    _amb_run_under_ambient "$_AMB_YAML" "$1" 60 2>&1 | grep -qxF "HEAVY=1"
}
assert "the production ambient genuinely reaches the nested child (REIFY_GATE_EXCLUDE_HEAVY=1 observed inside it)" \
    _amb_ambient_reaches_child "$_amb_probe_suite"

# _amb_verdict_case RC WANT_ZERO PATTERN... -- the verdict for RC must carry the
# right success/failure sense and name every PATTERN.
_amb_verdict_case() {
    local _rc="$1" _want_zero="$2"; shift 2
    local _out _vrc=0 _pat _bad=0
    _out="$(_amb_nested_verdict "$_rc" "$_AMB_SUITE_NAME" 2>&1)" || _vrc=$?
    echo "rc=$_rc -> vrc=$_vrc verdict: $_out"
    if [ "$_want_zero" = "yes" ] && [ "$_vrc" -ne 0 ]; then
        echo "the verdict for rc=$_rc reported failure; a clean nested run must not."
        _bad=1
    fi
    if [ "$_want_zero" = "no" ] && [ "$_vrc" -eq 0 ]; then
        echo "the verdict for rc=$_rc reported success; that outcome is not a pass."
        _bad=1
    fi
    for _pat in "$@"; do
        case "$_out" in
            *"$_pat"*) ;;
            *) echo "the verdict for rc=$_rc never mentions '$_pat'."; _bad=1 ;;
        esac
    done
    return "$_bad"
}

assert "verdict(124): names the nested suite and calls it a WEDGE, and is not a pass" \
    _amb_verdict_case 124 no "$_AMB_SUITE_NAME" wedged
assert "verdict(3): names the nested suite and says it RAN and failed -- not a wedge" \
    _amb_verdict_case 3 no "$_AMB_SUITE_NAME" failed
assert "verdict(99): names the ambient-not-applied preflight, so a proves-nothing run is never read as either of the other two" \
    _amb_verdict_case 99 no "$_AMB_SUITE_NAME" ambient-not-applied
assert "verdict(0): a clean nested run is a pass" \
    _amb_verdict_case 0 yes "$_AMB_SUITE_NAME" passed

# ---------------------------------------------------------------------------
# End-to-end: test_occt_flock_gate.sh under the REAL production ambient.
#
# Mirrors test_run_all_ambient_isolation.sh (task 4961)'s run-the-real-
# suite-once idiom, generalized from one hardcoded knob to the FULL
# verify_env export set. The ambient export loop, the preflight and the
# anti-hang backstop all live in _amb_run_under_ambient above, which the
# section before this one exercises against fixture suites.
#
# Non-vacuity: post-4965, test_occt_flock_gate.sh exits 0 under BOTH the
# default env and this ambient, so its exit code alone can't prove the
# ambient was actually applied. The in-subshell probe asserts
# REIFY_GATE_EXCLUDE_HEAVY=1 is genuinely set (exit 99 -> RED) before the
# suite ever runs, so "occt exits 0 under a PROVABLY hostile ambient" is the
# real, non-vacuous claim below.
# ---------------------------------------------------------------------------
echo ""
echo "--- End-to-end: test_occt_flock_gate.sh under the real verify_env ambient ---"

amb_rc=0
amb_out="$(_amb_run_under_ambient "$_AMB_YAML" "$SCRIPT_DIR/$_AMB_SUITE_NAME" "$_AMB_NESTED_BACKSTOP_SECS")" || amb_rc=$?

# Emitted BEFORE the assert, so a wedge is attributed even to a reader who sees
# nothing but this file's own output.
_amb_nested_verdict "$amb_rc" "$_AMB_SUITE_NAME" || true

assert "test_occt_flock_gate.sh exits 0 under the real verify_env ambient (got rc=$amb_rc)" \
    test "$amb_rc" -eq 0

# Anchored line match (not a substring grep) so an inner mock's own
# "0 failed"-shaped output could never false-pass this assertion -- only the
# nested test_occt_flock_gate.sh's OWN test_summary line qualifies.
#
# $amb_out below is the COMBINED stdout+stderr of a DEADLINE-CAPABLE child:
# test_occt_flock_gate.sh reaches a real REIFY_OCCT_LOCK_WAIT deadline and
# emits a column-0 @@REIFY_SLOT_TIMEOUT@@ sentinel. Interpolating it into an
# assert description is safe ONLY because test_helpers.sh's assert() prefixes
# lines 2+ of $desc with `  | ` (_assert_emit_desc, task 6353) -- a
# NON-whitespace prefix, which is what defeats dark-factory's `^[ \t]*`-anchored
# classifier; indentation alone does not. Do NOT "simplify" that emitter back
# to a bare echo, and do not add a second local filter here: the structural fix
# is the fix. Behavioural pin: test_slot_timeout_marker.sh E4.
if printf '%s\n' "$amb_out" | grep -qE '^Results: [0-9]+ passed, 0 failed$'; then
    assert "test_occt_flock_gate.sh reports 0 failed under the real verify_env ambient" true
else
    assert "test_occt_flock_gate.sh reports 0 failed under the real verify_env ambient (got: $amb_out)" false
fi

test_summary
