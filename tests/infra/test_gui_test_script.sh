#!/usr/bin/env bash
# Infrastructure tests for the canonical frontend-checkpoint runner
# scripts/gui-test.sh (esc-5232-3).
#
# Validates the script's CONTRACT (source checks, arg-parsing smoke, and Tests 3
# and 6, which run a copy against stub npm / stub runner via
# gui_test_fixture_lib.sh) so the guarantee "an agent can reliably run the gui
# vitest suites in any lane" can't silently regress.
# Deliberately never executes the real `npm ci`/typecheck/vitest path (slow,
# network-adjacent) — mirroring test_run_gui_scripts.sh, which checks the
# launcher contract without launching the GUI.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"
[ -f "$SCRIPT_DIR/gui_test_fixture_lib.sh" ] || { echo "ERROR: gui_test_fixture_lib.sh not found at $SCRIPT_DIR/gui_test_fixture_lib.sh"; exit 1; }
source "$SCRIPT_DIR/gui_test_fixture_lib.sh"

GUI_TEST="$REPO_ROOT/scripts/gui-test.sh"

# Suite-private scratch root for the impostor scripts of Tests 3 and 6 (each
# stubbed run makes and removes its own tree). Neither sourced lib installs an
# EXIT trap, so this one clobbers nothing.
FIXTURE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/reify-guitest.XXXXXX")"
trap 'rm -rf "$FIXTURE_ROOT"' EXIT

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
#
# Task 7630 moved the vitest leaf itself into scripts/gui-vitest-run.sh, shared
# with verify.sh's gui block, so the two can no longer drift. This script's
# contract is unchanged in substance -- provision, then typecheck, then vitest;
# only the spelling of the vitest step moved.
#
# The checks run a copy of the script against a stub npm and a stub runner and
# judge the calls those recorded, so a usage line or progress echo that merely
# names `npm ci` or the runner cannot satisfy them. The impostor pins below
# prove each check tells an invocation from a mention (tests/infra/README.md
# "Assert behaviour, not source prose"). script_runs_runner lives in
# gui_test_fixture_lib.sh, shared with rpc-hardening C2, so its no-runner pin
# here proves C2's sensitivity too.
echo ""
echo "--- Test 3: 'npm ci' precedes the vitest run (self-provisioning) ---"

script_runs_npm_ci() {
    local calls; calls="$(gui_test_calls "$1")" || return 2
    [ -n "$(first_call "$calls" npm ci)" ]
}

script_npm_ci_prefers_offline() {
    local calls; calls="$(gui_test_calls "$1")" || return 2
    awk '$1 == "npm" && $2 == "ci" { for (i = 3; i <= NF; i++) if ($i == "--prefer-offline") found = 1 }
         END { exit !found }' <<<"$calls"
}

script_runs_npm_ci_before_runner() {
    local calls ci runner
    calls="$(gui_test_calls "$1")" || return 2
    ci="$(first_call "$calls" npm ci)"
    runner="$(first_call "$calls" gui-vitest-run.sh)"
    [ -n "$ci" ] && [ -n "$runner" ] && [ "$ci" -lt "$runner" ]
}

assert "scripts/gui-test.sh runs 'npm ci'" \
    script_runs_npm_ci "$GUI_TEST"

assert "scripts/gui-test.sh uses '--prefer-offline' (warm ~/.npm cache, offline-fast)" \
    script_npm_ci_prefers_offline "$GUI_TEST"

assert "scripts/gui-test.sh runs vitest via the shared scripts/gui-vitest-run.sh" \
    script_runs_runner "$GUI_TEST"

assert "scripts/gui-test.sh: 'npm ci' invocation precedes the vitest invocation" \
    script_runs_npm_ci_before_runner "$GUI_TEST"

# Sensitivity pins: an impostor MENTIONS a step in an echo without performing it,
# and lacks exactly the property one check guards. Each check must reject its
# impostor.

# impostor <name> <body>: write <body> as an executable script, print its path.
impostor() {
    local path="$FIXTURE_ROOT/$1.sh"
    printf '#!/usr/bin/env bash\nset -euo pipefail\n%s\n' "$2" >"$path" || return 1
    chmod +x "$path" || return 1
    printf '%s\n' "$path"
}

# rejects <check> <script>: <check> judged <script> and found its property
# violated. Checks return 0 (holds), 1 (violated) or 2 (the stubbed run died, so
# nothing was judged); only 1 counts, so a broken impostor cannot pass vacuously.
rejects() {
    local check=$1 script=$2 rc=0
    "$check" "$script" || rc=$?
    [ "$rc" -eq 1 ]
}

IMPOSTOR_NO_RUNNER="$(impostor no-runner \
    $'npm ci --prefer-offline --no-audit\necho "==> vitest run (via scripts/gui-vitest-run.sh)..."')"
IMPOSTOR_CI_AFTER_RUNNER="$(impostor ci-after-runner \
    $'"$(dirname "$0")/gui-vitest-run.sh"\nnpm ci --prefer-offline')"
IMPOSTOR_NO_PREFER_OFFLINE="$(impostor no-prefer-offline \
    $'echo "==> npm ci --prefer-offline"\nnpm ci --no-audit\n"$(dirname "$0")/gui-vitest-run.sh"')"
IMPOSTOR_NO_NPM_CI="$(impostor no-npm-ci \
    $'echo "==> npm ci --prefer-offline"\n"$(dirname "$0")/gui-vitest-run.sh"')"

assert "sensitivity pin: runs-runner check rejects the no-runner impostor (its echo names the runner, nothing invokes it)" \
    rejects script_runs_runner "$IMPOSTOR_NO_RUNNER"

assert "sensitivity pin: npm-ci-before-runner check rejects the no-runner impostor (no runner invocation to order against)" \
    rejects script_runs_npm_ci_before_runner "$IMPOSTOR_NO_RUNNER"

assert "sensitivity pin: npm-ci-before-runner check rejects the ci-after-runner impostor (runner invoked first)" \
    rejects script_runs_npm_ci_before_runner "$IMPOSTOR_CI_AFTER_RUNNER"

assert "sensitivity pin: prefers-offline check rejects the no-prefer-offline impostor (its echo says --prefer-offline, its npm ci omits it)" \
    rejects script_npm_ci_prefers_offline "$IMPOSTOR_NO_PREFER_OFFLINE"

assert "sensitivity pin: runs-npm-ci check rejects the no-npm-ci impostor (its echo says npm ci, nothing runs it)" \
    rejects script_runs_npm_ci "$IMPOSTOR_NO_NPM_CI"

IMPOSTOR_DIES="$(impostor dies 'exit 64')"

dead_script_is_not_rejected() { ! rejects script_runs_runner "$IMPOSTOR_DIES"; }

assert "sensitivity pin: a pin fails on an impostor that dies (no check ever judged it), so a broken impostor cannot pass vacuously" \
    dead_script_is_not_rejected

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

# -- Test 6: --no-typecheck and '--' forwarding, judged by the calls made ------
echo ""
echo "--- Test 6: --no-typecheck skips typecheck and '--' forwards vitest args (stub-driven) ---"

script_typechecks_by_default() {
    local calls; calls="$(gui_test_calls "$1")" || return 2
    has_call "$calls" npm run typecheck
}

script_no_typecheck_skips_typecheck() {
    local calls; calls="$(gui_test_calls "$1" --no-typecheck)" || return 2
    ! has_call "$calls" npm run typecheck && [ -n "$(first_call "$calls" gui-vitest-run.sh)" ]
}

# The space inside one argument tells a boundary-keeping "${VITEST_ARGS[@]}"
# apart from a joining "${VITEST_ARGS[*]}".
script_forwards_vitest_args() {
    local vitest_args=(-t 'two words' src/a.test.ts) calls
    calls="$(gui_test_calls "$1" --no-typecheck -- "${vitest_args[@]}")" || return 2
    has_call "$calls" gui-vitest-run.sh "${vitest_args[@]}"
}

assert "scripts/gui-test.sh runs 'npm run typecheck' by default" \
    script_typechecks_by_default "$GUI_TEST"

assert "scripts/gui-test.sh --no-typecheck skips 'npm run typecheck' and still runs the runner" \
    script_no_typecheck_skips_typecheck "$GUI_TEST"

assert "scripts/gui-test.sh -- <args> hands the runner exactly those args (boundaries kept)" \
    script_forwards_vitest_args "$GUI_TEST"

IMPOSTOR_NEVER_TYPECHECKS="$(impostor never-typechecks \
    $'npm ci --prefer-offline\necho "==> npm run typecheck"\n"$(dirname "$0")/gui-vitest-run.sh"')"
IMPOSTOR_IGNORES_NO_TYPECHECK="$(impostor ignores-no-typecheck \
    $'echo "usage: [--no-typecheck]"\nnpm ci --prefer-offline\nnpm run typecheck\n"$(dirname "$0")/gui-vitest-run.sh"')"
IMPOSTOR_SKIP_RUNS_NOTHING="$(impostor skip-runs-nothing \
    $'npm ci --prefer-offline\n[ "${1:-}" = --no-typecheck ] && exit 0\nnpm run typecheck\n"$(dirname "$0")/gui-vitest-run.sh"')"
IMPOSTOR_DROPS_VITEST_ARGS="$(impostor drops-vitest-args \
    $'npm ci --prefer-offline\necho "forwarding VITEST_ARGS: $*"\n"$(dirname "$0")/gui-vitest-run.sh"')"
IMPOSTOR_JOINS_VITEST_ARGS="$(impostor joins-vitest-args \
    $'npm ci --prefer-offline\nwhile [ "$#" -gt 0 ]; do arg=$1; shift; [ "$arg" = -- ] && break; done\n"$(dirname "$0")/gui-vitest-run.sh" "$*"')"

assert "sensitivity pin: typechecks-by-default check rejects the never-typechecks impostor (its echo names typecheck, nothing runs it)" \
    rejects script_typechecks_by_default "$IMPOSTOR_NEVER_TYPECHECKS"

assert "sensitivity pin: no-typecheck check rejects the ignores-no-typecheck impostor (its usage names the flag, it typechecks anyway)" \
    rejects script_no_typecheck_skips_typecheck "$IMPOSTOR_IGNORES_NO_TYPECHECK"

assert "sensitivity pin: no-typecheck check rejects the skip-runs-nothing impostor (the flag skips the runner too)" \
    rejects script_no_typecheck_skips_typecheck "$IMPOSTOR_SKIP_RUNS_NOTHING"

assert "sensitivity pin: forwards-args check rejects the drops-vitest-args impostor (its echo names the args, the runner gets none)" \
    rejects script_forwards_vitest_args "$IMPOSTOR_DROPS_VITEST_ARGS"

assert "sensitivity pin: forwards-args check rejects the joins-vitest-args impostor (the runner gets one joined arg)" \
    rejects script_forwards_vitest_args "$IMPOSTOR_JOINS_VITEST_ARGS"

test_summary
