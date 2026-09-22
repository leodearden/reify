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

# The RUN-SCOPE verdict (task 7724): a run whose only failures were run-level
# RPC timeouts, so the classifier had no suite to narrow to and named none.
RUN_SCOPE='{"kind":"worker_rpc_timeout","suites":[],"methods":["snapshotSaved"]}'

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
assert "B13: a narrowed retry announces scope=suites" \
    bash -c "grep -qE '^@@REIFY_GUI_FLAKE@@ .*outcome=retried .*scope=suites' '$FIX_OUT'"

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
assert "B13: a failing narrowed retry still names its scope=suites" \
    bash -c "grep -qE '^@@REIFY_GUI_FLAKE@@ .*outcome=escalated .*scope=suites' '$FIX_OUT'"

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
hostile_case "a non-string suite entry"              '[17]'

# THE NEW VALIDATION HOLE (task 7724). An empty ARRAY is now a legitimate
# verdict meaning "nothing to narrow to", and the reporter->runner seam is
# newline-delimited -- so `[""].join("\n")` is byte-identical to `[].join("\n")`
# and `["a.ts",""]` loses its tail to command substitution. An empty STRING must
# therefore be rejected where the array is still STRUCTURED, in the node parser:
# downstream it is indistinguishable from the thing that now means "retry
# everything".
hostile_case "an empty-string suite entry"             '[""]'
hostile_case "an empty-string entry beside a real one" '["src/__tests__/a.test.ts",""]'
assert "B14: an empty-string entry is rejected LOUDLY, never promoted to a full retry" \
    bash -c "grep -qi 'WARNING' '$FIX_OUT'"

# ...and EMPTINESS is not the only way into that collapse. Measured against the
# parser as it stood: `["\n"]` survives a length check, joins to "\n", and loses
# it to command substitution -- leaving the same empty string `[]` produces, so a
# REJECTED artifact was promoted to a full re-run of the caller's invocation. An
# EMBEDDED newline collapses the other way: `["a.ts\nb.ts"]` is ONE malformed
# token that mapfile silently splits into two specs. Both are closed by rejecting
# WHITESPACE in the parser -- not a new rule, only is_safe_spec's existing
# character class moved upstream to the one place these are still distinguishable
# from a genuinely empty array.
hostile_case "a whitespace-only suite entry (newline)" '["\n"]'
hostile_case "a whitespace-only suite entry (space)"   '[" "]'
hostile_case "an entry with an EMBEDDED newline"       '["src/__tests__/a.test.ts\nsrc/__tests__/b.test.ts"]'
assert "B14: a whitespace-only entry is rejected LOUDLY too, never read as an empty array" \
    bash -c "grep -qi 'WARNING' '$FIX_OUT'"

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

# (8) A retry must answer the question that was ASKED. scripts/gui-test.sh
# advertises `-- <vitest args>`, so dropping `-t <name>`, `--coverage` or `-u`
# on the retry would report success for a different run than the requested one
# -- and a dropped VALUE would leave a dangling `-t` that swallowed a suite
# path. Options (and the token following one) carry through; only the
# positional spec filters are replaced by the classified suites.
fixture_run "$TWO_SUITES" 1 0 -- -t someName --coverage
assert "B8: the retry carries the caller's options and their values through" \
    bash -c "[ \"\$(sed -n 2p '$FIX_ARGV')\" = 'test -- -t someName --coverage src/__tests__/engineStore.test.ts src/__tests__/meshManager.attributeResize.test.ts' ]"

# (9) ...and the positional filters really are REPLACED, not unioned, so the
# retry stays narrowed to what the reporter classified.
fixture_run "$TWO_SUITES" 1 0 -- src/__tests__/unitLadder.test.ts
assert "B9: the retry replaces the caller's positional spec filters" \
    bash -c "[ \"\$(sed -n 2p '$FIX_ARGV')\" = 'test -- src/__tests__/engineStore.test.ts src/__tests__/meshManager.attributeResize.test.ts' ]"

# (10) THE RUN-SCOPE VERDICT (task 7724). `snapshotSaved` is issued after a
# file's tests have already passed, so a timeout on it surfaces at run level
# with no module to attribute it to and the classifier names NO suite. That is
# not "nothing to do": the runner re-runs the caller's ORIGINAL invocation,
# which on the merge gate is the bare full suite. This is the hole esc-7600-1
# fell through -- the same fixture was previously asserted to be REJECTED.
fixture_run "$RUN_SCOPE" 1 0 --
assert "B10: a verdict naming NO suite + a passing retry => runner exits 0" \
    bash -c "[ \"$FIX_RC\" -eq 0 ]"
assert "B10: the run-scope retry is still bounded to ONE (exactly two npm invocations)" \
    bash -c "[ \"\$(npm_invocations)\" -eq 2 ]"
assert "B10: the retry is the caller's bare full-suite invocation" \
    bash -c "[ \"\$(sed -n 2p '$FIX_ARGV')\" = 'test' ]"
assert "B12: the run-scope retry announces scope=run both before and after" \
    bash -c "
        grep -qE '^@@REIFY_GUI_FLAKE@@ .*outcome=retrying .*scope=run' '$FIX_OUT' &&
        grep -qE '^@@REIFY_GUI_FLAKE@@ .*outcome=retried .*scope=run' '$FIX_OUT'
    "

# (11) ...and "the original invocation" means exactly that. Options, their
# values AND the caller's own positional filters all survive, because with no
# classified suites there is nothing to replace them with. A block narrowed by
# REIFY_GUI_RETRY_SPECS therefore retries its own narrowing rather than
# suddenly answering a wider question than the one that was asked.
fixture_run "$RUN_SCOPE" 1 0 -- -t someName --coverage src/__tests__/unitLadder.test.ts
assert "B11: the run-scope retry reproduces the caller's original argv verbatim" \
    bash -c "[ \"\$(sed -n 2p '$FIX_ARGV')\" = 'test -- -t someName --coverage src/__tests__/unitLadder.test.ts' ]"

# (12) A run-scope retry that also fails is announced and stopped, never looped.
fixture_run "$RUN_SCOPE" 1 1 --
assert "B12: a failing run-scope retry escalates at scope=run, never silently" \
    bash -c "grep -qE '^@@REIFY_GUI_FLAKE@@ .*outcome=escalated .*scope=run' '$FIX_OUT'"
assert "B12: a failing run-scope retry is still bounded to two invocations" \
    bash -c "[ \"\$(npm_invocations)\" -eq 2 ]"

# -- Section C: the wiring — one definition of how vitest is invoked ---------
# CLAUDE.md requires scripts/gui-test.sh and verify.sh's gui block to stay
# equivalent ("if the gui block's command shape changes materially, update this
# script to match"). Routing both through the one runner turns that convention
# into a structural property instead of a promise.
echo ""
echo "--- Section C: verify.sh and gui-test.sh both route through the runner ---"

GUI_TEST_SH="$REPO_ROOT/scripts/gui-test.sh"

assert "C1: verify.sh's gui_inner invokes scripts/gui-vitest-run.sh" \
    bash -c "grep -qE '^[[:space:]]*gui_inner\+?=.*gui-vitest-run\.sh' '$VERIFY_SH'"

assert "C1: verify.sh's gui_inner has NO bare 'npm test' leaf left" \
    bash -c "! grep -qE '^[[:space:]]*gui_inner\+?=.*npm test' '$VERIFY_SH'"

# Both arms must route through the runner: the validated REIFY_GUI_RETRY_SPECS
# subset AND the loud full-suite fallback. Missing either would split the
# command shape back in two.
assert "C1: BOTH gui_inner arms (retry subset and full fallback) use the runner" \
    bash -c "[ \"\$(grep -cE '^[[:space:]]*gui_inner\+=.*gui-vitest-run\.sh' '$VERIFY_SH')\" -eq 2 ]"

# The δ honest marker counts the validated specs at the single narrowing site;
# rewiring the leaf must not disturb that accounting.
assert "C1: the _RETRY_GUI_SUBSET_APPLIED accounting survives the rewiring" \
    grep -q '_RETRY_GUI_SUBSET_APPLIED=\${#_gui_retry_toks\[@\]}' "$VERIFY_SH"

assert "C1: the REIFY_GUI_RETRY_SPECS allowlist and leading-dash guard survive" \
    bash -c "grep -q 'A-Za-z0-9._' '$VERIFY_SH' && grep -q 'REIFY_GUI_RETRY_SPECS' '$VERIFY_SH'"

# The runner's is_safe_spec and verify.sh's REIFY_GUI_RETRY_SPECS check are a
# DELIBERATE duplicate (different inputs: one already-split token from JSON vs
# one space-separated string), but they must not DRIFT. Extract both character
# class literals and compare them, so widening either side alone reds here
# instead of silently opening a hole on one path only.
extract_char_class() {  # <file> <shell-var-name>
    grep -oE "\\\$\{$2//\[[^]]*\]/\}" "$1" | head -1 | sed -E 's/^.*\[([^]]*)\].*$/\1/'
}
_VERIFY_CLASS="$(extract_char_class "$VERIFY_SH" _gui_retry_specs || true)"
_RUNNER_CLASS="$(extract_char_class "$RUNNER" tok || true)"

assert "C5: both spec-token character classes are still locatable" \
    bash -c "[ -n '$_VERIFY_CLASS' ] && [ -n '$_RUNNER_CLASS' ]"

# verify.sh validates the whole space-separated string, so its class carries the
# one extra ' '. Strip it and the two must be identical.
_V="$_VERIFY_CLASS" _R="$_RUNNER_CLASS" \
assert "C5: verify.sh's spec allowlist is exactly the runner's plus a space" \
    bash -c '[ "${_V/ /}" = "$_R" ]'

assert "C2: scripts/gui-test.sh invokes the SAME runner" \
    grep -q 'gui-vitest-run.sh' "$GUI_TEST_SH"

assert "C2: scripts/gui-test.sh has NO bare 'npm test' invocation left" \
    bash -c "! grep -E '^[[:space:]]*(npm test|.*[^-]npm test )' '$GUI_TEST_SH' | grep -qv '^[[:space:]]*#'"

# (3) The block's wall-clock budget, kept at 15 minutes even though a retry is
# no longer always a narrowed one: at scope=run it re-runs the WHOLE suite, so
# the pair costs roughly TWICE the vitest phase rather than the few seconds a
# two-suite retry cost. The typical pair still fits the 900 s; at the worst
# RECORDED dilation it does not, and `timeout` kills the block.
#
# That overrun is ACCEPTED, not overlooked -- do not read this assertion as
# proof the budget is safe. The outcome at that dilation is red either way, and
# `outcome=retrying scope=run` is emitted BEFORE the retry starts, so such a
# block still explains itself instead of ending in an unexplained SIGKILL.
# Widening speculatively would only delay detection of a genuinely hung block,
# and REIFY_GUI_RPC_FLAKE_RETRY=0 remains the escape hatch.
#
# The measured basis lives in ONE place, deliberately not restated here where it
# would drift: docs/notes/verify-pipeline-knobs.md, the "bound this does NOT
# change" bullet under "GUI worker-RPC starvation marker & bounded retry".
assert "C3: the gui block keeps its 15-minute wrap_subshell budget" \
    grep -q 'wrap_subshell gui 15' "$VERIFY_SH"

# (4) An edit to the runner must route to the full --scope all gate, not the
# merge worker's config fast-path.
assert "C4: verify-pipeline-guard recognises the runner as load-bearing" \
    bash "$REPO_ROOT/scripts/verify-pipeline-guard.sh" requires-full-gate scripts/gui-vitest-run.sh

# -- Section D: end-to-end — real reporter, real runner, real vitest ---------
# Sections B and C mock the two halves separately; this one pins them as ONE
# contract. A throwaway vitest project holds two trivial specs, one of which
# throws the exact `[vitest-worker]: Timeout calling "fetch"` message at module
# load on its FIRST evaluation and passes thereafter -- reproducing the recorded
# signature (a failed suite, zero failed tests) without needing real starvation.
#
# node_modules is a REAL directory of symlinks to the installed packages rather
# than a symlink to the directory itself, so the artifact the reporter writes
# lands inside the fixture and never touches the repo's own gui/node_modules.
echo ""
echo "--- Section D: reporter -> artifact -> runner, end to end ---"

REAL_NODE_MODULES="$REPO_ROOT/gui/node_modules"

if [ ! -x "$REAL_NODE_MODULES/.bin/vitest" ]; then
    echo "  SKIP: Section D needs gui/node_modules (run 'scripts/gui-test.sh' or"
    echo "  SKIP: 'cd gui && npm ci' first). Sections A-C above cover the contract"
    echo "  SKIP: deterministically; this section adds the end-to-end pinning."
else
    E2E="$(mktemp -d "${TMPDIR:-/tmp}/reify-guie2e.XXXXXX")"
    mkdir -p "$E2E/scripts" "$E2E/gui/specs" "$E2E/gui/node_modules"
    cp "$RUNNER" "$E2E/scripts/gui-vitest-run.sh"
    chmod +x "$E2E/scripts/gui-vitest-run.sh"
    # The REAL reporter, byte-for-byte -- not a stand-in.
    cp "$REPO_ROOT/gui/vitest-worker-rpc-flake-reporter.ts" "$E2E/gui/"

    for _entry in "$REAL_NODE_MODULES"/*; do
        ln -sfn "$_entry" "$E2E/gui/node_modules/"
    done
    ln -sfn "$REAL_NODE_MODULES/.bin" "$E2E/gui/node_modules/.bin"

    cat > "$E2E/gui/package.json" <<'PKG'
{ "name": "e2e-fixture", "private": true, "type": "module",
  "scripts": { "test": "vitest run" } }
PKG

    cat > "$E2E/gui/vitest.config.ts" <<'CFG'
export default {
  test: {
    globals: true,
    include: ['specs/**/*.test.ts'],
    reporters: ['default', './vitest-worker-rpc-flake-reporter.ts'],
  },
}
CFG

    # Throws on its first evaluation only, so the retry finds it green -- the
    # transient-starvation shape, reproduced deterministically.
    cat > "$E2E/gui/specs/starved.test.ts" <<'SPEC'
import { existsSync, writeFileSync } from 'node:fs'
const seen = process.env.E2E_SEEN_FILE as string
if (!existsSync(seen)) {
  writeFileSync(seen, 'x')
  throw new Error('[vitest-worker]: Timeout calling "fetch" with "["/gui/vitest.setup.ts","web"]"')
}
it('passes once the host is responsive again', () => {
  expect(1).toBe(1)
})
SPEC

    cat > "$E2E/gui/specs/healthy.test.ts" <<'SPEC'
it('is unaffected by the starvation event', () => {
  expect(true).toBe(true)
})
SPEC

    E2E_ARTIFACT="$E2E/gui/node_modules/.reify-gui-rpc-flake.json"

    # Phase 1 -- vitest alone, so the reporter's two outputs can be inspected
    # before the runner consumes the artifact.
    E2E_SEEN="$E2E/state-phase1"
    p1_rc=0
    ( cd "$E2E/gui" && E2E_SEEN_FILE="$E2E_SEEN" npm test ) >"$E2E/phase1.log" 2>&1 || p1_rc=$?

    assert "D1: the starved run fails (a failed suite, zero failed tests)" \
        bash -c "[ '$p1_rc' -ne 0 ] && grep -q '1 failed' '$E2E/phase1.log'"

    assert "D2: the real reporter emits the marker at COLUMN 0" \
        grep -qE '^@@REIFY_GUI_FLAKE@@ kind=worker_rpc_timeout ' "$E2E/phase1.log"

    assert "D3: the marker carries the lineage" \
        grep -qE '^@@REIFY_GUI_FLAKE@@ .*lineage=3185,4856,7630' "$E2E/phase1.log"

    assert "D4: the real reporter writes the artifact" \
        test -f "$E2E_ARTIFACT"

    assert "D5: the artifact names ONLY the starved suite, gui-relative" \
        bash -c "[ \"\$(node -e 'process.stdout.write(JSON.parse(require(\"node:fs\").readFileSync(process.argv[1],\"utf8\")).suites.join(\",\"))' '$E2E_ARTIFACT')\" = 'specs/starved.test.ts' ]"

    # Phase 2 -- the runner over the same project from a clean slate: it must
    # see the same failure, read the same artifact, and retry just that suite.
    rm -f "$E2E_ARTIFACT"
    E2E_SEEN="$E2E/state-phase2"
    p2_rc=0
    E2E_SEEN_FILE="$E2E_SEEN" "$E2E/scripts/gui-vitest-run.sh" >"$E2E/phase2.log" 2>&1 || p2_rc=$?

    assert "D6: the runner's retry rescues the run (final exit 0)" \
        bash -c "[ '$p2_rc' -eq 0 ]"

    assert "D7: the runner announces the retry AND its outcome" \
        bash -c "
            grep -qE '^@@REIFY_GUI_FLAKE@@ .*outcome=retrying' '$E2E/phase2.log' &&
            grep -qE '^@@REIFY_GUI_FLAKE@@ .*outcome=retried' '$E2E/phase2.log'
        "

    # The log holds both runs; the FIRST summarises 2 files, the retry 1. That
    # last summary is the direct evidence the healthy suite was not re-run.
    assert "D8: the retry ran ONLY the starved suite, not the healthy one" \
        bash -c "grep 'Test Files' '$E2E/phase2.log' | tail -n1 | grep -q '1 passed (1)'"

    assert "D9: the rescued run leaves no artifact behind for the next run" \
        bash -c "[ ! -f '$E2E_ARTIFACT' ]"

    # A genuine failure over the SAME project must not be retried: the spec
    # below fails a TEST, which vetoes the classification outright.
    cat > "$E2E/gui/specs/starved.test.ts" <<'SPEC'
it('is a real defect', () => {
  expect(1).toBe(2)
})
SPEC
    p3_rc=0
    E2E_SEEN_FILE="$E2E/state-phase3" "$E2E/scripts/gui-vitest-run.sh" >"$E2E/phase3.log" 2>&1 || p3_rc=$?

    assert "D10: a genuine test failure propagates, with no marker and no retry" \
        bash -c "
            [ '$p3_rc' -ne 0 ] &&
            ! grep -q '@@REIFY_GUI_FLAKE@@' '$E2E/phase3.log' &&
            [ ! -f '$E2E_ARTIFACT' ]
        "

    # Phase 5 -- THE RUN-SCOPE SHAPE (task 7724), end to end. Every test PASSES
    # and the only failure is an unhandled error raised from a timer, so it
    # lands at RUN level with no module to attribute it to: the recorded
    # esc-7600-1 signature. specs/healthy.test.ts is untouched from Phase 1.
    #
    # This is also the ONE assertion in the whole suite that drives vitest's
    # REAL SerializedError through the classifier. Everywhere else the
    # classifier is fed synthetic `{message}` objects, so only here can the
    # printer's `Error: ` prefix be shown NOT to be part of `.message` -- the
    # premise the anchored RPC_TIMEOUT_MESSAGE regex rests on.
    cat > "$E2E/gui/specs/starved.test.ts" <<'SPEC'
import { existsSync, writeFileSync } from 'node:fs'
const seen = process.env.E2E_SEEN_FILE as string
it('passes while the host stalls on a post-test RPC', async () => {
  if (!existsSync(seen)) {
    writeFileSync(seen, 'x')
    // Thrown from a timer, so it is an UNHANDLED error at run level rather
    // than this test's failure -- which is exactly how snapshotSaved behaves:
    // it is issued after the file's tests have already passed.
    setTimeout(() => {
      throw new Error('[vitest-worker]: Timeout calling "snapshotSaved"')
    }, 0)
    await new Promise((resolve) => setTimeout(resolve, 50))
  }
  expect(1).toBe(1)
})
SPEC

    E2E_SEEN="$E2E/state-phase5"
    rm -f "$E2E_ARTIFACT"
    p5_rc=0
    ( cd "$E2E/gui" && E2E_SEEN_FILE="$E2E_SEEN" npm test ) >"$E2E/phase5.log" 2>&1 || p5_rc=$?

    assert "D11: the run-scope event fails the run with ZERO failed suites" \
        bash -c "
            [ '$p5_rc' -ne 0 ] &&
            grep -q 'Test Files.*2 passed (2)' '$E2E/phase5.log' &&
            ! grep -q 'Test Files.*failed' '$E2E/phase5.log' &&
            grep -q 'Errors' '$E2E/phase5.log'
        "

    assert "D12: the real reporter classifies it at RUN scope, at column 0" \
        grep -qE '^@@REIFY_GUI_FLAKE@@ kind=worker_rpc_timeout scope=run ' "$E2E/phase5.log"

    assert "D12: the run-scope marker carries suites=0, the method and the lineage" \
        grep -qE '^@@REIFY_GUI_FLAKE@@ .*suites=0 methods=snapshotSaved lineage=3185,4856,7630' "$E2E/phase5.log"

    assert "D13: the real artifact names NO suite -- an EMPTY array, not an absent key" \
        bash -c "[ \"\$(node -e 'const s = JSON.parse(require(\"node:fs\").readFileSync(process.argv[1], \"utf8\")).suites; process.stdout.write(Array.isArray(s) ? String(s.length) : \"not-an-array\")' '$E2E_ARTIFACT')\" = '0' ]"

    # Phase 6 -- the runner over the same run-scope project from a clean slate.
    rm -f "$E2E_ARTIFACT"
    p6_rc=0
    E2E_SEEN_FILE="$E2E/state-phase6" "$E2E/scripts/gui-vitest-run.sh" >"$E2E/phase6.log" 2>&1 || p6_rc=$?

    assert "D14: the runner rescues the run-scope flake (final exit 0)" \
        bash -c "[ '$p6_rc' -eq 0 ]"

    assert "D14: the run-scope retry is announced before AND after" \
        bash -c "
            grep -qE '^@@REIFY_GUI_FLAKE@@ .*outcome=retrying .*scope=run' '$E2E/phase6.log' &&
            grep -qE '^@@REIFY_GUI_FLAKE@@ .*outcome=retried .*scope=run' '$E2E/phase6.log'
        "

    # The log holds both runs; with no suite to narrow to, the retry must re-run
    # the WHOLE project -- so the LAST summary still counts both files.
    assert "D14: the retry re-ran the whole project, not a narrowed subset" \
        bash -c "grep 'Test Files' '$E2E/phase6.log' | tail -n1 | grep -q '2 passed (2)'"

    assert "D14: the rescued run-scope run leaves no artifact behind" \
        bash -c "[ ! -f '$E2E_ARTIFACT' ]"

    rm -rf "$E2E"
fi

test_summary
