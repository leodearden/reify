#!/usr/bin/env bash
#
# Tree-sitter Pipeline Infrastructure Tests
#
# Validates the tree-sitter parser auto-generation pipeline:
#   1. build.rs auto-generation — deletes parser.c and verifies cargo check
#      regenerates it via the needs_generate -> run_tree_sitter_generate path.
#   2. scripts/tree-sitter-generate.sh — positive and negative tests for the
#      standalone generation script.
#   3. Infrastructure checks — .gitignore, git tracking, orchestrator config,
#      hooks, and install guidance.
#
# Assert helpers capture full stdout/stderr on failure for diagnostics.
# File state is managed via backup/restore with trap-based cleanup.
#
set -euo pipefail

# Ensure Cargo-installed tools (e.g. tree-sitter-cli) are on PATH.
# Mirrors the '. ~/.cargo/env' prefix used in dark-factory-orchestrator.yaml verify commands.
[ -f "${HOME:-~}/.cargo/env" ] && . "${HOME:-~}/.cargo/env" || true

# --- Paths ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# plan_capture_lib.sh — retrying, completeness-guarded --print-plan capture
# (capture_print_plan / plan_capture_complete). Same fork-free, retry-on-truncation
# pattern already used by test_verify_scope.sh, test_scope_boundary.sh,
# test_run_all_tiering.sh, test_verify_throughput.sh.
[ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found at $SCRIPT_DIR/plan_capture_lib.sh"; exit 1; }
source "$SCRIPT_DIR/plan_capture_lib.sh"

TS_DIR="$REPO_ROOT/tree-sitter-reify"

# The out-of-build freshness guard (task #5629 / esc-5392-1).  A build script that
# cargo has DECLINED to re-run cannot repair itself, so the staleness check and the
# rebuild force must both live outside build.rs, in a plan leaf verify.sh runs
# before any cargo leaf.
FRESHNESS_SCRIPT="$REPO_ROOT/scripts/tree-sitter-freshness.sh"

# --- Counters ---
PASS_COUNT=0
FAIL_COUNT=0

# --- Colors ---
if [[ -t 1 ]]; then
    GREEN='\033[0;32m'
    RED='\033[0;31m'
    BOLD='\033[1m'
    RESET='\033[0m'
else
    GREEN=''
    RED=''
    BOLD=''
    RESET=''
fi

# --- Cleanup ---
CLEANUP_ACTIONS=()

cleanup() {
    for action in "${CLEANUP_ACTIONS[@]:-}"; do
        eval "$action" 2>/dev/null || true
    done
    CLEANUP_ACTIONS=()
}

trap cleanup EXIT

# --- Assert Helpers ---
# All helpers capture stdout+stderr to a temp file. On assertion failure the
# full captured output is printed for diagnostics. Nothing is sent to /dev/null.

assert_cmd_success() {
    # Usage: assert_cmd_success <description> <cmd> [args...]
    local desc="$1"; shift
    local tmpfile
    tmpfile=$(mktemp)
    CLEANUP_ACTIONS+=("rm -f '$tmpfile'")

    if "$@" >"$tmpfile" 2>&1; then
        return 0
    else
        local rc=$?
        echo ""
        echo "  ASSERTION FAILED: expected success for: $desc"
        echo "  Command: $*"
        echo "  Exit code: $rc"
        echo "  --- captured output ---"
        cat "$tmpfile"
        echo "  --- end output ---"
        return 1
    fi
}

assert_cmd_fails() {
    # Usage: assert_cmd_fails <description> <cmd> [args...]
    local desc="$1"; shift
    local tmpfile
    tmpfile=$(mktemp)
    CLEANUP_ACTIONS+=("rm -f '$tmpfile'")

    if "$@" >"$tmpfile" 2>&1; then
        echo ""
        echo "  ASSERTION FAILED: expected failure for: $desc"
        echo "  Command: $*"
        echo "  Exit code: 0 (success)"
        echo "  --- captured output ---"
        cat "$tmpfile"
        echo "  --- end output ---"
        return 1
    else
        return 0
    fi
}

assert_file_exists() {
    # Usage: assert_file_exists <path>
    if [[ ! -f "$1" ]]; then
        echo ""
        echo "  ASSERTION FAILED: file does not exist: $1"
        return 1
    fi
}

assert_file_nonempty() {
    # Usage: assert_file_nonempty <path>
    if [[ ! -s "$1" ]]; then
        echo ""
        echo "  ASSERTION FAILED: file is empty or missing: $1"
        return 1
    fi
}

require_tree_sitter_cli() {
    # Usage: require_tree_sitter_cli || return 0
    # SKIPs (not fails) when the tree-sitter CLI is not on PATH. The CLI is
    # an optional dev dependency (cargo install tree-sitter-cli); tests that
    # need to invoke it directly must degrade gracefully rather than report
    # a false FAIL for an environment gap. Shared by
    # test_auto_generation_rebuilds_parser and test_generate_script_succeeds_normally.
    if ! command -v tree-sitter >/dev/null 2>&1; then
        echo "  SKIP: tree-sitter CLI not on PATH (install via: cargo install tree-sitter-cli)"
        return 1
    fi
}

# --- Guard Helper ---
# run_guarded_cargo_check <out_file> <cmd...>
# Runs <cmd...>, capturing combined stdout+stderr to <out_file>.
# Returns a tri-state code safe under `set -euo pipefail`:
#   0 — success     (caller continues to parser.c existence checks)
#   1 — hard fail   (diagnostic already printed; caller returns 1)
#   2 — timeout     (SKIP message printed; caller returns 0 to skip asserts)
#
# Uses `|| rc=$?` to capture cmd's GENUINE exit code, shielding it from
# `set -e` (the established codebase idiom; see test_portable_timeout.sh:212).
run_guarded_cargo_check() {
    local out_file="$1"; shift
    local rc=0
    "$@" >"$out_file" 2>&1 || rc=$?
    if [ "$rc" -eq 0 ]; then
        return 0
    elif [ "$rc" -eq 124 ]; then
        echo "  SKIP: cargo check timed out after 300 s (cold/contended-cache environment)"
        return 2
    else
        echo ""
        echo "  ASSERTION FAILED: cargo check failed (exit $rc)"
        echo "  --- captured output ---"
        cat "$out_file"
        echo "  --- end output ---"
        return 1
    fi
}

# --- Freshness-guard Fixture Helpers ---
# Deliberately NOT named test_* — run_tests discovers by matching 'test_' against
# the whole `declare -F` line, so any helper carrying that substring would be run
# as a test case.

# mk_ts_fixture <target_dir> <fingerprint_dir_name> <stamp_content|__none__>
#
# Builds a fake cargo build tree — <target_dir>/debug/build/<name>/out/ holding a
# dummy libtree_sitter_reify.a and, unless __none__, the sibling
# tree_sitter_inputs.stamp build.rs would have written next to it. Entirely
# hermetic: the real target/ is never read or written.
mk_ts_fixture() {
    local target="$1" name="$2" stamp="$3"
    local out="$target/debug/build/$name/out"
    mkdir -p "$out"
    printf 'not a real archive\n' > "$out/libtree_sitter_reify.a"
    if [ "$stamp" != "__none__" ]; then
        printf '%s\n' "$stamp" > "$out/tree_sitter_inputs.stamp"
    fi
}

# run_ts_freshness <mode> <target_dir> [ts_dir]
# Sets TS_FRESHNESS_OUT (combined stdout+stderr) and TS_FRESHNESS_RC.
run_ts_freshness() {
    local mode="$1" target="$2" ts="${3:-}"
    TS_FRESHNESS_RC=0
    if [ -n "$ts" ]; then
        TS_FRESHNESS_OUT=$(REIFY_TS_FRESHNESS_TARGET_DIR="$target" \
            REIFY_TS_FRESHNESS_TS_DIR="$ts" \
            bash "$FRESHNESS_SCRIPT" "$mode" 2>&1) || TS_FRESHNESS_RC=$?
    else
        TS_FRESHNESS_OUT=$(REIFY_TS_FRESHNESS_TARGET_DIR="$target" \
            bash "$FRESHNESS_SCRIPT" "$mode" 2>&1) || TS_FRESHNESS_RC=$?
    fi
}

# --- Runner ---
run_tests() {
    local tests
    tests=$(declare -F | awk '/test_/{print $3}')

    for t in $tests; do
        CLEANUP_ACTIONS=()
        printf "${BOLD}RUN${RESET}  %s ... " "$t"
        if "$t"; then
            printf "${GREEN}PASS${RESET}\n"
            (( PASS_COUNT++ )) || true
        else
            printf "${RED}FAIL${RESET}\n"
            (( FAIL_COUNT++ )) || true
        fi
        cleanup
    done

    echo ""
    echo "========================================="
    printf "Results: ${GREEN}%d passed${RESET}, ${RED}%d failed${RESET}\n" "$PASS_COUNT" "$FAIL_COUNT"
    echo "========================================="

    [[ "$FAIL_COUNT" -eq 0 ]]
}

# --- Test Cases ---

test_auto_generation_rebuilds_parser() {
    # Validates that build.rs auto-regenerates parser.c when it is missing.
    # This exercises the needs_generate -> run_tree_sitter_generate path.
    local parser="$TS_DIR/src/parser.c"
    local backup="$TS_DIR/src/parser.c.bak"

    # This test deletes parser.c below and relies on `cargo check` to
    # regenerate it; build.rs's run_tree_sitter_generate() spawns the
    # 'tree-sitter' binary directly and panics if it's missing, which
    # run_guarded_cargo_check reports as a hard FAIL rather than a SKIP. That
    # applies whether or not parser.c happens to exist right now (e.g. left
    # over from a prior run), so check for the CLI first, unconditionally,
    # rather than only in the self-provisioning branch below.
    require_tree_sitter_cli || return 0

    # parser.c is a gitignored generated artifact. In a freshly-seeded warm
    # lane (tracked-files-only + git clean -xfd) it does not exist until
    # something generates it — under scripts/verify.sh that's always the
    # tree-sitter-generate.sh plan leaf running first, but a standalone
    # invocation of this test file has no such guarantee. Self-provision it
    # here rather than assume it, so the failure mode (if any) is a legible
    # assertion instead of a bare `cp: cannot stat`.
    if [ ! -f "$parser" ]; then
        local gen_out
        gen_out=$(mktemp)
        CLEANUP_ACTIONS+=("rm -f '$gen_out'")
        "$REPO_ROOT/scripts/tree-sitter-generate.sh" >"$gen_out" 2>&1 || true
        if [ ! -f "$parser" ]; then
            echo ""
            echo "  ASSERTION FAILED: parser.c does not exist and scripts/tree-sitter-generate.sh did not produce it"
            echo "  --- captured output ---"
            cat "$gen_out"
            echo "  --- end output ---"
            return 1
        fi
    fi

    # Backup original parser.c
    cp "$parser" "$backup"
    CLEANUP_ACTIONS+=("mv -f '$backup' '$parser'")

    # Delete parser.c to trigger auto-regeneration
    rm -f "$parser"

    # Remove any stamp files from target dirs so staleness check triggers
    find "$REPO_ROOT/target" -name "grammar_hash.stamp" -delete 2>/dev/null || true

    # Touch grammar.js to ensure cargo re-runs build.rs (it uses rerun-if-changed=grammar.js).
    # Without this, cargo may skip build.rs entirely from cache even with parser.c missing.
    touch "$TS_DIR/grammar.js"

    # Run cargo check — build.rs should detect missing parser.c and regenerate.
    # Bound to 300 s to avoid consuming the entire 20-min run_all.sh budget on a
    # cold cache.  parser.c is ~5 MB; C compilation can take several minutes when
    # sccache is cold.  On a warm cache this completes in seconds.  Skip
    # gracefully on timeout (exit 124) so the rest of the suite still runs.
    local cargo_out
    cargo_out=$(mktemp)
    CLEANUP_ACTIONS+=("rm -f '$cargo_out'")
    local guard_rc=0
    run_guarded_cargo_check "$cargo_out" timeout 300 cargo check -p tree-sitter-reify \
            --manifest-path "$REPO_ROOT/Cargo.toml" || guard_rc=$?
    if [ "$guard_rc" -eq 2 ]; then return 0; fi   # timed out — SKIP (message already printed)
    if [ "$guard_rc" -ne 0 ]; then return 1; fi    # hard fail — message already printed

    # Verify parser.c was recreated
    assert_file_exists "$parser" || return 1
    assert_file_nonempty "$parser" || return 1
}

test_build_rs_no_piped_stdout_deadlock() {
    # Regression guard for the deadlock_pipe_buffer issue: build.rs must NOT
    # use Stdio::piped() for child stdout. The run_with_timeout loop drains
    # the pipe only AFTER try_wait() returns Some(status). If the child writes
    # more than 64KB to stdout, the OS pipe buffer fills, the child blocks on
    # write(), and try_wait() never returns Some — hard deadlock until the
    # 60-second timeout kills the process.
    #
    # Fix: use Stdio::null() instead. tree-sitter generate writes diagnostics
    # to stderr (already inherited), so discarding stdout loses nothing.
    local build_rs="$TS_DIR/build.rs"

    assert_file_exists "$build_rs" || return 1

    # Grep for lines that set .stdout(... Stdio::piped() ...). If found, FAIL.
    if grep -E '\.stdout\(.*Stdio::piped\(\)' "$build_rs" >/dev/null 2>&1; then
        echo ""
        echo "  ASSERTION FAILED: build.rs uses Stdio::piped() for child stdout"
        echo ""
        echo "  This creates a deadlock in run_with_timeout(): the parent only"
        echo "  drains the stdout pipe AFTER try_wait() returns Some(status), but"
        echo "  if the child writes >64KB to stdout, the pipe buffer fills, the"
        echo "  child blocks on write, and try_wait() returns Ok(None) forever."
        echo "  The deadlock persists until the 60-second timeout kills the process,"
        echo "  making every build take a full minute."
        echo ""
        echo "  Use .stdout(Stdio::null()) instead. tree-sitter generate writes"
        echo "  useful diagnostics to stderr (already inherited via Stdio::inherit())."
        return 1
    else
        return 0
    fi
}

test_build_rs_no_stdout_inheritance() {
    # Regression guard for the stdio_pollution issue: build.rs must NOT let
    # child processes inherit stdout, because Cargo parses build-script stdout
    # line-by-line for "cargo:" directives. If tree-sitter CLI emits anything
    # to stdout (e.g. structured output), Cargo would misinterpret it.
    #
    # The fix: run_with_timeout must explicitly set .stdout(Stdio::piped())
    # or .stdout(Stdio::null()) so child stdout is NOT inherited.
    local build_rs="$TS_DIR/build.rs"

    assert_file_exists "$build_rs" || return 1

    # Grep for .stdout(Stdio:: or .stdout(std::process::Stdio:: in the Command
    # builder within build.rs. This catches accidental removal of the redirect.
    if grep -qE '\.stdout\(.*Stdio::' "$build_rs"; then
        return 0
    else
        echo ""
        echo "  ASSERTION FAILED: build.rs does not configure child stdout"
        echo "  build.rs must use .stdout(Stdio::piped()) or .stdout(Stdio::null())"
        echo "  on the Command builder in run_with_timeout() to prevent child"
        echo "  processes from inheriting build-script stdout, which Cargo parses"
        echo "  for 'cargo:' directives."
        echo ""
        echo "  See: https://doc.rust-lang.org/cargo/reference/build-scripts.html"
        return 1
    fi
}

test_generate_script_exists_and_executable() {
    # Validates the generation script exists and has execute permission.
    assert_file_exists "$REPO_ROOT/scripts/tree-sitter-generate.sh" || return 1
    if [[ ! -x "$REPO_ROOT/scripts/tree-sitter-generate.sh" ]]; then
        echo ""
        echo "  ASSERTION FAILED: scripts/tree-sitter-generate.sh is not executable"
        return 1
    fi
}

test_generate_script_fails_without_grammar() {
    # Negative test: tree-sitter-generate.sh must exit non-zero when
    # grammar.js is missing, with a clear error message.
    local grammar="$TS_DIR/grammar.js"
    local backup="$TS_DIR/grammar.js.bak"

    # Move grammar.js away
    mv "$grammar" "$backup"
    CLEANUP_ACTIONS+=("mv -f '$backup' '$grammar'")

    # The script should fail
    assert_cmd_fails "generate script fails without grammar.js" \
        "$REPO_ROOT/scripts/tree-sitter-generate.sh" || return 1
}

test_generate_script_succeeds_normally() {
    # Positive baseline: tree-sitter-generate.sh should succeed when
    # grammar.js is present, producing all expected output files.
    # Skip gracefully when tree-sitter CLI is not installed in this environment.
    require_tree_sitter_cli || return 0
    assert_cmd_success "generate script succeeds with grammar.js present" \
        "$REPO_ROOT/scripts/tree-sitter-generate.sh" || return 1

    # Verify expected output files exist
    assert_file_exists "$TS_DIR/src/parser.c" || return 1
    assert_file_exists "$TS_DIR/src/grammar.json" || return 1
    assert_file_exists "$TS_DIR/src/node-types.json" || return 1
}

test_gitignore_excludes_generated_files() {
    # .gitignore must list all tree-sitter generated files.
    local gitignore="$REPO_ROOT/.gitignore"
    assert_file_exists "$gitignore" || return 1

    for f in "tree-sitter-reify/src/parser.c" "tree-sitter-reify/src/grammar.json" "tree-sitter-reify/src/node-types.json"; do
        if ! grep -qF "$f" "$gitignore"; then
            echo ""
            echo "  ASSERTION FAILED: .gitignore does not contain $f"
            return 1
        fi
    done
}

test_generated_files_not_tracked() {
    # Generated files must NOT be tracked by git.
    for f in "tree-sitter-reify/src/parser.c" "tree-sitter-reify/src/grammar.json" "tree-sitter-reify/src/node-types.json"; do
        if [ -n "$(cd "$REPO_ROOT" && git ls-files "$f")" ]; then
            echo ""
            echo "  ASSERTION FAILED: $f is tracked by git (should be gitignored)"
            return 1
        fi
    done
}

test_orchestrator_includes_generation() {
    # Since task 3766 the orchestrator runs scripts/verify.sh, so each verify
    # action's plan (not dark-factory-orchestrator.yaml literals) must include tree-sitter
    # generation. --scope all forces the full plan; env lines are '# ' comments.
    local verify="$REPO_ROOT/scripts/verify.sh"
    assert_file_exists "$verify" || return 1

    local action plan
    for action in "test --profile both --scope all --include-infra" \
                  "lint --scope all --include-infra" \
                  "typecheck --scope all"; do
        # Retrying, completeness-guarded capture. A truncated/interrupted --print-plan
        # capture under concurrent load would silently flip the tree-sitter-generate
        # check below to a spurious FAIL; capture_print_plan retries to a structurally
        # complete capture. $action word-splits into flags inside the inner bash -c
        # (its unquoted $2), so no outer SC2086 exposure.
        capture_print_plan plan "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
            bash -c 'exec bash "$1" $2 --print-plan 2>/dev/null' _ "$verify" "$action" || true
        if ! plan_capture_complete "$plan"; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' --print-plan capture truncated after retries"
            return 1
        fi
        # Match the RAW completeness-guarded capture with fork-free bash-native
        # substring matching (no pipe-to-grep EINTR surface — esc-4574-42 / esc-4707-64).
        # tree-sitter-generate is emitted only as a command leaf (verify.sh:1283), never
        # in a preamble comment, so the raw match cannot false-positive on a marker/env line.
        if [[ "$plan" != *"tree-sitter-generate"* ]]; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' plan does not include tree-sitter-generate"
            return 1
        fi
    done
}

test_hooks_include_generation() {
    # The main-branch git hook runs `verify.sh all`; that plan (not the hook
    # file's literals) must include tree-sitter generation.
    local verify="$REPO_ROOT/scripts/verify.sh"
    assert_file_exists "$verify" || return 1

    local plan
    # Retrying, completeness-guarded capture (same rationale/pattern as
    # test_orchestrator_includes_generation). The fixed action string word-splits
    # into flags inside the inner bash -c (its unquoted $2).
    capture_print_plan plan "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
        bash -c 'exec bash "$1" $2 --print-plan 2>/dev/null' _ "$verify" "all --profile debug --scope all --include-infra" || true
    if ! plan_capture_complete "$plan"; then
        echo ""
        echo "  ASSERTION FAILED: verify.sh 'all' --print-plan capture truncated after retries"
        return 1
    fi
    # Fork-free bash-native match on the RAW completeness-guarded capture (esc-4574-42 /
    # esc-4707-64); tree-sitter-generate is emitted only as a command leaf (verify.sh:1283).
    if [[ "$plan" != *"tree-sitter-generate"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: verify.sh 'all' plan (the hook's gate) does not include tree-sitter-generate"
        return 1
    fi
}

test_print_plan_capture_retries_on_truncation() {
    # ITEM 1 (task 5260): prove capture_print_plan retries a truncated/SIGPIPE-class
    # --print-plan capture to completion. A deterministic fake emitter, backed by an
    # mktemp counter file, returns a TRUNCATED dump (only the '# verify.sh plan'
    # header, missing the '# --- commands' marker) on invocation #1 and a COMPLETE
    # dump (both structural markers verify.sh unconditionally emits) on invocation
    # #2+, so capture_print_plan must retry past the truncated first attempt.
    #
    # RED before plan_capture_lib.sh is sourced at file-top (capture_print_plan is
    # undefined, so the capture call fails and this test FAILS); GREEN once step-2
    # sources the lib.
    local counter
    counter=$(mktemp)
    CLEANUP_ACTIONS+=("rm -f '$counter'")
    printf '0' > "$counter"

    # Deterministic fake plan emitter (global once defined; inherited by the
    # command-substitution subshell capture_print_plan runs it in). The counter
    # file path is passed as an explicit arg — no reliance on dynamic-scope leak.
    _fake_plan_emit() {
        local _cf="$1" _n
        _n=$(cat "$_cf")
        _n=$((_n + 1))
        printf '%s' "$_n" > "$_cf"
        echo "# verify.sh plan"
        if [ "$_n" -ge 2 ]; then
            echo "./scripts/tree-sitter-generate.sh"
            echo "# --- commands"
        fi
    }

    local out="" rc=0
    capture_print_plan out 3 _fake_plan_emit "$counter" || rc=$?

    if [ "$rc" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: capture_print_plan returned $rc (expected 0 — retry to a complete capture)"
        return 1
    fi
    if ! plan_capture_complete "$out"; then
        echo ""
        echo "  ASSERTION FAILED: capture is not complete after retries: <<<$out>>>"
        return 1
    fi
    if [[ "$out" != *"# verify.sh plan"* ]] || [[ "$out" != *"# --- commands"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: capture missing a structural marker: <<<$out>>>"
        return 1
    fi
}

test_timeout_guard_skips_on_exit_124() {
    # Regression guard: confirms run_guarded_cargo_check returns tri-state 2
    # (SKIP) when the command exits 124 (timeout kill). Uses `timeout 0.1 sleep 5`
    # as a deterministic stub for exit 124.
    local out rc
    out=$(mktemp)
    CLEANUP_ACTIONS+=("rm -f '$out'")
    rc=0
    run_guarded_cargo_check "$out" timeout 0.1 sleep 5 || rc=$?
    if [ "$rc" -ne 2 ]; then
        echo ""
        echo "  ASSERTION FAILED: expected run_guarded_cargo_check to return 2 (SKIP) on exit 124, got $rc"
        return 1
    fi
}

test_timeout_guard_fails_on_other_exit() {
    # Regression guard: confirms run_guarded_cargo_check returns tri-state 1
    # (hard FAIL) when the command exits with a non-zero, non-124 code. Uses
    # `false` (exit 1) as stub; output suppressed so the helper's diagnostic
    # chatter does not pollute a passing run.
    local out rc
    out=$(mktemp)
    CLEANUP_ACTIONS+=("rm -f '$out'")
    rc=0
    run_guarded_cargo_check "$out" false >/dev/null 2>&1 || rc=$?
    if [ "$rc" -ne 1 ]; then
        echo ""
        echo "  ASSERTION FAILED: expected run_guarded_cargo_check to return 1 (FAIL) on exit 1, got $rc"
        return 1
    fi
}

test_timeout_guard_passes_on_exit_0() {
    # Regression guard: confirms run_guarded_cargo_check returns tri-state 0
    # (SUCCESS) when the command exits 0, meaning the caller continues to the
    # parser.c assertions. Uses `true` (exit 0) as stub.
    local out rc
    out=$(mktemp)
    CLEANUP_ACTIONS+=("rm -f '$out'")
    rc=0
    run_guarded_cargo_check "$out" true || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: expected run_guarded_cargo_check to return 0 (SUCCESS) on exit 0, got $rc"
        return 1
    fi
}

test_freshness_script_exists_and_executable() {
    # Task #5629 / esc-5392-1.  scripts/tree-sitter-freshness.sh is the guard that
    # proves the compiled libtree_sitter_reify.a was built from the bytes currently
    # on disk.  It must be a directly-runnable, strict-mode bash script because
    # verify.sh emits it as a plan leaf (`./scripts/tree-sitter-freshness.sh ensure`).
    assert_file_exists "$FRESHNESS_SCRIPT" || return 1

    if [[ ! -x "$FRESHNESS_SCRIPT" ]]; then
        echo ""
        echo "  ASSERTION FAILED: scripts/tree-sitter-freshness.sh is not executable"
        echo "  verify.sh emits it as a bare command leaf, so it must carry mode 0755."
        return 1
    fi

    assert_cmd_success "tree-sitter-freshness.sh parses under bash -n" \
        bash -n "$FRESHNESS_SCRIPT" || return 1

    local first_line
    first_line=$(head -n 1 "$FRESHNESS_SCRIPT")
    if [[ "$first_line" != "#!/usr/bin/env bash" ]]; then
        echo ""
        echo "  ASSERTION FAILED: line 1 is not '#!/usr/bin/env bash'"
        echo "  Got: $first_line"
        return 1
    fi

    if ! grep -qF 'set -euo pipefail' "$FRESHNESS_SCRIPT"; then
        echo ""
        echo "  ASSERTION FAILED: tree-sitter-freshness.sh does not 'set -euo pipefail'"
        echo "  A freshness guard that swallows an error would fail OPEN — exactly the"
        echo "  false-GREEN shape this task exists to close."
        return 1
    fi
}

test_freshness_script_lists_all_compiled_inputs() {
    # `--list-inputs` is the single source of truth for the C-compilation input set
    # (everything build.rs hands to cc::Build, plus the headers they include).
    #
    # The expectation is DERIVED from `git ls-files` rather than restated inline, so
    # that adding a fourth header — or a second .c to c_config — without wiring it
    # into the freshness input set fails loudly HERE instead of silently
    # under-covering.  That silent under-coverage is precisely how this defect class
    # (src/scanner.c watched by nothing) came to exist in the first place.
    assert_file_exists "$FRESHNESS_SCRIPT" || return 1

    # Run from a directory that is NOT the repo root: the script must resolve its
    # own paths from ${BASH_SOURCE[0]}, never from $PWD.  verify.sh invokes plan
    # leaves from the repo root, but the infra tests and a human debugging a merge
    # lane do not.
    local out rc=0
    out=$(cd /tmp && bash "$FRESHNESS_SCRIPT" --list-inputs 2>&1) || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: '--list-inputs' exited $rc (expected 0) when run from /tmp"
        echo "  --- captured output ---"
        echo "$out"
        echo "  --- end output ---"
        return 1
    fi

    local headers expected
    headers=$(cd "$REPO_ROOT" && git ls-files 'tree-sitter-reify/src/tree_sitter/*.h' \
        | sed 's|^tree-sitter-reify/||' | LC_ALL=C sort)
    if [ -z "$headers" ]; then
        echo ""
        echo "  ASSERTION FAILED: git ls-files found no tracked tree-sitter-reify/src/tree_sitter/*.h"
        echo "  The expectation is derived from git; an empty derivation would make this"
        echo "  test vacuous, so it is a hard failure rather than a skip."
        return 1
    fi
    # src/parser.c is generated+gitignored (hence not from git ls-files) but IS
    # compiled by c_config, so it belongs in the fingerprint even though it is
    # deliberately NOT in the rerun-if-changed watch set.
    expected=$(printf 'src/parser.c\nsrc/scanner.c\n%s\n' "$headers")

    if [ "$out" != "$expected" ]; then
        echo ""
        echo "  ASSERTION FAILED: '--list-inputs' does not match the compiled input set"
        echo "  --- expected ---"
        echo "$expected"
        echo "  --- actual ---"
        echo "$out"
        echo "  --- end ---"
        return 1
    fi

    # Sorted, so the manifest ordering is reproducible byte-for-byte between the
    # shell script and build.rs (step-7 asserts those two agree exactly).
    local sorted
    sorted=$(printf '%s\n' "$out" | LC_ALL=C sort)
    if [ "$out" != "$sorted" ]; then
        echo ""
        echo "  ASSERTION FAILED: '--list-inputs' output is not in LC_ALL=C sorted order"
        echo "  --- actual ---"
        echo "$out"
        echo "  --- end ---"
        return 1
    fi

    # No blank lines (a blank line would hash as a missing file downstream).
    if printf '%s\n' "$out" | grep -q '^[[:space:]]*$'; then
        echo ""
        echo "  ASSERTION FAILED: '--list-inputs' emitted a blank/whitespace-only line"
        return 1
    fi

    # No duplicates (a duplicated path would double-count in the fingerprint and
    # make the build.rs and shell manifests disagree).
    local total uniq
    total=$(printf '%s\n' "$out" | wc -l)
    uniq=$(printf '%s\n' "$out" | LC_ALL=C sort -u | wc -l)
    if [ "$total" -ne "$uniq" ]; then
        echo ""
        echo "  ASSERTION FAILED: '--list-inputs' emitted duplicate paths ($total lines, $uniq unique)"
        echo "$out"
        return 1
    fi
}

test_freshness_check_verdicts() {
    # Fully hermetic: fake cargo build trees under mktemp -d, driven via
    # REIFY_TS_FRESHNESS_TARGET_DIR. The real target/ is never touched, so this
    # test is safe to run concurrently inside a warm lane.
    assert_file_exists "$FRESHNESS_SCRIPT" || return 1

    local tmp
    tmp=$(mktemp -d)
    CLEANUP_ACTIONS+=("rm -rf '$tmp'")

    # ---- (1) --print-fingerprint shape (read-only against the real tree) ----
    local fp rc=0
    fp=$(bash "$FRESHNESS_SCRIPT" --print-fingerprint 2>&1) || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: '--print-fingerprint' exited $rc (expected 0)"
        echo "$fp"
        return 1
    fi

    local inputs
    inputs=$(bash "$FRESHNESS_SCRIPT" --list-inputs)

    # The relpath column must equal --list-inputs EXACTLY — same paths, same
    # order, same count. Anything else means the fingerprint does not cover the
    # declared input set.
    local fp_paths
    fp_paths=$(printf '%s\n' "$fp" | awk '{print $2}')
    if [ "$fp_paths" != "$inputs" ]; then
        echo ""
        echo "  ASSERTION FAILED: fingerprint path column != --list-inputs"
        echo "  --- --list-inputs ---"; echo "$inputs"
        echo "  --- fingerprint paths ---"; echo "$fp_paths"
        return 1
    fi

    # Every line must be '<64-hex-sha256><2 spaces><relpath>' — the exact shape
    # sha256sum/shasum emit, so build.rs can reproduce it byte-for-byte.
    # Matched with a bash-native regex, not `grep -q` in a pipe: `grep -q` exits
    # early and SIGPIPEs its writer, which under `set -o pipefail` yields 141 and
    # silently inverts the verdict.
    local line bad=""
    while IFS= read -r line; do
        [[ "$line" =~ ^[0-9a-f]{64}\ \ [^[:space:]]+$ ]] || bad="$line"
    done <<< "$fp"
    if [ -n "$bad" ]; then
        echo ""
        echo "  ASSERTION FAILED: fingerprint line is not '<64-hex>  <relpath>': <<<$bad>>>"
        return 1
    fi

    # ---- (2) stamp byte-identical to the fingerprint -> FRESH, exit 0 ----
    mk_ts_fixture "$tmp/t_fresh" "tree-sitter-reify-deadbeef01" "$fp"
    run_ts_freshness check "$tmp/t_fresh"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: check on a matching stamp exited $TS_FRESHNESS_RC (expected 0)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (3) altered stamp -> STALE, exit 1, NAMES the fingerprint dir ----
    # Deterministic single-hex-digit flip (never a no-op).
    local altered
    if [ "${fp:0:1}" = "0" ]; then altered="1${fp:1}"; else altered="0${fp:1}"; fi
    mk_ts_fixture "$tmp/t_altered" "tree-sitter-reify-deadbeef01" "$altered"
    run_ts_freshness check "$tmp/t_altered"
    if [ "$TS_FRESHNESS_RC" -ne 1 ]; then
        echo ""
        echo "  ASSERTION FAILED: check on an ALTERED stamp exited $TS_FRESHNESS_RC (expected 1)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"tree-sitter-reify-deadbeef01"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: stale verdict does not name the offending fingerprint dir"
        echo "  A verdict that does not say WHICH archive is stale is not actionable."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (4) stamp ABSENT next to an existing .a -> STALE, exit 1 ----
    # An unattested archive is UNPROVEN, never assumed fresh — the same
    # missing-sidecar-means-stale policy as scripts/reify-bin-freshness.sh.
    mk_ts_fixture "$tmp/t_nostamp" "tree-sitter-reify-deadbeef01" "__none__"
    run_ts_freshness check "$tmp/t_nostamp"
    if [ "$TS_FRESHNESS_RC" -ne 1 ]; then
        echo ""
        echo "  ASSERTION FAILED: check with NO stamp beside the archive exited $TS_FRESHNESS_RC (expected 1)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (5) stamp == 'UNAVAILABLE' -> SKIP, exit 0 ----
    # A host with neither sha256sum nor shasum cannot attest anything. Failing
    # there would be a permanent spurious RED; force-looping there would recompile
    # parser.c on every single run. Both are worse than a labelled skip.
    mk_ts_fixture "$tmp/t_unavail" "tree-sitter-reify-deadbeef01" "UNAVAILABLE"
    run_ts_freshness check "$tmp/t_unavail"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: check on an UNAVAILABLE stamp exited $TS_FRESHNESS_RC (expected 0)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"SKIP"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: UNAVAILABLE degradation is not labelled SKIP in the output"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (6) target dir with NO archive at all -> exit 0 ----
    # Nothing built means nothing can be stale.
    mkdir -p "$tmp/t_empty"
    run_ts_freshness check "$tmp/t_empty"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: check on an EMPTY target dir exited $TS_FRESHNESS_RC (expected 0)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (7) two fingerprint dirs, one fresh one stale -> exit 1, only the stale named ----
    # A real checkout carries many fingerprint dirs (7 in this lane, 9 in the main
    # checkout). check must scan them all and report precisely.
    mk_ts_fixture "$tmp/t_mixed" "tree-sitter-reify-aaaaaaaaaaaaaaaa" "$fp"
    mk_ts_fixture "$tmp/t_mixed" "tree-sitter-reify-bbbbbbbbbbbbbbbb" "$altered"
    run_ts_freshness check "$tmp/t_mixed"
    if [ "$TS_FRESHNESS_RC" -ne 1 ]; then
        echo ""
        echo "  ASSERTION FAILED: check with one stale of two dirs exited $TS_FRESHNESS_RC (expected 1)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"tree-sitter-reify-bbbbbbbbbbbbbbbb"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: the stale fingerprint dir (bbbb...) is not named"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" == *"tree-sitter-reify-aaaaaaaaaaaaaaaa"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: the FRESH fingerprint dir (aaaa...) is named as stale"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
}

# --- Main ---
run_tests
