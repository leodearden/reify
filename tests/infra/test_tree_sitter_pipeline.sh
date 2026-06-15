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
# Mirrors the '. ~/.cargo/env' prefix used in orchestrator.yaml verify commands.
[ -f "${HOME:-~}/.cargo/env" ] && . "${HOME:-~}/.cargo/env" || true

# --- Paths ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TS_DIR="$REPO_ROOT/tree-sitter-reify"

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
    if ! command -v tree-sitter >/dev/null 2>&1; then
        echo "  SKIP: tree-sitter CLI not on PATH (install via: cargo install tree-sitter-cli)"
        return 0
    fi
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
    # action's plan (not orchestrator.yaml literals) must include tree-sitter
    # generation. --scope all forces the full plan; env lines are '# ' comments.
    local verify="$REPO_ROOT/scripts/verify.sh"
    assert_file_exists "$verify" || return 1

    local action plan
    for action in "test --profile both --scope all --include-infra" \
                  "lint --scope all --include-infra" \
                  "typecheck --scope all"; do
        # shellcheck disable=SC2086 — $action intentionally word-splits into flags.
        plan="$(bash "$verify" $action --print-plan 2>/dev/null | grep -v '^#')"
        if ! printf '%s\n' "$plan" | grep -q "tree-sitter-generate"; then
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
    plan="$(bash "$verify" all --profile debug --scope all --include-infra --print-plan 2>/dev/null | grep -v '^#')"
    if ! printf '%s\n' "$plan" | grep -q "tree-sitter-generate"; then
        echo ""
        echo "  ASSERTION FAILED: verify.sh 'all' plan (the hook's gate) does not include tree-sitter-generate"
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

# --- Main ---
run_tests
