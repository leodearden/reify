#!/usr/bin/env bash
# Infrastructure tests for the GUI vitest worker->host RPC hardening (task 7630).
#
# WHAT IS BEING PINNED. Under merge-lane host starvation the vitest worker's
# birpc channel to the host times out after birpc's hardcoded 60 s
# DEFAULT_TIMEOUT (vitest 3.2.4, dist/chunks/index.*.js:3), producing suites
# that fail with `[vitest-worker]: Timeout calling "<method>"` and ZERO failed
# tests. vitest exposes no knob for that bound (WorkerRpcOptions type-excludes
# `timeout`, dist/workers.d.ts:23), and the merge lane is exempt from CPU
# admission control by design (cpu-admit.sh C-A3), so the flake cannot be
# prevented outright. This suite pins the three things that ARE reachable:
#
#   Section A — the role EXPORT contract, so task 4856's isVerifyLane fork cap
#               actually reaches the vitest child on every path (genuine
#               prevention for the uncapped-31-fork class).
#   Section B — scripts/gui-vitest-run.sh's bounded, signature-gated retry.
#   Section C — the verify.sh / gui-test.sh wiring that makes that runner the
#               single definition of how vitest is invoked.
#
# Hermetic by construction: Sections B and C drive the runner with a STUB npm
# on PATH, never the real 173-file suite — same stance as
# tests/infra/test_gui_test_script.sh, which validates gui-test.sh's contract
# without executing npm.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

VERIFY_SH="$REPO_ROOT/scripts/verify.sh"
VITEST_CONFIG="$REPO_ROOT/gui/vitest.config.ts"

echo "=== gui vitest worker->host RPC hardening tests (task 7630) ==="

# -- Section A: DF_VERIFY_ROLE reaches the vitest child -----------------------
# gui/vitest.config.ts gates BOTH of task 4856's mitigations (maxForks = 4 and
# teardownTimeout = 120 s) on process.env.DF_VERIFY_ROLE. verify.sh defaults
# that variable for every run, but a plain shell assignment is not in the
# environment of the npm/vitest child it later spawns, so on every path where
# the caller does not pre-export the role (hooks/project-checks, and all manual
# or agent-driven runs) the gate reads undefined and vitest silently reverts to
# nCPU-1 forks -- the exact uncapped state 4856 set out to remove.
echo ""
echo "--- Section A: DF_VERIFY_ROLE defaulting assignment is exported ---"

assert "verify.sh's DF_VERIFY_ROLE defaulting assignment carries 'export'" \
    grep -qE '^export DF_VERIFY_ROLE="\$\{DF_VERIFY_ROLE:-task\}"' "$VERIFY_SH"

# The semantics above, in executable form. A comment asserting "a bare
# assignment is invisible to a child" can drift; these two fixtures cannot.
assert "fixture: a BARE 'X=\${X:-task}' is NOT visible in a child's environment" \
    bash -c "! bash -c 'unset X; X=\"\${X:-task}\"; env | grep -q \"^X=\"'"

assert "fixture: an EXPORTED 'X=\${X:-task}' IS visible in a child's environment" \
    bash -c "bash -c 'unset X; export X=\"\${X:-task}\"; env | grep -q \"^X=\"'"

# Guard against a regression that adds a SECOND, unexported defaulting site:
# two assignment sites would make the export conditional on evaluation order.
assert "verify.sh has exactly one DF_VERIFY_ROLE defaulting assignment" \
    bash -c "[ \"\$(grep -cE '^(export )?DF_VERIFY_ROLE=\"\\\$\{DF_VERIFY_ROLE:-task\}\"' '$VERIFY_SH')\" -eq 1 ]"

# -- Section A2: gui/vitest.config.ts cites a real source --------------------
# The config's comment named "verify.sh:328" as where the role is set. Line 328
# is a comment line inside the semaphore documentation block, not code -- a
# false citation sends the next responder to the wrong place.
echo ""
echo "--- Section A2: gui/vitest.config.ts carries no false source citation ---"

assert "gui/vitest.config.ts does not cite the non-existent source 'verify.sh:328'" \
    bash -c "! grep -q 'verify\.sh:328' '$VITEST_CONFIG'"

assert "gui/vitest.config.ts still gates on DF_VERIFY_ROLE (isVerifyLane intact)" \
    grep -q 'DF_VERIFY_ROLE' "$VITEST_CONFIG"

# -- Section B: scripts/gui-vitest-run.sh — the bounded, gated retry ---------
# Driven in a HERMETIC FIXTURE: the runner is copied into a throwaway tree laid
# out like the repo (scripts/ + gui/) so its BASH_SOURCE repo-root resolution
# lands there, and a stub `npm` first on PATH records its argv and replays a
# scripted exit code per invocation. The real 173-file suite is never run --
# same stance as tests/infra/test_verify_throughput.sh, which copies
# cpu-admit.sh into its own fixture.
echo ""
echo "--- Section B: gui-vitest-run.sh contract (stub npm, no real suite) ---"

RUNNER="$REPO_ROOT/scripts/gui-vitest-run.sh"
ARTIFACT_REL="node_modules/.reify-gui-rpc-flake.json"

assert "scripts/gui-vitest-run.sh exists" \
    test -f "$RUNNER"

assert "scripts/gui-vitest-run.sh is executable" \
    test -x "$RUNNER"

assert "scripts/gui-vitest-run.sh passes 'bash -n' syntax check" \
    bash -n "$RUNNER"

assert "scripts/gui-vitest-run.sh carries 'set -euo pipefail'" \
    grep -q 'set -euo pipefail' "$RUNNER"

assert "scripts/gui-vitest-run.sh resolves its repo root from \${BASH_SOURCE[0]}" \
    grep -q 'BASH_SOURCE\[0\]' "$RUNNER"

# The artifact path is the ONE seam between the reporter and the runner. It is
# spelled in two languages, so pin that the two spellings agree.
assert "runner and reporter name the SAME artifact path" \
    bash -c "
        grep -q '$ARTIFACT_REL' '$RUNNER' &&
        grep -q \"WORKER_RPC_FLAKE_ARTIFACT = '$ARTIFACT_REL'\" '$REPO_ROOT/gui/vitest-worker-rpc-flake-reporter.ts'
    "

# fixture_run <artifact-json-or-empty> <exit1> [exit2] -- [runner args...]
# Builds a fresh fixture, scripts the stub npm, runs the runner, and leaves the
# result in FIX_RC / FIX_ARGV (the stub's argv log) / FIX_DIR for assertions.
fixture_run() {
    local artifact="$1" exit1="$2" exit2="${3:-0}"
    shift 3
    [ "${1:-}" = "--" ] && shift

    FIX_DIR="$(mktemp -d "${TMPDIR:-/tmp}/reify-guirun.XXXXXX")"
    mkdir -p "$FIX_DIR/scripts" "$FIX_DIR/gui/node_modules" "$FIX_DIR/bin" "$FIX_DIR/state"
    # Tolerate an absent runner (the RED half of TDD, and any future rename) so
    # every behavioural assertion below reports a FAIL and the suite still
    # reaches test_summary instead of aborting under `set -e` at the first copy.
    if [ -f "$RUNNER" ]; then
        cp "$RUNNER" "$FIX_DIR/scripts/gui-vitest-run.sh"
        chmod +x "$FIX_DIR/scripts/gui-vitest-run.sh"
    fi

    echo "0" > "$FIX_DIR/state/count"
    : > "$FIX_DIR/state/argv.log"
    printf '%s\n' "$exit1" > "$FIX_DIR/state/exit.1"
    printf '%s\n' "$exit2" > "$FIX_DIR/state/exit.2"
    # The stub writes the artifact on its FIRST invocation only, standing in for
    # the reporter having classified that run.
    [ -n "$artifact" ] && printf '%s\n' "$artifact" > "$FIX_DIR/state/artifact.1"

    cat > "$FIX_DIR/bin/npm" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
n=$(( $(cat "$FIX_DIR/state/count") + 1 ))
printf '%s\n' "$n" > "$FIX_DIR/state/count"
printf '%s\n' "$*" >> "$FIX_DIR/state/argv.log"
if [ -f "$FIX_DIR/state/artifact.$n" ]; then
    cp "$FIX_DIR/state/artifact.$n" "$FIX_DIR/gui/node_modules/.reify-gui-rpc-flake.json"
fi
exit "$(cat "$FIX_DIR/state/exit.$n" 2>/dev/null || echo 0)"
STUB
    chmod +x "$FIX_DIR/bin/npm"

    FIX_RC=0
    export FIX_ARGV="$FIX_DIR/state/argv.log"
    FIX_OUT="$FIX_DIR/state/out.log"
    : > "$FIX_OUT"
    if [ -x "$FIX_DIR/scripts/gui-vitest-run.sh" ]; then
        FIX_DIR="$FIX_DIR" PATH="$FIX_DIR/bin:$PATH" \
            "$FIX_DIR/scripts/gui-vitest-run.sh" "$@" >"$FIX_OUT" 2>&1 || FIX_RC=$?
    else
        FIX_RC=127
    fi
}

# Exported so the `bash -c` assertions below (a fresh shell each) can call it.
npm_invocations() { wc -l < "$FIX_ARGV" | tr -d ' '; }
export -f npm_invocations

# Realistic artifact contents: the reporter names failed MODULES, which are
# always collectable test files. (gui/vitest.setup.ts appears in the recorded
# errors as the fetch ARGUMENT, never as the failed suite -- it is a setupFile,
# outside the test include pattern, so vitest could not re-run it as a spec.)
TWO_SUITES='{"kind":"worker_rpc_timeout","suites":["src/__tests__/engineStore.test.ts","src/__tests__/meshManager.attributeResize.test.ts"],"methods":["fetch"]}'

# (1) A green run must not retry, and must not consult the artifact at all.
fixture_run "" 0 0 --
assert "B1: vitest exits 0 => runner exits 0" \
    bash -c "[ \"$FIX_RC\" -eq 0 ]"
assert "B1: vitest exits 0 => npm invoked exactly once (no retry on success)" \
    bash -c "[ \"\$(npm_invocations)\" -eq 1 ]"

# (2) The classified failure: retry exactly the named suites, exactly once.
fixture_run "$TWO_SUITES" 1 0 --
assert "B2: classified failure + passing retry => runner exits 0" \
    bash -c "[ \"$FIX_RC\" -eq 0 ]"
assert "B2: npm invoked exactly twice (bounded to ONE retry)" \
    bash -c "[ \"\$(npm_invocations)\" -eq 2 ]"
assert "B2: the retry passes ONLY the two suites named in the artifact" \
    bash -c "[ \"\$(sed -n 2p '$FIX_ARGV')\" = 'test -- src/__tests__/engineStore.test.ts src/__tests__/meshManager.attributeResize.test.ts' ]"
assert "B2: the retry is announced with an @@REIFY_GUI_FLAKE@@ outcome marker" \
    bash -c "grep -qE '^@@REIFY_GUI_FLAKE@@ .*outcome=retried' '$FIX_OUT'"

# (3) THE CENTRAL SAFETY PROPERTY. No artifact means the reporter did not
# classify this run -- a genuine test failure. Never retry it, never mask it.
fixture_run "" 1 0 --
assert "B3: failure with NO artifact => npm invoked exactly once (no retry)" \
    bash -c "[ \"\$(npm_invocations)\" -eq 1 ]"
assert "B3: failure with NO artifact => original exit code propagated verbatim" \
    bash -c "[ \"$FIX_RC\" -eq 1 ]"

fixture_run "" 7 0 --
assert "B3: a non-1 failure exit code is propagated verbatim too" \
    bash -c "[ \"$FIX_RC\" -eq 7 ]"

# (4) A retry that also fails is reported, not retried again.
fixture_run "$TWO_SUITES" 1 1 --
assert "B4: failing retry => non-zero propagated" \
    bash -c "[ \"$FIX_RC\" -ne 0 ]"
assert "B4: failing retry => still exactly two npm invocations (never a loop)" \
    bash -c "[ \"\$(npm_invocations)\" -eq 2 ]"
assert "B4: a failing retry is still announced (escalated, never silently absorbed)" \
    bash -c "grep -qE '^@@REIFY_GUI_FLAKE@@ .*outcome=escalated' '$FIX_OUT'"

# (5) The kill switch.
REIFY_GUI_RPC_FLAKE_RETRY=0 fixture_run "$TWO_SUITES" 1 0 --
assert "B5: REIFY_GUI_RPC_FLAKE_RETRY=0 disables the retry even with an artifact" \
    bash -c "[ \"\$(npm_invocations)\" -eq 1 ]"
assert "B5: REIFY_GUI_RPC_FLAKE_RETRY=0 propagates the original failure" \
    bash -c "[ \"$FIX_RC\" -eq 1 ]"

# (6) Suite paths come from a file on disk; validate before they reach vitest's
# argument parser. Same allowlist and leading-dash rules as
# REIFY_GUI_RETRY_SPECS in verify.sh -- one definition of a safe spec token.
# A rejected artifact falls back LOUDLY to the original non-zero exit; it must
# never be forwarded and never be silently swallowed.
hostile_case() {
    local label="$1" suites="$2"
    fixture_run "{\"kind\":\"worker_rpc_timeout\",\"suites\":$suites,\"methods\":[\"fetch\"]}" 1 0 --
    assert "B6: rejects $label -- no retry, original exit propagated" \
        bash -c "[ \"\$(npm_invocations)\" -eq 1 ] && [ \"$FIX_RC\" -eq 1 ]"
}

hostile_case "an option-like token (--run)"          '["--run"]'
hostile_case "a leading-dash token (-x)"             '["-x"]'
hostile_case "an absolute path (/etc/passwd)"        '["/etc/passwd"]'
hostile_case "a parent-directory escape (../x.ts)"   '["../x.ts"]'
hostile_case "a command substitution"                '["$(id).ts"]'
hostile_case "a shell metacharacter (;)"             '["a.ts;id"]'
hostile_case "a glob"                                '["*.test.ts"]'
hostile_case "an embedded space"                     '["a b.test.ts"]'
hostile_case "an empty suite list"                   '[]'
hostile_case "a non-string suite entry"              '[17]'

fixture_run 'not json at all' 1 0 --
assert "B6: malformed JSON artifact -- no retry, original exit propagated" \
    bash -c "[ \"\$(npm_invocations)\" -eq 1 ] && [ \"$FIX_RC\" -eq 1 ]"

assert "B6: a rejected artifact says so on stderr (loud, not silent)" \
    bash -c "grep -qi 'WARNING' '$FIX_OUT'"

# Caller-supplied vitest args must still reach the first run unchanged, so
# verify.sh's existing REIFY_GUI_RETRY_SPECS narrowing keeps working.
fixture_run "" 0 0 -- src/__tests__/unitLadder.test.ts
assert "B7: caller-supplied vitest args are forwarded to the first run" \
    bash -c "[ \"\$(sed -n 1p '$FIX_ARGV')\" = 'test -- src/__tests__/unitLadder.test.ts' ]"

test_summary
