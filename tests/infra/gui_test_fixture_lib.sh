#!/usr/bin/env bash
# tests/infra/gui_test_fixture_lib.sh — run a copy of a gui-test.sh-shaped script
# against recording stubs and query the calls it made.
#
# Consumers: tests/infra/test_gui_test_script.sh (Tests 3 and 6, whose impostor
# pins prove each predicate tells an invocation from a mention) and
# tests/infra/test_gui_vitest_rpc_hardening.sh (C2).
#
# PREDICATE CONTRACT. Every script_* check, here and in its consumers, returns
# 0 when the property holds, 1 when it is violated, and 2 when the stubbed run
# failed, so nothing was judged.
#
# Not test_*-named, so run_all.sh does not discover it as a member.
#
# Usage:
#   [ -f "$SCRIPT_DIR/gui_test_fixture_lib.sh" ] || { echo "ERROR: gui_test_fixture_lib.sh not found at $SCRIPT_DIR/gui_test_fixture_lib.sh"; exit 1; }
#   source "$SCRIPT_DIR/gui_test_fixture_lib.sh"

# call_line <program> [args...]: one call-log line, each argument %q-quoted so
# argument boundaries survive. The stubs embed this function, so a recorded
# call and an expected one share one encoding.
call_line() {
    printf '%s' "$1"
    shift
    [ "$#" -eq 0 ] || printf ' %q' "$@"
    printf '\n'
}

# gui_test_calls <script> [args...]: run a copy of <script> as scripts/gui-test.sh
# with [args...], in a fresh repo-shaped tree whose npm and sibling
# scripts/gui-vitest-run.sh are recording stubs, and print the calls they made,
# one call_line each. A non-zero run prints its output to stderr and returns the
# run's exit status. The tree is removed on every return path.
gui_test_calls() {
    local script=$1 dir rc=0
    shift
    dir="$(mktemp -d "${TMPDIR:-/tmp}/reify-guitest-run.XXXXXX")" || return 1
    _gui_test_calls_in "$dir" "$script" "$@" || rc=$?
    rm -rf "$dir"
    return "$rc"
}

_gui_test_calls_in() {
    local dir=$1 script=$2
    shift 2
    mkdir "$dir/scripts" "$dir/gui" "$dir/bin" || return 1
    cp "$script" "$dir/scripts/gui-test.sh" || return 1
    {
        printf '#!/usr/bin/env bash\n'
        declare -f call_line
        printf '%s\n' 'call_line "${0##*/}" "$@" >>"$GUI_TEST_CALL_LOG"'
    } >"$dir/bin/npm" || return 1
    cp "$dir/bin/npm" "$dir/scripts/gui-vitest-run.sh" || return 1
    chmod +x "$dir/scripts/gui-test.sh" "$dir/scripts/gui-vitest-run.sh" "$dir/bin/npm" || return 1
    : >"$dir/calls.log"
    GUI_TEST_CALL_LOG="$dir/calls.log" PATH="$dir/bin:$PATH" \
        "$dir/scripts/gui-test.sh" "$@" >"$dir/out.log" 2>&1 \
        || { local rc=$?; cat "$dir/out.log" >&2; return "$rc"; }
    cat "$dir/calls.log"
}

# first_call <calls> <program> [first-arg]: 1-based index of the first call to
# <program> (whose first argument is <first-arg>, when given); empty when none.
first_call() {
    awk -v program="$2" -v first_arg="${3:-}" \
        '$1 == program && (first_arg == "" || $2 == first_arg) { print NR; exit }' <<<"$1"
}

# has_call <calls> <program> [args...]: <calls> holds exactly this call. Matched
# as a fixed whole line: `awk -v` would rewrite the backslashes %q emits.
has_call() {
    local calls=$1 want
    shift
    want="$(call_line "$@")"
    grep -qxF -- "$want" <<<"$calls"
}

# script_runs_runner <script>: a default run invokes scripts/gui-vitest-run.sh.
script_runs_runner() {
    local calls; calls="$(gui_test_calls "$1")" || return 2
    [ -n "$(first_call "$calls" gui-vitest-run.sh)" ]
}
