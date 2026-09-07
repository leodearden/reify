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

# The semaphore e2e suite stubs cargo and then RUNS verify.sh, so it drives the
# freshness guard's ensure/check leaves against a tree nothing ever compiles into.
# It isolates itself with REIFY_TS_FRESHNESS_TARGET_DIR; the two tests at the end
# of the freshness block below own that seam from this side.
SEMAPHORE_E2E_SUITE="$SCRIPT_DIR/test_verify_semaphore_e2e.sh"

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

provision_fixture_parser_c() {
    # Usage: provision_fixture_parser_c <dest-path> || return 1
    #
    # src/parser.c is a GITIGNORED generated artifact: in a freshly-seeded warm
    # lane (tracked-files-only + `git clean -xfd`) it does not exist until
    # something generates it. Under scripts/verify.sh it always does — the
    # tree-sitter-generate.sh plan leaf runs before any infra pole (measured:
    # generate at plan line 16, this file's pole at line 31 on a task-scope
    # plan) — but a STANDALONE run of this file has no such guarantee, and a
    # bare `cp` of it then dies with `cp: cannot stat`. That is the same
    # standalone-crash class task #6361 fixed for
    # test_auto_generation_rebuilds_parser; this is its hermetic-fixture
    # counterpart.
    #
    # The callers below build a fixture tree that is only ever FINGERPRINTED —
    # hashed and mtime-stamped, never compiled — so parser.c's CONTENT is
    # irrelevant to what they assert. A placeholder is therefore strictly
    # better than self-provisioning via the tree-sitter CLI: it is instant, it
    # adds no CLI dependency, and it cannot turn into a vacuous SKIP that hides
    # a real regression.
    local dest="$1"
    if [ -f "$TS_DIR/src/parser.c" ]; then
        cp "$TS_DIR/src/parser.c" "$dest" || return 1
    else
        printf '/* placeholder parser.c — fixture is fingerprinted, never compiled */\n' \
            > "$dest" || return 1
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

# mk_ts_fixture <target_dir> <fingerprint_dir_name> <stamp_content|__none__> [marker_mtime]
#
# Builds a fake cargo build tree — <target_dir>/debug/build/<name>/out/ holding a
# dummy libtree_sitter_reify.a and, unless __none__, the sibling
# tree_sitter_inputs.stamp build.rs would have written next to it. Entirely
# hermetic: the real target/ is never read or written.
#
# marker_mtime (optional) writes the `output` file cargo drops in the RUN dir
# when it executes the build script, stamped to that `touch -d` time spec. That
# file's mtime is how the script picks the LIVE fingerprint dir, so a fixture
# supplies it whenever the test needs a specific dir to win. Omit it and the dir
# carries no marker at all — which is what every pre-liveness fixture here does,
# and which the script reads as "liveness undeterminable, assert on everything".
mk_ts_fixture() {
    local target="$1" name="$2" stamp="$3" marker="${4:-}"
    local run="$target/debug/build/$name"
    local out="$run/out"
    mkdir -p "$out"
    printf 'not a real archive\n' > "$out/libtree_sitter_reify.a"
    if [ "$stamp" != "__none__" ]; then
        printf '%s\n' "$stamp" > "$out/tree_sitter_inputs.stamp"
    fi
    if [ -n "$marker" ]; then
        printf 'cargo:rerun-if-changed=grammar.js\n' > "$run/output"
        touch -d "$marker" "$run/output"
    fi
}

# ts_mtime <path> — modification time in epoch seconds (GNU stat, BSD fallback).
ts_mtime() {
    stat -c %Y "$1" 2>/dev/null || stat -f %m "$1"
}

# ts_watched_files <ts_dir>
#
# The EXACT set `ensure` is allowed to touch: grammar.js + src/scanner.c +
# src/tree_sitter/*.h — i.e. what build.rs declares via rerun-if-changed.
# src/parser.c is deliberately excluded: build.rs writes it, so watching it
# would cause double execution, and touching it would repair nothing.
ts_watched_files() {
    local ts="$1" h
    printf '%s\n' "$ts/grammar.js" "$ts/src/scanner.c"
    for h in "$ts"/src/tree_sitter/*.h; do
        [ -f "$h" ] && printf '%s\n' "$h"
    done
}

# ts_witness_value <ts_dir>
#
# The value a real `ensure` writes to the ledger's mtime WITNESS file
# (.tree-sitter-freshness.ledger.mtime): the highest mtime across the watched
# inputs at ledger-write time.
#
# A hand-seeded ledger must carry this too, or it is not the state a previous
# force leaves behind. The script bypasses a ledger whose witness is missing or
# whose value is ABOVE the current max — the warm-lane rewind signature, where
# seed-warm-lane.sh stamps sources back to 2020 while CoW-cloning target/ (and
# so the ledger) intact. A fixture seeding only the fingerprint would therefore
# exercise the bypass rather than whatever rule it meant to test.
ts_witness_value() {
    local d="$1" f m best=0
    while IFS= read -r f; do
        m=$(ts_mtime "$f")
        if [ "$m" -gt "$best" ]; then best="$m"; fi
    done < <(ts_watched_files "$d")
    printf '%s' "$best"
}

# ts_freshest_run_dir
#
# Print the most recently modified cargo build-script RUN directory for
# tree-sitter-reify — the one holding `output` (cargo's verbatim capture of
# build-script stdout) and the sibling `out/` with libtree_sitter_reify.a.
# Prints nothing when none exists.
ts_freshest_run_dir() {
    local d m best="" best_m=0
    local had_nullglob=0
    shopt -q nullglob && had_nullglob=1
    shopt -s nullglob
    for d in "$REPO_ROOT"/target/*/build/tree-sitter-reify-*/output \
             "$REPO_ROOT"/target/*/*/build/tree-sitter-reify-*/output; do
        m=$(ts_mtime "$d")
        if [ "$m" -gt "$best_m" ]; then
            best_m="$m"
            best="$(dirname "$d")"
        fi
    done
    [ "$had_nullglob" -eq 1 ] || shopt -u nullglob
    printf '%s' "$best"
}

# ts_mirror_run_dir <dest_target_dir>
#
# Mirror the freshest built fingerprint dir's archive (and its stamp, if any)
# into a temp target tree, preserving the dir NAME so a failure message still
# names the real one. Prints the name; returns 1 if there is nothing to mirror.
#
# WHY MIRROR: a real checkout accumulates leftover fingerprint dirs (7 in this
# lane) that cargo will never rebuild again and which therefore carry no
# attestation. In an end-to-end test they would drown the one signal under test.
# Nothing is weakened: the stamp mirrored here is the one build.rs genuinely
# wrote next to the archive cargo genuinely just built.
#
# This is NOT compensating for `check` being unusable against a real target/ —
# that was true when this helper was written, and is no longer (#5629 review
# round 2). `check` now scopes its failing verdict to the LIVE fingerprint dir
# and demotes those leftovers to informational lines, so a plain `check` against
# the real tree is meaningful and is what verify.sh runs after the cargo wave.
# The mirror is kept only to pin the assertion to one KNOWN dir, so the test
# cannot be perturbed by whatever else has been built in the lane.
ts_mirror_run_dir() {
    local dest="$1" run_dir name out
    run_dir=$(ts_freshest_run_dir)
    [ -n "$run_dir" ] || return 1
    name="$(basename "$run_dir")"
    out="$dest/debug/build/$name/out"
    rm -rf "$dest"
    mkdir -p "$out"
    cp "$run_dir/out/libtree_sitter_reify.a" "$out/" 2>/dev/null || return 1
    if [ -f "$run_dir/out/tree_sitter_inputs.stamp" ]; then
        cp "$run_dir/out/tree_sitter_inputs.stamp" "$out/"
    fi
    printf '%s' "$name"
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

# ts_offset_of <haystack> <needle>
#
# Byte offset of <needle>'s first occurrence in <haystack>, or -1 when absent.
# Fork-free (parameter expansion only) for the same reason plan_capture_lib.sh
# is: a pipe-to-grep/awk offset computation is an EINTR surface that can flip an
# ordering assertion to a spurious FAIL under concurrent load (esc-4574-42).
# <needle> is quoted inside the expansion, so it is matched literally.
ts_offset_of() {
    local hay="$1" needle="$2" pre
    if [[ "$hay" != *"$needle"* ]]; then
        printf '%s' -1
        return 0
    fi
    pre="${hay%%"$needle"*}"
    printf '%s' "${#pre}"
}

# ts_last_offset_of <haystack> <needle>
#
# Byte offset of <needle>'s LAST occurrence in <haystack>, or -1 when absent.
# Same fork-free rationale as ts_offset_of; `%` (shortest suffix removal) leaves
# the prefix ending at the final occurrence, where `%%` would leave the first.
#
# Needed because the freshness POST-CONDITION must follow the LAST cargo leaf that
# can compile the parser, not the first: ordering against the first cargo leaf is
# satisfied by a `check` sitting immediately after clippy, which attests a
# different fingerprint dir than the one the later nextest builds link (#5629
# review round 3).
ts_last_offset_of() {
    local hay="$1" needle="$2" pre
    if [[ "$hay" != *"$needle"* ]]; then
        printf '%s' -1
        return 0
    fi
    pre="${hay%"$needle"*}"
    printf '%s' "${#pre}"
}

# ts_masked_path_dir <name-to-mask>
#
# Print a shim directory holding symlinks to every executable currently
# resolvable on PATH EXCEPT <name-to-mask>, so `PATH=$shim` reproduces this
# host minus exactly one binary. PATH dirs are walked in order and the first
# occurrence of a basename wins (`[ -e "$shim/$b" ] && continue`), so
# resolution order is preserved exactly. Registers its own cleanup through
# CLEANUP_ACTIONS.
#
# MASKING BY OMISSION, NOT BY SHADOWING — load-bearing. The obvious shim
# (a `sha256sum` stub that exits 1, placed first on PATH) drives a DIFFERENT
# code path than the one under test: build.rs's `try_hasher` maps ENOENT to
# `Ok(None)` — "not on this host, fall through to the next binary" — but a
# present-but-failing binary to `Err(())` — "transient, retry". A stub
# therefore exercises the retry loop and never reaches the shasum fallback.
# Only genuine absence does.
#
# PRIOR ART, and why this is a new helper rather than a widening of it:
# tests/infra/nextest_absent_lib.sh (task 5602, lifted from task 5599) already
# implements a symlink-farm PATH shim, and its header records the measured trap
# this helper must not re-step in — the naive `PATH="$STUB:/usr/bin:/bin"`
# recipe strips unrelated toolchain wholesale (the tree-sitter CLI lives in the
# cargo bin dir) and cost that task 5 spurious FAILs. The PATTERN is reused
# here; the LIB is not, deliberately:
#   - it simulates exactly one variable ("cargo-nextest is not installed"),
#     whereas this masks an arbitrary named binary;
#   - its temp-HOME / `env -u CARGO_HOME` machinery targets cargo SUBCOMMAND
#     resolution and would work AGAINST this test, which needs the real cargo,
#     rustup toolchain and cc all functioning under the mask;
#   - it is nest-sensitive and shared by three suites, so widening it would
#     charge unrelated tests for this one.
#
# Deliberately NOT named test_* (see the block header above).
ts_masked_path_dir() {
    local mask="${1:-}"
    if [ -z "$mask" ]; then
        echo "  ts_masked_path_dir: usage: ts_masked_path_dir <name-to-mask>" >&2
        return 1
    fi
    local shim
    shim=$(mktemp -d) || return 1
    CLEANUP_ACTIONS+=("rm -rf '$shim'")

    local -a dirs=()
    IFS=':' read -r -a dirs <<< "$PATH"
    local d f b
    local had_nullglob=0
    shopt -q nullglob && had_nullglob=1
    shopt -s nullglob
    for d in "${dirs[@]}"; do
        # An empty PATH element means "the current directory" — deliberately not
        # mirrored: the shim must reproduce the TOOLCHAIN, not the cwd.
        [ -n "$d" ] || continue
        [ -d "$d" ] || continue
        for f in "$d"/*; do
            b="${f##*/}"
            [ "$b" = "$mask" ] && continue
            [ -d "$f" ] && continue
            [ -x "$f" ] || continue
            # First-wins: preserve the real PATH's resolution order.
            [ -e "$shim/$b" ] && continue
            ln -s "$f" "$shim/$b" 2>/dev/null || true
        done
    done
    [ "$had_nullglob" -eq 1 ] || shopt -u nullglob

    printf '%s' "$shim"
}

# mk_verify_fixture
#
# A throwaway git repo holding a copy of scripts/ and .config/, enough for
# `verify.sh ... --print-plan` to classify scope from a staged file list without
# touching the real worktree's index. Prints the directory; registers cleanup.
#
# Copies the WHOLE scripts/ dir (1.8 MB) rather than enumerating verify.sh's
# source closure the way test_verify_scope.sh's make_fixture does: that file
# needs the explicit list because it also asserts on it, whereas here a missing
# lib would only ever produce a confusing failure. Wholesale copy cannot drift.
mk_verify_fixture() {
    local dir
    dir="$(mktemp -d)"
    CLEANUP_ACTIONS+=("rm -rf '$dir'")
    cp -r "$REPO_ROOT/scripts" "$dir/scripts" || return 1
    [ -d "$REPO_ROOT/.config" ] && cp -r "$REPO_ROOT/.config" "$dir/.config"
    mkdir -p "$dir/docs"
    git -C "$dir" init -q || return 1
    git -C "$dir" config user.email "test@test.com"
    git -C "$dir" config user.name "Test"
    printf '%s' "$dir"
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

    # The backup lives OUTSIDE the tracked source tree, for the same reason as
    # test_freshness_detects_and_repairs_stale_archive. tree-sitter-reify/.gitignore
    # covers src/parser.c but NOT src/*.bak (`git check-ignore -v
    # tree-sitter-reify/src/parser.c.bak` exits 1), so an in-tree backup leaves
    # `git status --short` reporting an untracked file for this test's whole
    # duration. The lane cleanliness guards, the lane audit and scripts/land.sh all
    # read that as a dirty worktree.
    local bakdir backup
    bakdir=$(mktemp -d) || return 1
    backup="$bakdir/parser.c.orig"
    # Registered BEFORE the copy so an interrupt in the gap leaks nothing, and
    # AHEAD of the `rm -rf` since cleanup replays in insertion order. `mv -f` is
    # atomic and self-consuming: a failed restore cannot go on to destroy the only
    # copy (CLEANUP_ACTIONS entries run under `eval ... || true`).
    CLEANUP_ACTIONS+=("mv -f '$backup' '$parser'")
    CLEANUP_ACTIONS+=("rm -rf '$bakdir'")
    # `|| return 1`: set -e is disabled inside test bodies (the runner invokes them
    # as `if "$t"; then`), so an unchecked cp would fail silently and the `rm -f`
    # below would then delete parser.c with no usable copy to restore from.
    cp "$parser" "$backup" || return 1

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

    # The backup lives OUTSIDE the tracked source tree, for the same reason as
    # test_auto_generation_rebuilds_parser and
    # test_freshness_detects_and_repairs_stale_archive. This site is the WORST of
    # the three: grammar.js is TRACKED (unlike the gitignored src/parser.c), so
    # while this test runs `git status --short` reported BOTH ` D
    # tree-sitter-reify/grammar.js` — a tracked-file deletion — and an untracked
    # tree-sitter-reify/grammar.js.bak (`git check-ignore` exits 1 on it; the
    # .gitignore covers src/parser.c and *.o, not *.bak). The lane cleanliness
    # guards, the lane audit and scripts/land.sh all read that as a dirty worktree.
    local bakdir backup
    bakdir=$(mktemp -d) || return 1
    backup="$bakdir/grammar.js.orig"
    # Registered BEFORE the move, so an interrupt in the gap cannot leave the tree
    # with grammar.js gone and no registered restore — the previous order (move
    # first, register second) had exactly that window, and here the file at risk
    # is tracked. Restore is registered AHEAD of the `rm -rf` since cleanup
    # replays in insertion order. `mv -f` is atomic and self-consuming.
    CLEANUP_ACTIONS+=("mv -f '$backup' '$grammar'")
    CLEANUP_ACTIONS+=("rm -rf '$bakdir'")
    # `|| return 1`: set -e is disabled inside test bodies (the runner invokes them
    # as `if "$t"; then`), so an unchecked mv would fail silently and the assertion
    # below would then be checking the script against a grammar.js that is still
    # present — passing or failing for the wrong reason.
    mv "$grammar" "$backup" || return 1

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

    # Deliberately NO source-text assertions here (no shebang pin, no
    # `grep 'set -euo pipefail'`). Both were tried and removed: they pin cosmetic
    # detail without exercising behaviour. A grep passes on the string appearing in
    # a comment — this script's own header discusses exactly that text — and fails
    # on a semantically identical `set -e; set -u; set -o pipefail`; a shebang pin
    # fails on `#!/bin/bash` even though the script works identically as a plan
    # leaf. What actually matters (the file exists, is directly runnable, and
    # parses) is asserted above; strict-mode BEHAVIOUR is covered by the modes
    # exercised elsewhere in this suite, which assert exit codes rather than text.
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

    # ---- (8) an UNAVAILABLE dir sorting FIRST must not suppress a stale sibling ----
    # An UNAVAILABLE stamp is a caveat about ONE archive, never a verdict about the
    # run. Degrading the whole scan on it re-creates, inside the guard, exactly the
    # false GREEN this task exists to close: a genuinely stale sibling goes
    # unexamined and the gate passes against an archive nothing vouches for. It is
    # also silent and permanent — dead fingerprint dirs are never rebuilt, so the
    # stamp is never rewritten, and target/ is CoW-cloned into every lane seeded
    # from that base. build.rs's sha256_of returns None on ANY subprocess failure
    # (fork pressure, EMFILE), not just a missing binary, so the sentinel is
    # reachable on the ordinary Linux build host.
    #
    # ts_archives sorts with `LC_ALL=C sort -u`, so the fingerprint-dir names pick
    # the traversal order. cccc... < dddd..., so the unattestable dir is met FIRST:
    # this pins that the scan does not ABORT before reaching the stale one.
    mk_ts_fixture "$tmp/t_unavail_first" "tree-sitter-reify-cccccccccccccccc" "UNAVAILABLE"
    mk_ts_fixture "$tmp/t_unavail_first" "tree-sitter-reify-dddddddddddddddd" "$altered"
    run_ts_freshness check "$tmp/t_unavail_first"
    if [ "$TS_FRESHNESS_RC" -ne 1 ]; then
        echo ""
        echo "  ASSERTION FAILED: check exited $TS_FRESHNESS_RC (expected 1) with an"
        echo "  UNAVAILABLE dir sorting BEFORE a genuinely stale one."
        echo "  One unattestable archive must not convert the run into a global skip."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"tree-sitter-reify-dddddddddddddddd"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: the stale dir (dddd...) is not named — the scan stopped"
        echo "  at the unattestable dir (cccc...) instead of continuing past it."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    # The unattestable dir must still be REPORTED, as a labelled per-dir SKIP: it is
    # unproven, not proven-fresh, and a reader has to be able to act on it.
    local skip_line=""
    while IFS= read -r line; do
        [[ "$line" == *"tree-sitter-reify-cccccccccccccccc"* ]] && skip_line="$line"
    done <<< "$TS_FRESHNESS_OUT"
    if [ -z "$skip_line" ]; then
        echo ""
        echo "  ASSERTION FAILED: the unattestable dir (cccc...) is not mentioned at all."
        echo "  Silently dropping it would report FRESH-or-STALE over an archive whose"
        echo "  provenance was never established."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$skip_line" != *"SKIP"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: the unattestable dir's line is not labelled SKIP"
        echo "  line: <<<$skip_line>>>"
        return 1
    fi

    # ---- (9) an UNAVAILABLE dir sorting LAST must not discard an accumulated stale ----
    # aaaa... < eeee..., so the stale dir is met FIRST and is already in the stale
    # set when the unattestable one is reached. This pins the OTHER half of the
    # defect: a whole-scan bail-out also THROWS AWAY everything accumulated so far.
    # Both orderings are required — either alone leaves half the bug untested.
    mk_ts_fixture "$tmp/t_unavail_last" "tree-sitter-reify-aaaaaaaaaaaaaaaa" "$altered"
    mk_ts_fixture "$tmp/t_unavail_last" "tree-sitter-reify-eeeeeeeeeeeeeeee" "UNAVAILABLE"
    run_ts_freshness check "$tmp/t_unavail_last"
    if [ "$TS_FRESHNESS_RC" -ne 1 ]; then
        echo ""
        echo "  ASSERTION FAILED: check exited $TS_FRESHNESS_RC (expected 1) with an"
        echo "  UNAVAILABLE dir sorting AFTER a genuinely stale one."
        echo "  A stale archive already found must not be discarded by a later"
        echo "  unattestable sibling."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"tree-sitter-reify-aaaaaaaaaaaaaaaa"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: the stale dir (aaaa...) is not named — its already-"
        echo "  accumulated stale entry was dropped when the scan hit eeee...."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (10) a stale DORMANT dir must NOT fail check when the LIVE dir is fresh ----
    # This is what makes `check` usable at all. Every checkout accumulates
    # fingerprint dirs for build configs it no longer uses — 7 in this lane, 9 in
    # the main checkout. Cargo never rebuilds them, so once the sources move on
    # they are stale FOREVER, with no action available to anyone. A check that
    # failed on them would be permanently RED in any long-lived checkout: not an
    # assertion, just a red herring that trains readers to ignore the gate.
    #
    # LIVE is decided by the mtime of cargo's own `output` marker in the run dir,
    # NOT by name order. Here the stale dormant dir sorts LAST (ffff... > 1111...)
    # yet the fresh dir is newer, so a name-ordered or last-wins implementation
    # fails this case.
    mk_ts_fixture "$tmp/t_dormant" "tree-sitter-reify-1111111111111111" "$fp"      "2024-01-02 00:00:00"
    mk_ts_fixture "$tmp/t_dormant" "tree-sitter-reify-ffffffffffffffff" "$altered" "2024-01-01 00:00:00"
    run_ts_freshness check "$tmp/t_dormant"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: check exited $TS_FRESHNESS_RC (expected 0) when the only"
        echo "  stale archive is a DORMANT fingerprint dir cargo will never rebuild or link."
        echo "  A whole-tree assertion here is permanently RED in a real checkout."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    # Not asserted on is not the same as hidden: a green check must still disclose
    # exactly which archives it declined to vouch for.
    if [[ "$TS_FRESHNESS_OUT" != *"tree-sitter-reify-ffffffffffffffff"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: the dormant stale dir (ffff...) is not reported at all."
        echo "  Demoting it to non-failing must not mean dropping it silently."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"tree-sitter-reify-1111111111111111"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: check does not name the LIVE dir (1111...) it asserted on."
        echo "  A verdict that does not say WHICH archive it proved fresh is not evidence."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (11) a stale LIVE dir MUST fail check, even beside a fresh dormant one ----
    # The other half of the narrowing: demoting dormant dirs must not also demote
    # the one archive cargo is actually about to link. Here the stale dir sorts
    # FIRST (1111... < ffff...) and is the NEWER one, so it is live.
    mk_ts_fixture "$tmp/t_live_stale" "tree-sitter-reify-1111111111111111" "$altered" "2024-01-02 00:00:00"
    mk_ts_fixture "$tmp/t_live_stale" "tree-sitter-reify-ffffffffffffffff" "$fp"      "2024-01-01 00:00:00"
    run_ts_freshness check "$tmp/t_live_stale"
    if [ "$TS_FRESHNESS_RC" -ne 1 ]; then
        echo ""
        echo "  ASSERTION FAILED: check exited $TS_FRESHNESS_RC (expected 1) when the LIVE"
        echo "  archive — the one cargo just built and will link — is stale."
        echo "  This is precisely the false GREEN the guard exists to close."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"tree-sitter-reify-1111111111111111"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: the stale LIVE dir (1111...) is not named"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (12) invoked.timestamp alone is enough to establish liveness ----
    # Cargo writes `output` and `invoked.timestamp` together, but only `output`
    # carries the build-script stdout, and a build that failed part-way can leave
    # one without the other. The fallback keeps liveness determinable there rather
    # than silently reverting to assert-on-everything.
    mk_ts_fixture "$tmp/t_invoked" "tree-sitter-reify-1111111111111111" "$fp"
    mk_ts_fixture "$tmp/t_invoked" "tree-sitter-reify-ffffffffffffffff" "$altered"
    : > "$tmp/t_invoked/debug/build/tree-sitter-reify-1111111111111111/invoked.timestamp"
    touch -d "2024-01-02 00:00:00" "$tmp/t_invoked/debug/build/tree-sitter-reify-1111111111111111/invoked.timestamp"
    : > "$tmp/t_invoked/debug/build/tree-sitter-reify-ffffffffffffffff/invoked.timestamp"
    touch -d "2024-01-01 00:00:00" "$tmp/t_invoked/debug/build/tree-sitter-reify-ffffffffffffffff/invoked.timestamp"
    run_ts_freshness check "$tmp/t_invoked"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: check exited $TS_FRESHNESS_RC (expected 0) when liveness"
        echo "  is carried by invoked.timestamp alone (no 'output' file present)."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (13) the RUN WINDOW: every dir rebuilt this run is asserted on ----
    # Scoping the failing verdict to the single NEWEST-marker dir is not enough
    # under `--profile both`: the parser compiles into clippy's fingerprint dir,
    # the debug nextest dir AND the release nextest dir, so exactly one of them is
    # newest and the others go unexamined — the same false GREEN one level up
    # (#5629 review round 3). `ensure` stamps TARGET_DIR/.tree-sitter-freshness.epoch
    # before the cargo wave; every dir whose build-script run marker lands at or
    # after that epoch is in the window and is hard-asserted on.
    #
    # The stale dir here is deliberately the OLDER of the two, i.e. NOT the newest —
    # under the pre-fix newest-marker scope it was invisible.
    mk_ts_fixture "$tmp/t_window" "tree-sitter-reify-1111111111111111" "$altered" "2024-01-01 00:00:00"
    mk_ts_fixture "$tmp/t_window" "tree-sitter-reify-ffffffffffffffff" "$fp"      "2024-01-02 00:00:00"

    # (13a) epoch BEFORE both markers -> both in the window -> the stale NON-newest
    # dir must fail. This is the assertion that the widening exists for.
    touch -d "2023-12-31 00:00:00" "$tmp/t_window/.tree-sitter-freshness.epoch"
    run_ts_freshness check "$tmp/t_window"
    if [ "$TS_FRESHNESS_RC" -ne 1 ]; then
        echo ""
        echo "  ASSERTION FAILED: check exited $TS_FRESHNESS_RC (expected 1) with a STALE"
        echo "  non-newest dir inside this run's window. Under --profile both the parser"
        echo "  compiles into several fingerprint dirs; asserting only on the newest one"
        echo "  lets a stale archive that a test binary DOES link pass unexamined."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"tree-sitter-reify-1111111111111111"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: the stale in-window dir (1111...) is not named."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # (13a2) the window lasts exactly ONE ensure/check pair: check CONSUMES the
    # epoch it just read, and a repeat check — with no intervening `ensure` — falls
    # back to newest-marker scope on the very same tree.
    #
    # Pins the epoch LIFECYCLE, which is the half that was missing. `ensure`
    # created the file and nothing ever removed it, so "no epoch on disk" was a
    # state a real checkout left behind after its first verify run and never
    # revisited: every later standalone check silently replayed the last run's
    # window — hours or days stale — hard-asserting on dirs that run rebuilt but
    # which have since gone dormant. That is the un-actionable permanent RED the
    # live/dormant split exists to prevent. Note that (13b) below reaches the same
    # scope by explicitly `rm -f`-ing the epoch, i.e. it tests a synthetic tree;
    # this case is the REAL standalone path.
    if [ -f "$tmp/t_window/.tree-sitter-freshness.epoch" ]; then
        echo ""
        echo "  ASSERTION FAILED: check left the run-window epoch on disk. It must consume"
        echo "  the window it read, or every later standalone check inherits this run's"
        echo "  scope forever and hard-asserts on dirs that have since gone dormant."
        return 1
    fi
    run_ts_freshness check "$tmp/t_window"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: a REPEAT check (no new 'ensure') exited $TS_FRESHNESS_RC"
        echo "  (expected 0). With the window consumed, the same tree must be judged at"
        echo "  newest-marker scope — the stale dir is dormant, hence a note, not a RED."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # (13b) CONTROL — no epoch on disk degrades to the old newest-marker scope, so
    # the very same tree goes green. Without this the case above could pass for a
    # reason unrelated to the window (e.g. a whole-tree assertion), and a revert to
    # newest-marker-only would still look tested. Narrower is the documented
    # degradation for a missing/unwritable epoch: never wider, never skipped.
    rm -f "$tmp/t_window/.tree-sitter-freshness.epoch"
    run_ts_freshness check "$tmp/t_window"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: check exited $TS_FRESHNESS_RC (expected 0) with NO epoch"
        echo "  on disk. A standalone check outside a verify run must fall back to the"
        echo "  newest-marker scope, not hard-fail on forever-stale dormant dirs."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # (13c) epoch AFTER both markers -> nothing was rebuilt this run -> dormant,
    # not RED. This is what stops the 7-9 forever-stale dirs a real checkout
    # carries from making the gate permanently red.
    touch -d "2024-01-03 00:00:00" "$tmp/t_window/.tree-sitter-freshness.epoch"
    run_ts_freshness check "$tmp/t_window"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: check exited $TS_FRESHNESS_RC (expected 0) when the epoch"
        echo "  postdates every marker — cargo rebuilt nothing this run, so every dir is"
        echo "  dormant. Failing here would make the window a permanent RED instead of a"
        echo "  statement about what THIS run built."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
}

test_freshness_ensure_forces_and_is_idempotent() {
    # Hermetic: a COPY of the tree-sitter sources under mktemp -d, driven via both
    # REIFY_TS_FRESHNESS_TS_DIR and REIFY_TS_FRESHNESS_TARGET_DIR. The real
    # worktree's mtimes are never touched, so this cannot perturb a concurrent
    # cargo build in the lane.
    assert_file_exists "$FRESHNESS_SCRIPT" || return 1

    if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
        echo "  SKIP: no sha256sum/shasum on PATH — the guard degrades to UNAVAILABLE here"
        return 0
    fi

    local tmp
    tmp=$(mktemp -d)
    CLEANUP_ACTIONS+=("rm -rf '$tmp'")

    local ts="$tmp/ts"
    mkdir -p "$ts/src/tree_sitter"
    cp "$TS_DIR/grammar.js"    "$ts/grammar.js"
    provision_fixture_parser_c "$ts/src/parser.c" || return 1
    cp "$TS_DIR/src/scanner.c" "$ts/src/scanner.c"
    cp "$TS_DIR"/src/tree_sitter/*.h "$ts/src/tree_sitter/"

    # Model the warm-lane bulk stamp: seed-warm-lane.sh sets every non-target/,
    # non-.git file to 2020-01-01T00:00:00 (measured by task #5630), which is what
    # makes mtime useless as a freshness signal in a lane.
    local OLD='2020-01-01T00:00:00'
    local f
    stamp_at() {
        local d="$1" when="$2" x
        while IFS= read -r x; do touch -d "$when" "$x"; done < <(ts_watched_files "$d")
        touch -d "$when" "$d/src/parser.c"
    }
    stamp_old() { stamp_at "$1" "$OLD"; }
    stamp_old "$ts"

    local baseline
    baseline=$(while IFS= read -r f; do ts_mtime "$f"; done < <(ts_watched_files "$ts"))

    # Helpers over the watched set. Defined inline so they close over $ts.
    mtimes_now() {
        while IFS= read -r f; do ts_mtime "$f"; done < <(ts_watched_files "$ts")
    }
    all_recent() {
        local now m
        now=$(date +%s)
        while IFS= read -r m; do
            [ $(( now - m )) -le 300 ] || return 1
        done <<< "$(mtimes_now)"
    }

    local hash_grammar_before hash_scanner_before
    hash_grammar_before=$(sha256sum "$ts/grammar.js" | awk '{print $1}')
    hash_scanner_before=$(sha256sum "$ts/src/scanner.c" | awk '{print $1}')

    local fp
    fp=$(REIFY_TS_FRESHNESS_TS_DIR="$ts" bash "$FRESHNESS_SCRIPT" --print-fingerprint)

    local altered
    if [ "${fp:0:1}" = "0" ]; then altered="1${fp:1}"; else altered="0${fp:1}"; fi

    # ---- (1) STALE fixture -> exit 0, watched inputs touched, loud "forcing" line ----
    mk_ts_fixture "$tmp/target" "tree-sitter-reify-deadbeef01" "$altered"
    run_ts_freshness ensure "$tmp/target" "$ts"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure on a STALE fixture exited $TS_FRESHNESS_RC (expected 0)"
        echo "  ensure REPAIRS; failing the gate on a repairable condition would just turn"
        echo "  a false GREEN into a spurious RED that every later run reproduces."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if ! all_recent; then
        echo ""
        echo "  ASSERTION FAILED: ensure did not bump the watched inputs' mtimes"
        echo "  Bumping a watched input's mtime is the ONLY lever available: a build script"
        echo "  cargo has declined to run cannot repair itself."
        echo "  --- mtimes ---"; mtimes_now
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"forcing rebuild"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: ensure did not print a greppable 'forcing rebuild' line"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (2) ensure must NEVER rewrite content ----
    # This is what keeps `git status` clean in the shared merge lane; a content
    # rewrite would trip the cleanliness guards mid-gate.
    local hash_grammar_after hash_scanner_after
    hash_grammar_after=$(sha256sum "$ts/grammar.js" | awk '{print $1}')
    hash_scanner_after=$(sha256sum "$ts/src/scanner.c" | awk '{print $1}')
    if [ "$hash_grammar_before" != "$hash_grammar_after" ] || \
       [ "$hash_scanner_before" != "$hash_scanner_after" ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure changed file CONTENT (mtime-only touch expected)"
        echo "  grammar.js: $hash_grammar_before -> $hash_grammar_after"
        echo "  scanner.c : $hash_scanner_before -> $hash_scanner_after"
        return 1
    fi

    # ---- (3) ledger records the fingerprint the force was applied for ----
    local ledger="$tmp/target/.tree-sitter-freshness.ledger"
    assert_file_exists "$ledger" || return 1
    if [ "$(cat "$ledger")" != "$fp" ]; then
        echo ""
        echo "  ASSERTION FAILED: ledger content != --print-fingerprint output"
        echo "  --- ledger ---"; cat "$ledger"
        echo "  --- fingerprint ---"; echo "$fp"
        return 1
    fi

    # ---- (4) idempotence: a permanently-dead fingerprint dir must not force-loop ----
    # The fixture stays STALE (dead fingerprint dirs are never rebuilt, so they are
    # stale forever — 7 such dirs exist in this lane). Without the ledger, ensure
    # would touch grammar.js on EVERY verify run, forcing a full ~5 MB parser.c
    # recompile each time.
    #
    # The mtimes are deliberately NOT rewound to 2020 here. Case (1) just forced,
    # and the ledger's mtime witness records that bump; stamping the sources back
    # would BE the warm-lane reseed signature, and ensure would then correctly
    # re-force (case (11) below). What this case models is two consecutive verify
    # runs in ONE checkout, where run 1's bump is still on disk. Parking them a day
    # in the future keeps the comparison exact at stat's 1-second resolution — a
    # re-force would move them back to now, which is unambiguously different.
    local future post_force
    future="@$(( $(date +%s) + 86400 ))"
    stamp_at "$ts" "$future"
    post_force=$(mtimes_now)
    run_ts_freshness ensure "$tmp/target" "$ts"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: second ensure exited $TS_FRESHNESS_RC (expected 0)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [ "$(mtimes_now)" != "$post_force" ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure re-forced for a fingerprint already in the ledger"
        echo "  whose force is demonstrably still on disk."
        echo "  This is the force-loop the ledger exists to prevent."
        echo "  --- expected (post-force stamp) ---"; echo "$post_force"
        echo "  --- actual ---"; mtimes_now
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"already"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: second ensure did not report the force as already applied"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (5) a CONTENT change invalidates the ledger -> force again ----
    printf '\n/* task 5629 ledger-invalidation probe */\n' >> "$ts/src/scanner.c"
    stamp_old "$ts"
    run_ts_freshness ensure "$tmp/target" "$ts"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure after a content change exited $TS_FRESHNESS_RC (expected 0)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if ! all_recent; then
        echo ""
        echo "  ASSERTION FAILED: a scanner.c content change did not invalidate the ledger"
        echo "  A stale ledger that outlives its inputs would suppress the force forever —"
        echo "  strictly worse than having no ledger at all."
        echo "  --- mtimes ---"; mtimes_now
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (6) FRESH fixture -> no bump, no 'forcing' line ----
    local fp2
    fp2=$(REIFY_TS_FRESHNESS_TS_DIR="$ts" bash "$FRESHNESS_SCRIPT" --print-fingerprint)
    mk_ts_fixture "$tmp/target_fresh" "tree-sitter-reify-deadbeef01" "$fp2"
    stamp_old "$ts"
    run_ts_freshness ensure "$tmp/target_fresh" "$ts"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure on a FRESH fixture exited $TS_FRESHNESS_RC (expected 0)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [ "$(mtimes_now)" != "$baseline" ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure forced despite every archive being FRESH"
        echo "  --- expected (2020 stamp) ---"; echo "$baseline"
        echo "  --- actual ---"; mtimes_now
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" == *"forcing rebuild"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: ensure printed 'forcing rebuild' for a FRESH tree"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (7) nothing built at all -> exit 0, no bump ----
    mkdir -p "$tmp/target_empty"
    stamp_old "$ts"
    run_ts_freshness ensure "$tmp/target_empty" "$ts"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure on an EMPTY target dir exited $TS_FRESHNESS_RC (expected 0)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [ "$(mtimes_now)" != "$baseline" ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure forced with nothing built (nothing built = nothing stale)"
        echo "  --- expected (2020 stamp) ---"; echo "$baseline"
        echo "  --- actual ---"; mtimes_now
        return 1
    fi

    # ---- (8) an UNAVAILABLE dir must not suppress the REPAIR of a stale sibling ----
    # The operationally load-bearing half of the scoped-degradation fix: check
    # merely reports, but ensure is what actually cures the stale archive before
    # the cargo leaves run. If one unattestable dir degrades the whole scan, ensure
    # returns a global SKIP and repairs NOTHING — the stale archive survives into
    # the gate. cccc... sorts before dddd..., so the unattestable dir is met first.
    #
    # Fresh target dir => no ledger => the force is genuinely due here. The
    # fingerprint is recomputed rather than reusing $fp, which case (5) invalidated
    # by appending to scanner.c.
    local fp3 altered3
    fp3=$(REIFY_TS_FRESHNESS_TS_DIR="$ts" bash "$FRESHNESS_SCRIPT" --print-fingerprint)
    if [ "${fp3:0:1}" = "0" ]; then altered3="1${fp3:1}"; else altered3="0${fp3:1}"; fi

    mk_ts_fixture "$tmp/target_unavail" "tree-sitter-reify-cccccccccccccccc" "UNAVAILABLE"
    mk_ts_fixture "$tmp/target_unavail" "tree-sitter-reify-dddddddddddddddd" "$altered3"
    stamp_old "$ts"
    run_ts_freshness ensure "$tmp/target_unavail" "$ts"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure past an UNAVAILABLE dir exited $TS_FRESHNESS_RC (expected 0)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"forcing rebuild"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: ensure did not report a force with one unattestable dir"
        echo "  present alongside a genuinely stale one — it degraded the whole run to a"
        echo "  skip instead of scoping the degradation to that one archive."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if ! all_recent; then
        echo ""
        echo "  ASSERTION FAILED: ensure did not bump the watched inputs' mtimes, so the"
        echo "  stale archive stands and cargo will link it unchanged. An unattestable"
        echo "  sibling must never disable the repair."
        echo "  --- mtimes ---"; mtimes_now
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (9) the ledger must NOT silence a stale LIVE archive ----
    # A force that was applied but did NOT take is precisely the failure mode the
    # post-cargo `check` leaf exists to surface, and the ledger is what could hide
    # it: run 1 forces and records the fingerprint, cargo somehow does not rebuild,
    # and every later run sees "already forced for these exact bytes" and goes
    # quiet — leaving the gate to link an archive it never compiled.
    #
    # So the ledger short-circuit is scoped to DORMANT staleness (dirs cargo will
    # never rebuild, where re-forcing buys a recompile per run for nothing). The
    # live archive is re-forced every run until it comes back fresh.
    local fp4 altered4
    fp4=$(REIFY_TS_FRESHNESS_TS_DIR="$ts" bash "$FRESHNESS_SCRIPT" --print-fingerprint)
    if [ "${fp4:0:1}" = "0" ]; then altered4="1${fp4:1}"; else altered4="0${fp4:1}"; fi

    # Live (newer marker) and STALE; the dormant sibling is fresh, so the only
    # thing driving a force is the live archive.
    mk_ts_fixture "$tmp/target_ledger" "tree-sitter-reify-1111111111111111" "$altered4" "2024-01-02 00:00:00"
    mk_ts_fixture "$tmp/target_ledger" "tree-sitter-reify-ffffffffffffffff" "$fp4"      "2024-01-01 00:00:00"
    # Pre-seed the ledger exactly as a previous run's force would have left it —
    # BOTH halves: the fingerprint AND the mtime witness attesting that force is
    # still on disk. Seeding only the fingerprint would make ensure bypass on the
    # missing witness instead, and this case would go green without ever
    # exercising the live-archive rule it exists for.
    printf '%s\n' "$fp4" > "$tmp/target_ledger/.tree-sitter-freshness.ledger"

    stamp_old "$ts"
    printf '%s\n' "$(ts_witness_value "$ts")" \
        > "$tmp/target_ledger/.tree-sitter-freshness.ledger.mtime"
    run_ts_freshness ensure "$tmp/target_ledger" "$ts"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure with a matching ledger exited $TS_FRESHNESS_RC (expected 0)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if ! all_recent; then
        echo ""
        echo "  ASSERTION FAILED: ensure honoured the ledger and skipped the force while the"
        echo "  LIVE archive — the one cargo links — was still stale. That is the state a"
        echo "  force which failed to take leaves behind, and going quiet on it is the"
        echo "  false GREEN this guard exists to close."
        echo "  --- mtimes ---"; mtimes_now
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (10) ...but a matching ledger DOES still silence purely DORMANT staleness ----
    # The other side of the same rule: without it, the 7-9 permanently-stale
    # fingerprint dirs a real checkout carries would force a full ~5 MB parser.c
    # recompile on every single verify run.
    mk_ts_fixture "$tmp/target_ledger2" "tree-sitter-reify-1111111111111111" "$fp4"      "2024-01-02 00:00:00"
    mk_ts_fixture "$tmp/target_ledger2" "tree-sitter-reify-ffffffffffffffff" "$altered4" "2024-01-01 00:00:00"
    printf '%s\n' "$fp4" > "$tmp/target_ledger2/.tree-sitter-freshness.ledger"

    stamp_old "$ts"
    # Witness sampled AFTER the stamp, so it says "the mtimes on disk right now are
    # the ones the force left" — the honest previous-run state. See case (11) for
    # what happens when that is no longer true.
    printf '%s\n' "$(ts_witness_value "$ts")" \
        > "$tmp/target_ledger2/.tree-sitter-freshness.ledger.mtime"
    baseline=$(mtimes_now)
    run_ts_freshness ensure "$tmp/target_ledger2" "$ts"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure on dormant-only staleness exited $TS_FRESHNESS_RC (expected 0)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [ "$(mtimes_now)" != "$baseline" ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure re-forced for a fingerprint already in the ledger"
        echo "  when the ONLY stale archives were dormant ones cargo never rebuilds."
        echo "  This is the per-run parser.c recompile the ledger exists to prevent."
        echo "  --- expected (2020 stamp) ---"; echo "$baseline"
        echo "  --- actual ---"; mtimes_now
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (11) ...unless the force the ledger stands for has been UNDONE ----
    # The warm-lane rewind. scripts/seed-warm-lane.sh bulk-stamps every non-target/
    # file back to 2020-01-01 while target/ — ledger included — is CoW-cloned
    # intact, so a seeded lane inherits ledger==CURRENT_FP beside 2020 source
    # mtimes and a stale dormant fingerprint dir. On the fingerprint alone the
    # short-circuit above fires and no force happens; when that lane's build config
    # then resolves to the dormant dir, cargo compares the 2020 mtimes against the
    # dir's later recorded reference, calls it unchanged, and links the stale
    # archive. That is a residual instance of the exact false GREEN this guard
    # exists to close, so the ledger carries an mtime WITNESS and a current max
    # BELOW it re-forces.
    #
    # Identical to case (10) in every respect except the witness, which is stamped
    # from a PRE-reseed clock (now) while the sources sit at 2020.
    mk_ts_fixture "$tmp/target_rewind" "tree-sitter-reify-1111111111111111" "$fp4"      "2024-01-02 00:00:00"
    mk_ts_fixture "$tmp/target_rewind" "tree-sitter-reify-ffffffffffffffff" "$altered4" "2024-01-01 00:00:00"
    printf '%s\n' "$fp4" > "$tmp/target_rewind/.tree-sitter-freshness.ledger"
    stamp_old "$ts"
    printf '%s\n' "$(date +%s)" > "$tmp/target_rewind/.tree-sitter-freshness.ledger.mtime"

    run_ts_freshness ensure "$tmp/target_rewind" "$ts"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure after an mtime rewind exited $TS_FRESHNESS_RC (expected 0)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if ! all_recent; then
        echo ""
        echo "  ASSERTION FAILED: ensure honoured a ledger whose force had been rewound away."
        echo "  The fingerprint says 'a force was applied for these bytes'; it does NOT say"
        echo "  the mtime bump is still on disk, and a warm-lane reseed removes it while"
        echo "  CoW-cloning the ledger. Skipping the force there leaves cargo free to"
        echo "  resurrect the dormant dir and link the stale archive."
        echo "  --- mtimes ---"; mtimes_now
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"no longer on disk"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: ensure re-forced past a MATCHING ledger without saying why."
        echo "  'the ledger matched, yet we forced anyway' is otherwise indistinguishable"
        echo "  from a bug in the short-circuit."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (12) DETECTED BUT UNREPAIRABLE must be LOUD, never silently green ----
    # ensure's only failing exit path, and the single way this leaf can fail the
    # merge gate. Everything above asserts exit 0, so a regression that swallowed
    # the touch failure — dropping `failed=1`, or an `ts_force_touch || true` —
    # would turn a detected-but-unrepairable stale archive into a silent GREEN
    # with nothing to catch it: the same defect class as the original bug.
    #
    # Driven against a THROWAWAY copy of the source tree, so breaking the watched
    # set cannot perturb the cases above or leak into the real worktree.
    local broken="$tmp/ts_broken"
    cp -a "$ts" "$broken" || return 1

    local fp5 altered5
    fp5=$(REIFY_TS_FRESHNESS_TS_DIR="$broken" bash "$FRESHNESS_SCRIPT" --print-fingerprint)
    if [ "${fp5:0:1}" = "0" ]; then altered5="1${fp5:1}"; else altered5="0${fp5:1}"; fi
    # No marker => liveness undeterminable => the archive is treated as live; no
    # ledger in this fresh target dir => the force is unconditionally due.
    mk_ts_fixture "$tmp/target_unrepairable" "tree-sitter-reify-2222222222222222" "$altered5"

    # (12a) a watched input that is GONE. grammar.js is watched but is not part of
    # the fingerprint (it is an input to generation, not to compilation), so the
    # scan still computes a verdict and the failure lands squarely in ts_force_touch.
    rm -f "$broken/grammar.js"
    run_ts_freshness ensure "$tmp/target_unrepairable" "$broken"
    if [ "$TS_FRESHNESS_RC" -ne 1 ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure exited $TS_FRESHNESS_RC (expected 1) with a stale"
        echo "  archive it could NOT repair — a watched input was missing, so the force"
        echo "  cannot have taken. Continuing green here reports success against a parser"
        echo "  the gate never compiled."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"DETECTED BUT UNREPAIRABLE"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: the unrepairable exit is not greppable — no"
        echo "  'DETECTED BUT UNREPAIRABLE' line. A bare non-zero exit from a mid-plan leaf"
        echo "  leaves the operator no way to tell this apart from a crash."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # (12b) the other arm: the input EXISTS but `touch` on it fails. Reached via a
    # symlink to a root-owned file, since `touch` succeeds for the OWNER of a
    # mode-0444 file (verified on this host) — chmod alone cannot deny it.
    # The probe is the guard AND is non-mutating: if the touch unexpectedly
    # succeeds (running as root, an exotic mount), the arm is skipped rather than
    # asserted, so this can never become a flaky or destructive case.
    local unwritable='/proc/version'
    if [ "$(id -u)" != "0" ] && [ -f "$unwritable" ] && ! touch "$unwritable" 2>/dev/null; then
        ln -s "$unwritable" "$broken/grammar.js" || return 1
        run_ts_freshness ensure "$tmp/target_unrepairable" "$broken"
        rm -f "$broken/grammar.js"
        if [ "$TS_FRESHNESS_RC" -ne 1 ]; then
            echo ""
            echo "  ASSERTION FAILED: ensure exited $TS_FRESHNESS_RC (expected 1) when 'touch'"
            echo "  itself failed on a watched input. A read-only tree cannot be repaired, and"
            echo "  a silent 0 here is the false GREEN with an extra step."
            echo "$TS_FRESHNESS_OUT"
            return 1
        fi
        if [[ "$TS_FRESHNESS_OUT" != *"DETECTED BUT UNREPAIRABLE"* ]] || \
           [[ "$TS_FRESHNESS_OUT" != *"'touch' failed"* ]]; then
            echo ""
            echo "  ASSERTION FAILED: the touch-failure arm did not name itself. Expected both"
            echo "  a \"'touch' failed\" line and 'DETECTED BUT UNREPAIRABLE'."
            echo "$TS_FRESHNESS_OUT"
            return 1
        fi
    else
        echo "  (skipped 12b: no unwritable regular file available here — running as root?)"
    fi
}

test_freshness_target_dir_override_is_a_clean_noop() {
    # REIFY_TS_FRESHNESS_TARGET_DIR pointed at an EMPTY tree must make BOTH modes
    # clean no-ops (exit 0) that leave the real worktree untouched.
    #
    # This is not a curiosity of the knob — it is the contract
    # tests/infra/test_verify_semaphore_e2e.sh's apply_hermetic_env now depends on.
    # That harness stubs cargo and then runs verify.sh for real, so it executes
    # both freshness leaves while nothing is ever compiled:
    #   - `check`'s post-condition ("every archive rebuilt in this run's window
    #     matches the sources on disk") would be asserted against archives that
    #     run never built — an unconditional RED in any lane whose real archive is
    #     stale or unattested, with no relation to the semaphore behaviour tested.
    #   - `ensure`'s repair path would bump mtimes on the lane's TRACKED
    #     tree-sitter sources, which in a CoW-seeded warm lane is exactly the
    #     signal this task's whole mechanism reads.
    # Both are closed by pointing the knob at an empty per-run dir. If either mode
    # ever stopped being a no-op there, that suite breaks for a reason nobody
    # would look for in this file — so the contract is pinned here, next to the
    # guard that owns it.
    #
    # Deliberately does NOT set REIFY_TS_FRESHNESS_TS_DIR: the point is that the
    # target-dir knob ALONE suffices, which is precisely how the e2e harness uses
    # it. TS_DIR therefore resolves to the real tree-sitter-reify/ throughout.
    local tmp target
    tmp="$(mktemp -d)"
    CLEANUP_ACTIONS+=("rm -rf '$tmp'")
    target="$tmp/empty-target"
    mkdir -p "$target"

    # Snapshot the REAL worktree state `ensure` would perturb if it ran its repair:
    # the watched inputs' mtimes, plus the real target/'s run-window epoch (whose
    # removal would silently narrow an enclosing verify run's `check` scope — the
    # regression a prior review round caught in this very file).
    local real_epoch="$REPO_ROOT/target/.tree-sitter-freshness.epoch"
    local snap_before snap_after epoch_before epoch_after
    snap_before=$(ts_watched_files "$TS_DIR" | while IFS= read -r f; do
        printf '%s %s\n' "$(ts_mtime "$f")" "$f"
    done)
    epoch_before="absent"
    [ -f "$real_epoch" ] && epoch_before="$(ts_mtime "$real_epoch")"

    # ---- (1) ensure: no built archive => nothing to force ----
    run_ts_freshness ensure "$target"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure against an empty target dir exited $TS_FRESHNESS_RC (expected 0)."
        echo "  The semaphore e2e harness runs this leaf on every hermetic section;"
        echo "  a non-zero here fails that suite wholesale."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"nothing to force"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: ensure against an empty target dir did not report the"
        echo "  no-archive no-op. Expected a 'nothing to force' line naming $target."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # Positive proof the writes were REDIRECTED rather than merely skipped: the
    # run-window epoch ensure stamps unconditionally must land under the override.
    if [ ! -f "$target/.tree-sitter-freshness.epoch" ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure stamped no run-window epoch under the override dir"
        echo "  ($target). Either the knob was ignored — in which case the real target/ was"
        echo "  written instead — or the epoch is no longer unconditional, which would"
        echo "  narrow every later 'check' back to newest-dir-only scope."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (2) check: no built archive => nothing to check ----
    run_ts_freshness check "$target"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: check against an empty target dir exited $TS_FRESHNESS_RC (expected 0)."
        echo "  Nothing was built there, so there is nothing that can be stale — a"
        echo "  non-zero verdict here is a RED with no archive behind it."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"nothing to check"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: check against an empty target dir did not report the"
        echo "  no-archive no-op. Expected a 'nothing to check' line naming $target."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- (3) ...and the real worktree was never touched ----
    snap_after=$(ts_watched_files "$TS_DIR" | while IFS= read -r f; do
        printf '%s %s\n' "$(ts_mtime "$f")" "$f"
    done)
    if [ "$snap_after" != "$snap_before" ]; then
        echo ""
        echo "  ASSERTION FAILED: the target-dir override alone did not spare the real"
        echo "  tree-sitter sources — their mtimes moved. In a CoW-seeded warm lane those"
        echo "  mtimes ARE the rebuild signal, so a harness that perturbs them is mutating"
        echo "  lane-shared state on every hermetic section."
        echo "  --- before ---"; printf '%s\n' "$snap_before"
        echo "  --- after ---";  printf '%s\n' "$snap_after"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    epoch_after="absent"
    [ -f "$real_epoch" ] && epoch_after="$(ts_mtime "$real_epoch")"
    if [ "$epoch_after" != "$epoch_before" ]; then
        echo ""
        echo "  ASSERTION FAILED: the real target/'s run-window epoch changed"
        echo "  ($epoch_before -> $epoch_after) while the override was in force. Writing it"
        echo "  re-opens a window that is not this run's; consuming it narrows an enclosing"
        echo "  verify run's later 'check' to newest-dir-only scope. Either way the override"
        echo "  is not isolating what it claims to."
        return 1
    fi
}

test_semaphore_e2e_harness_isolates_the_freshness_guard() {
    # The other half of the seam pinned above: prove that
    # tests/infra/test_verify_semaphore_e2e.sh's apply_hermetic_env actually
    # EXPORTS REIFY_TS_FRESHNESS_TARGET_DIR, at a per-run path.
    #
    # Mirrors that suite's Section E S-technique, which proves the same helper
    # exports REIFY_COMPILE_GATE_DISABLE. Section E can observe its knob through a
    # fixed marker verify.sh prints to stderr; this knob emits no marker, so the
    # helper is EXTRACTED AND RUN instead and its effect observed directly.
    #
    # Deliberately NOT a grep of the suite's source text. A bare
    # `grep REIFY_TS_FRESHNESS_TARGET_DIR` is satisfied by the knob appearing in a
    # comment — and that helper is now heavily commented about exactly this knob,
    # so the grep would pass with the export deleted. (This file has removed that
    # class of assertion once already.) Observing the value in a GRANDCHILD
    # process is what "exported" actually means, and no comment can fake it.
    assert_file_exists "$SEMAPHORE_E2E_SUITE" || return 1

    local tmp probe stub observed rc=0
    tmp="$(mktemp -d)"
    CLEANUP_ACTIONS+=("rm -rf '$tmp'")
    probe="$tmp/probe.sh"
    # Stands in for the per-run stub dir every call site builds under its own
    # `mktemp -d`; the helper is exports-only, so it need not exist.
    stub="$tmp/run-root/stubs"

    cat > "$probe" <<'PROBE'
# `set -u` is deliberately OFF. The helper is self-contained today, but a future
# edit that derives the path from one of that suite's globals would abort here
# under -u and produce a FAIL about an unbound variable rather than about the
# property under test. Without -u such a global expands empty, the derived path
# stops matching the per-run root, and the per-run assertion below says so.
set -o pipefail
src="$1"; stub="$2"
body=$(sed -n '/^apply_hermetic_env() {$/,/^}$/p' "$src")
if [ -z "$body" ]; then printf '%s' '__NO_FUNCTION__'; exit 0; fi
eval "$body"
apply_hermetic_env "$stub" "$stub.lock" 1
# Grandchild process: only a genuinely EXPORTED variable survives this hop.
bash -c 'printf "%s" "${REIFY_TS_FRESHNESS_TARGET_DIR:-__UNSET__}"'
PROBE

    observed=$(bash "$probe" "$SEMAPHORE_E2E_SUITE" "$stub" 2>&1) || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: could not evaluate apply_hermetic_env in isolation (exit $rc)."
        echo "  The helper is expected to be self-contained (exports only)."
        echo "  --- probe output ---"; printf '%s\n' "$observed"
        return 1
    fi
    if [ "$observed" = "__NO_FUNCTION__" ]; then
        echo ""
        echo "  ASSERTION FAILED: no 'apply_hermetic_env() {' definition found in"
        echo "  $SEMAPHORE_E2E_SUITE. If the helper was renamed or reshaped, this pin must"
        echo "  follow it — silently passing would leave the freshness isolation unproven."
        return 1
    fi
    if [ "$observed" = "__UNSET__" ]; then
        echo ""
        echo "  ASSERTION FAILED: apply_hermetic_env does not export"
        echo "  REIFY_TS_FRESHNESS_TARGET_DIR. Without it that suite runs verify.sh's"
        echo "  freshness leaves against the lane's REAL target/ with cargo stubbed:"
        echo "  'check' asserts a post-condition over archives the run never built (a RED"
        echo "  unrelated to the semaphore), and 'ensure' bumps mtimes on the worktree's"
        echo "  tracked tree-sitter sources. See the no-op contract pinned by"
        echo "  test_freshness_target_dir_override_is_a_clean_noop."
        return 1
    fi
    # Per-run: the value must live inside the caller's own throwaway root, so two
    # concurrent sections (and two concurrent lanes) cannot collide, and cleanup
    # of that root reclaims it. Asserted at the ROOT, not as an exact path, so a
    # future re-derivation inside the same tmp tree stays green.
    if [[ "$observed" != "$tmp/"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: REIFY_TS_FRESHNESS_TARGET_DIR is not derived from the"
        echo "  caller's per-run stub dir. Expected a path under $tmp/, got: $observed"
        echo "  A fixed or shared path would leak one section's run window into the next,"
        echo "  and a path under the repo would defeat the isolation entirely."
        return 1
    fi
    if [[ "$observed" == "$REPO_ROOT/target"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: REIFY_TS_FRESHNESS_TARGET_DIR points into the real build"
        echo "  tree ($observed). That is the state the override exists to avoid."
        return 1
    fi
}

test_build_rs_watches_all_compiled_inputs() {
    # Asserts on cargo's VERBATIM capture of build-script stdout
    # (target/*/build/tree-sitter-reify-*/output), NOT on build.rs source text.
    # A source grep would pass on a directive that is written but never emitted;
    # this is the real runtime contract.
    #
    # Measured on this lane before the fix: that file carried exactly ONE
    # rerun-if-changed line, `grammar.js`. src/scanner.c and all three headers
    # were watched by nothing, so an edit confined to them gave cargo no reason
    # to re-run the build script — the archive was never recompiled and the
    # change was never under test, while the gate reported GREEN.
    touch "$TS_DIR/grammar.js"

    local cargo_out
    cargo_out=$(mktemp)
    CLEANUP_ACTIONS+=("rm -f '$cargo_out'")
    local guard_rc=0
    run_guarded_cargo_check "$cargo_out" timeout 300 cargo check -p tree-sitter-reify \
        --manifest-path "$REPO_ROOT/Cargo.toml" || guard_rc=$?
    if [ "$guard_rc" -eq 2 ]; then return 0; fi
    if [ "$guard_rc" -ne 0 ]; then return 1; fi

    local run_dir
    run_dir=$(ts_freshest_run_dir)
    if [ -z "$run_dir" ]; then
        echo "  SKIP: no target/*/build/tree-sitter-reify-*/output found"
        return 0
    fi

    local directives
    directives=$(cat "$run_dir/output")

    # Unchanged behaviour: the generation input stays watched.
    if [[ "$directives" != *"cargo:rerun-if-changed=grammar.js"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: no 'cargo:rerun-if-changed=grammar.js' in $run_dir/output"
        return 1
    fi

    # The defect this task closes.
    if [[ "$directives" != *"cargo:rerun-if-changed=src/scanner.c"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: no 'cargo:rerun-if-changed=src/scanner.c' in $run_dir/output"
        echo "  build.rs compiles src/scanner.c (c_config.file(\"src/scanner.c\")) and the cc"
        echo "  crate emits no rerun-if-changed directives of its own, so without this line"
        echo "  a scanner.c-only edit is never recompiled and never under test."
        return 1
    fi

    # Every TRACKED header, derived from git rather than hardcoded so a new
    # header cannot silently escape the watch set.
    local h rel
    while IFS= read -r h; do
        [ -n "$h" ] || continue
        rel="${h#tree-sitter-reify/}"
        if [[ "$directives" != *"cargo:rerun-if-changed=$rel"* ]]; then
            echo ""
            echo "  ASSERTION FAILED: no 'cargo:rerun-if-changed=$rel' in $run_dir/output"
            echo "  The header set must be enumerated by glob on both sides, not hardcoded."
            return 1
        fi
    done < <(cd "$REPO_ROOT" && git ls-files 'tree-sitter-reify/src/tree_sitter/*.h')

    # NEGATIVE: src/parser.c must stay UNWATCHED. build.rs WRITES it, so watching
    # it would make every build-script run dirty its own watch set — the double
    # execution documented in build.rs. Pinned so a future "fix" cannot
    # reintroduce it while chasing the scanner.c gap.
    if [[ "$directives" == *"cargo:rerun-if-changed=src/parser.c"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: src/parser.c is watched — build.rs writes it, so this"
        echo "  causes double execution of the build script on every build."
        return 1
    fi

    # ATTESTATION: the stamp must sit beside the archive it describes, and must be
    # byte-identical to what the shell computes. If these two manifests can drift,
    # every `check` verdict is meaningless.
    local archive="$run_dir/out/libtree_sitter_reify.a"
    local stamp="$run_dir/out/tree_sitter_inputs.stamp"
    assert_file_exists "$archive" || return 1
    assert_file_exists "$stamp" || return 1

    local fp
    fp=$(bash "$FRESHNESS_SCRIPT" --print-fingerprint)
    if [ "$(cat "$stamp")" != "$fp" ]; then
        echo ""
        echo "  ASSERTION FAILED: build.rs's stamp != scripts/tree-sitter-freshness.sh --print-fingerprint"
        echo "  --- stamp ($stamp) ---"; cat "$stamp"
        echo "  --- shell fingerprint ---"; echo "$fp"
        return 1
    fi
}

test_build_rs_stamps_a_real_manifest_on_a_shasum_only_host() {
    # The two halves of the attestation contract must agree on WHAT can hash —
    # asserted BEHAVIOURALLY, by running build.rs on a host where `sha256sum` is
    # unreachable and only `shasum` is, and reading what it actually stamped.
    #
    # WHY THIS SHAPE, and not the source-text guard it replaces (#5629 review
    # round 4). The outgoing `test_build_rs_hasher_matches_the_shell_fallbacks`
    # grepped build.rs and scripts/lib_portable.sh, and was MEASURED vacuous in
    # three independent ways:
    #   (a) `grep -A3 '"shasum"' build.rs | grep -q '256'` cannot catch a bare
    #       `shasum` — the matched line is the HASHERS declaration, and the
    #       literal `sha256sum` on that same line already supplies the `256`, so
    #       rewriting the entry to ("shasum", &[]) (SHA-1!) still PASSES;
    #   (b) the sed-extracted portable_sha256 body was substring-matched for both
    #       binary names, and that function's own error string "neither sha256sum
    #       nor shasum found on PATH." satisfies both iterations — so found=2
    #       survives deleting both `command -v` arms outright;
    #   (c) `grep -q` for a Rust string literal proves the literal appears
    #       somewhere in the file, never that `sha256_of` is reached.
    # It also violated this file's OWN established standard: this suite has twice
    # REFUSED a source-text assertion on exactly this reasoning and said so in
    # place — test_freshness_script_exists_and_executable declines to grep for
    # `set -euo pipefail` (and extracts and RUNS the helper instead), and
    # test_freshness_target_dir_override_is_a_clean_noop declines to grep for an
    # env-var name (and observes the value in a grandchild process instead). So
    # the guard is replaced by a behavioural one, not merely repaired.
    #
    # MASKING BY OMISSION, not by shadowing, is load-bearing: build.rs's
    # `try_hasher` maps ENOENT to Ok(None) (fall through to the next binary) but
    # a present-but-failing binary to Err(()) (retry). A failing sha256sum stub
    # would therefore exercise the RETRY path, not the FALLBACK path under test.
    #
    # NON-VACUITY IS MEASURED BY MUTATION: build.rs reduced to sha256sum-only
    # writes the literal UNAVAILABLE (caught by assertion 1), and ("shasum", &[])
    # — bare shasum, which is SHA-1 — writes 40-hex digests that fail assertions
    # 2 and 3. That second mutation is precisely what the outgoing guard missed.
    #
    # scripts/lib_portable.sh stays deliberately OUT of
    # scripts/verify-pipeline-infra-tests.txt: routing it would pull this whole
    # suite's serial cargo checks into any task-scope verify touching that shared
    # lib. The merge gate runs --include-infra, so this arm is always evaluated
    # there; the exposure is one task-scope run, not a coverage hole.

    # SKIP only on a PROVEN environment defect. `shasum` absent from the host
    # means the fallback under test cannot exist here at all.
    if ! command -v shasum >/dev/null 2>&1; then
        echo "  SKIP: shasum not on PATH — the shasum-only fallback cannot be exercised on this host"
        return 0
    fi

    local mask
    mask=$(ts_masked_path_dir sha256sum) || return 1

    # POSITIVELY assert the mask took effect before concluding anything from the
    # run. Without this the test degrades into exactly the vacuity it replaces:
    # a shim that quietly still resolves sha256sum would pass every assertion
    # below while proving nothing about the fallback.
    if env PATH="$mask" bash -c 'command -v sha256sum' >/dev/null 2>&1; then
        echo ""
        echo "  ASSERTION FAILED: sha256sum still resolves under the masked PATH ($mask)"
        echo "  — the fixture is not simulating a shasum-only host, so nothing below"
        echo "  would exercise build.rs's fallback."
        return 1
    fi
    if ! env PATH="$mask" bash -c 'command -v shasum' >/dev/null 2>&1; then
        echo ""
        echo "  ASSERTION FAILED: shasum is on the host PATH but NOT under the masked"
        echo "  PATH ($mask) — the shim dropped the very binary under test. That is a"
        echo "  broken fixture, not an unsupported host."
        return 1
    fi

    # Masked-toolchain preflight — the second PROVEN environment defect that may
    # skip. A bare cargo failure below is a HARD FAIL, never a skip.
    local pf
    pf=$(mktemp)
    CLEANUP_ACTIONS+=("rm -f '$pf'")
    if ! env PATH="$mask" cargo --version >"$pf" 2>&1 || ! env PATH="$mask" cc --version >>"$pf" 2>&1; then
        echo "  SKIP: masked toolchain preflight failed (cargo/cc unusable under the shim)"
        cat "$pf"
        return 0
    fi

    # Force a build-script re-run: grammar.js is watched, and touching it is
    # mtime-only, so the worktree stays clean and the digests are unchanged.
    touch "$TS_DIR/grammar.js"

    local cargo_out
    cargo_out=$(mktemp)
    CLEANUP_ACTIONS+=("rm -f '$cargo_out'")
    local guard_rc=0
    run_guarded_cargo_check "$cargo_out" env PATH="$mask" timeout 300 cargo check \
        -p tree-sitter-reify --manifest-path "$REPO_ROOT/Cargo.toml" || guard_rc=$?
    if [ "$guard_rc" -eq 2 ]; then return 0; fi
    if [ "$guard_rc" -ne 0 ]; then return 1; fi

    local run_dir
    run_dir=$(ts_freshest_run_dir)
    if [ -z "$run_dir" ]; then
        if [ -n "${CARGO_TARGET_DIR:-}" ]; then
            echo "  SKIP: CARGO_TARGET_DIR=$CARGO_TARGET_DIR redirects build output away from"
            echo "  $REPO_ROOT/target, so the freshly written stamp is not locatable here"
            return 0
        fi
        echo ""
        echo "  ASSERTION FAILED: cargo check succeeded but no"
        echo "  $REPO_ROOT/target/*/build/tree-sitter-reify-*/output exists — build.rs"
        echo "  cannot have run, so no stamp was written."
        return 1
    fi
    local stamp="$run_dir/out/tree_sitter_inputs.stamp"
    assert_file_nonempty "$stamp" || return 1

    local content
    content=$(cat "$stamp")

    # (1) A REAL manifest, not the no-hasher sentinel. UNAVAILABLE here is the
    # whole defect: the shell side computes a real fingerprint from shasum, so
    # every archive on such a host becomes permanently unattestable, `check`
    # prints PARTIAL and exits 0, and the guard is a silent no-op for that whole
    # checkout — propagating through CoW lane seeding.
    if [ "$content" = "UNAVAILABLE" ]; then
        echo ""
        echo "  ASSERTION FAILED: build.rs stamped the literal UNAVAILABLE on a"
        echo "  shasum-only host — it hashes with sha256sum only, while"
        echo "  scripts/tree-sitter-freshness.sh falls back to 'shasum -a 256'."
        echo "  Every archive here is then permanently unattestable."
        return 1
    fi

    # (2) Shape: one <64-hex><two spaces><relpath> line per --list-inputs path,
    # in that order. DERIVED from the script's own input enumeration, so the two
    # sides cannot drift. 40-hex digests (bare `shasum` is SHA-1) fail here.
    local -a inputs=()
    local rel
    while IFS= read -r rel; do
        [ -n "$rel" ] && inputs+=("$rel")
    done < <(bash "$FRESHNESS_SCRIPT" --list-inputs)
    if [ "${#inputs[@]}" -eq 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: --list-inputs produced no paths — this test derives its"
        echo "  expected manifest from that enumeration, so it cannot silently pass."
        return 1
    fi
    local -a lines=()
    local line
    while IFS= read -r line; do
        lines+=("$line")
    done <<< "$content"
    if [ "${#lines[@]}" -ne "${#inputs[@]}" ]; then
        echo ""
        echo "  ASSERTION FAILED: stamp has ${#lines[@]} lines, --list-inputs has ${#inputs[@]} paths"
        echo "  --- stamp ($stamp) ---"; cat "$stamp"
        echo "  --- --list-inputs ---"; printf '%s\n' "${inputs[@]}"
        return 1
    fi
    local i
    for i in "${!inputs[@]}"; do
        if [[ ! "${lines[$i]}" =~ ^([0-9a-f]{64})\ \ (.+)$ ]]; then
            echo ""
            echo "  ASSERTION FAILED: stamp line $((i + 1)) is not '<64 hex>  <relpath>':"
            echo "    ${lines[$i]}"
            echo "  A 40-hex digest here means build.rs invoked bare 'shasum' (SHA-1)"
            echo "  instead of 'shasum -a 256'."
            return 1
        fi
        if [ "${BASH_REMATCH[2]}" != "${inputs[$i]}" ]; then
            echo ""
            echo "  ASSERTION FAILED: stamp line $((i + 1)) names '${BASH_REMATCH[2]}',"
            echo "  --list-inputs line $((i + 1)) names '${inputs[$i]}' — the two halves of the"
            echo "  attestation enumerate different inputs."
            return 1
        fi
    done

    # (3) The cross-hasher agreement contract in its strongest form: what shasum
    # wrote INSIDE the mask must be byte-identical to what sha256sum computes
    # OUTSIDE it. Anything else and every `check` verdict is meaningless.
    if ! cmp -s "$stamp" <(bash "$FRESHNESS_SCRIPT" --print-fingerprint); then
        echo ""
        echo "  ASSERTION FAILED: the shasum-written stamp is not byte-identical to"
        echo "  scripts/tree-sitter-freshness.sh --print-fingerprint (sha256sum)."
        echo "  --- stamp ($stamp) ---"; cat "$stamp"
        echo "  --- shell fingerprint ---"; bash "$FRESHNESS_SCRIPT" --print-fingerprint
        return 1
    fi
}

test_freshness_detects_and_repairs_stale_archive() {
    # End-to-end: reproduce "verified against a stale compiled parser" deterministically,
    # then prove the force actually cures it.
    #
    # Only the GITIGNORED src/parser.c is ever content-mutated. Tracked files receive
    # at most an mtime-level `touch` from `ensure`, so a concurrent `git status` in the
    # lane (cleanliness guards, lane audit) can never observe a dirty tree — a far worse
    # failure than the bug under test.
    #
    # Because parser.c is IN the fingerprint but deliberately NOT watched, mutating it
    # reproduces exactly the reported shape: fresh sources on disk, stale archive linked.
    local parser="$TS_DIR/src/parser.c"
    assert_file_exists "$parser" || return 1

    # The backup lives OUTSIDE the tracked source tree. tree-sitter-reify/.gitignore
    # covers src/parser.c but NOT src/*.bak (`git check-ignore` exits 1 on one), so an
    # in-tree backup leaves `git status --short` reporting an untracked file for this
    # test's whole duration — up to 900 s across three serial cargo checks. The lane
    # cleanliness guards, the lane audit and scripts/land.sh all read that as a dirty
    # worktree: a worse, and far more confusing, failure than the bug under test.
    local bakdir backup
    bakdir=$(mktemp -d) || return 1
    backup="$bakdir/parser.c.orig"
    # Registered BEFORE the copy, so an interrupt in the gap leaks nothing. `mv -f` is
    # atomic and self-consuming — unlike a cp+rm pair, a failed restore cannot go on to
    # destroy the only copy (the CLEANUP_ACTIONS string is `;`-separated under
    # `eval ... || true`, so every segment runs regardless of the previous one's rc).
    # The restore is registered AHEAD of the `rm -rf`: cleanup replays in insertion
    # order, so the backup dir must not be removable before it has been restored from.
    # `mv` (not `cp -p`) also gives the restored file an mtime of now, matching the
    # test_auto_generation_rebuilds_parser precedent.
    #
    # The trailing touch-sweep leaves the lane's target/ consistent with the restored
    # tree whichever way this test exits EARLY (the SKIP returns on a busy/absent
    # toolchain, or any assertion failure): those paths leave the real archive built
    # from the PROBED parser.c, so once the original is restored that archive is
    # genuinely stale and something must make cargo recompile it.
    #
    # On the SUCCESS path the restore-and-rebuild block at the end of this test does
    # that job explicitly instead, and then ASSERTS the result. Relying on the sweep
    # alone was an unverified bet (#5629 amendment pass): the sweep only queues a
    # rebuild, and the dir this test's `cargo check -p tree-sitter-reify` advanced is
    # IN this run's window (its marker moved after `ensure` stamped the epoch), so
    # the plan's later `check` leaf hard-asserts on it. That stayed green only
    # because the subsequent `cargo nextest run --workspace` waves happened to
    # recompile the same fingerprint dir — an assumption about build-script
    # fingerprint identity across two different cargo invocations that nothing here
    # verified. If it ever stopped holding, this test would turn the merge gate RED
    # for a reason unrelated to the change under test.
    #
    # It deliberately does NOT run `bash "$FRESHNESS_SCRIPT" ensure` against the real
    # target/. `ts_mode_ensure` writes $TARGET_DIR/.tree-sitter-freshness.epoch
    # unconditionally as its FIRST action, and this suite runs mid-plan (run_all.sh,
    # well before the final `check` leaf). Re-stamping the epoch here would move the
    # run window forward past every dir the earlier cargo leaves already rebuilt,
    # demoting them to DORMANT `note:` lines and narrowing `check` back to the newest
    # dir only — the exact multi-archive false GREEN the run window was added to close.
    # Pointing `ensure` at a throwaway target dir is not a fix either: an empty target
    # scans as `none` and returns before ts_force_touch, so the repair would silently
    # not happen.
    #
    # So the repair is applied directly, as `ts_force_touch` would: bump the mtime
    # (content untouched) of grammar.js plus every compiled input, which is what makes
    # cargo re-run the build script. The set is DERIVED from `--list-inputs` rather
    # than restated, so a new header or a second .c cannot drift out of it; grammar.js
    # is added because it is an input to generation, not to compilation, and so is
    # absent from that list. Tracked files are only ever mtime-touched, never
    # content-mutated, so a concurrent `git status` still sees a clean tree.
    CLEANUP_ACTIONS+=("mv -f '$backup' '$parser'; ( cd '$TS_DIR' && { printf 'grammar.js\n'; bash '$FRESHNESS_SCRIPT' --list-inputs; } | xargs -r touch ) || true")
    CLEANUP_ACTIONS+=("rm -rf '$bakdir'")
    # `|| return 1`: set -e is disabled inside test bodies (the runner invokes them as
    # `if "$t"; then`), so an unchecked cp would fail silently and the probe append
    # below would then mutate parser.c with no usable copy to restore from.
    cp "$parser" "$backup" || return 1

    local tmp cargo_out
    tmp=$(mktemp -d);      CLEANUP_ACTIONS+=("rm -rf '$tmp'")
    cargo_out=$(mktemp);   CLEANUP_ACTIONS+=("rm -f '$cargo_out'")

    # ---- baseline: a built, attested archive ----
    local guard_rc=0
    run_guarded_cargo_check "$cargo_out" timeout 300 cargo check -p tree-sitter-reify \
        --manifest-path "$REPO_ROOT/Cargo.toml" || guard_rc=$?
    if [ "$guard_rc" -eq 2 ]; then return 0; fi
    if [ "$guard_rc" -ne 0 ]; then return 1; fi

    local dir_name
    dir_name=$(ts_mirror_run_dir "$tmp/t1") || {
        echo "  SKIP: no built tree-sitter-reify archive to mirror"
        return 0
    }
    run_ts_freshness check "$tmp/t1"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: check on a freshly built archive exited $TS_FRESHNESS_RC (expected 0)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- mutate the gitignored generated source; cargo will NOT recompile ----
    printf '\n/* task 5629 probe */\n' >> "$parser"
    guard_rc=0
    run_guarded_cargo_check "$cargo_out" timeout 300 cargo check -p tree-sitter-reify \
        --manifest-path "$REPO_ROOT/Cargo.toml" || guard_rc=$?
    if [ "$guard_rc" -eq 2 ]; then return 0; fi
    if [ "$guard_rc" -ne 0 ]; then return 1; fi

    dir_name=$(ts_mirror_run_dir "$tmp/t2") || return 1
    run_ts_freshness check "$tmp/t2"
    if [ "$TS_FRESHNESS_RC" -ne 1 ]; then
        echo ""
        echo "  ASSERTION FAILED: check did not detect the stale archive (exit $TS_FRESHNESS_RC, expected 1)"
        echo "  cargo reported success against an archive it never recompiled — the false GREEN."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"$dir_name"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: stale verdict does not name the fingerprint dir '$dir_name'"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- ensure must force a real recompile ----
    run_ts_freshness ensure "$tmp/t2"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: ensure exited $TS_FRESHNESS_RC (expected 0)"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
    if [[ "$TS_FRESHNESS_OUT" != *"forcing rebuild"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: ensure did not report forcing a rebuild"
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    guard_rc=0
    run_guarded_cargo_check "$cargo_out" timeout 300 cargo check -p tree-sitter-reify \
        --manifest-path "$REPO_ROOT/Cargo.toml" || guard_rc=$?
    if [ "$guard_rc" -eq 2 ]; then return 0; fi
    if [ "$guard_rc" -ne 0 ]; then return 1; fi

    ts_mirror_run_dir "$tmp/t3" >/dev/null || return 1
    run_ts_freshness check "$tmp/t3"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: check still STALE after ensure + cargo check (exit $TS_FRESHNESS_RC)"
        echo "  The force did not actually cause a recompile — which is the whole point."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi

    # ---- leave the real build tree PROVABLY fresh, and prove it ----
    # At this point the lane's archive was compiled from the PROBED parser.c, which
    # is about to stop existing. That dir's run marker advanced inside THIS verify
    # run's window, so the plan's post-cargo `check` leaf hard-asserts on it — and
    # leaving it stale here would fail the merge gate for a reason that has nothing
    # to do with the change under test. Restoring in CLEANUP_ACTIONS only QUEUES a
    # repair (a touch-sweep) and bets that a later nextest wave happens to recompile
    # this same fingerprint dir. Do it here instead, and assert the outcome.
    #
    # `cp`, not `mv`: the backup must survive for the registered cleanup, which
    # still runs and is now a no-op-equivalent re-restore. The sweep is applied the
    # way ts_force_touch would (mtime only, content untouched) rather than via
    # `ensure`, for the epoch reason spelled out at the top of this test.
    #
    # Non-vacuous: right now the archive attests the PROBED bytes, so restoring the
    # original makes it stale again; only a genuine recompile can make the final
    # check green.
    cp -f "$backup" "$parser" || return 1
    # The final check below is only meaningful if the bytes it attests are the ones
    # that SURVIVE this test. Without this guard, dropping the restore above would
    # leave the check comparing the probed archive against the probed source —
    # green, vacuous, and the lane left stale the moment cleanup restores.
    if ! cmp -s "$parser" "$backup"; then
        echo ""
        echo "  ASSERTION FAILED: src/parser.c was not restored before the final rebuild,"
        echo "  so the archive about to be attested is not the one this test leaves behind."
        return 1
    fi
    ( cd "$TS_DIR" && { printf 'grammar.js\n'; bash "$FRESHNESS_SCRIPT" --list-inputs; } \
        | xargs -r touch ) || return 1

    guard_rc=0
    run_guarded_cargo_check "$cargo_out" timeout 300 cargo check -p tree-sitter-reify \
        --manifest-path "$REPO_ROOT/Cargo.toml" || guard_rc=$?
    if [ "$guard_rc" -eq 2 ]; then return 0; fi
    if [ "$guard_rc" -ne 0 ]; then return 1; fi

    ts_mirror_run_dir "$tmp/t4" >/dev/null || return 1
    run_ts_freshness check "$tmp/t4"
    if [ "$TS_FRESHNESS_RC" -ne 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: this test left the lane's archive STALE on exit"
        echo "  (check exit $TS_FRESHNESS_RC). The dir it rebuilt is inside this verify run's"
        echo "  window, so the plan's later 'tree-sitter-freshness.sh check' leaf will"
        echo "  hard-assert on it and turn the gate RED for a reason unrelated to the"
        echo "  change under test."
        echo "$TS_FRESHNESS_OUT"
        return 1
    fi
}

test_freshness_leaves_are_ld_scrubbed() {
    # Both freshness leaves must be emitted through verify.sh's SCRUBBED tool
    # emitter (add_tool), i.e. the plan line itself must literally begin with
    # `export LD_LIBRARY_PATH="${REIFY_AMBIENT_LD_LIBRARY_PATH-}"; `.
    #
    # WHY: /opt/reify-deps/lib is a whole conda prefix that shadows hundreds of
    # system sonames and outranks DT_RUNPATH, so any non-cargo tool inheriting
    # it gets conda libraries substituted underneath it. verify.sh states the
    # rule at its add_tool() definition: "If the line reaches cargo at all, use
    # `add` ... Everything else (shell scripts, npm, git, node) uses `add_tool`".
    # scripts/tree-sitter-freshness.sh is pure shell (sha256sum/shasum/stat/
    # find/touch via scripts/lib.sh) — no cargo, no reify binary — so it is a
    # TOOL line and genuinely wants the scrub, exactly like its neighbour
    # ./scripts/tree-sitter-generate.sh.
    #
    # WHY THIS EXISTS IN ADDITION TO tests/infra/test_verify_ld_library_path_scope.sh:
    # that guard ENUMERATES plain-`add` call sites in verify.sh SOURCE and accepts
    # EITHER add_tool() OR a trailing `# ld-ok: <reason>` marker. A future edit
    # could satisfy it by bolting on a marker while silently dropping the scrub.
    # This test pins the SCRUB ITSELF, on emitted plan text, and it is routed from
    # this task's own artifacts (scripts/tree-sitter-freshness.sh ->
    # test_tree_sitter_pipeline.sh, see the infra-test map), so it also fires on a
    # task-scope verify that touches only the freshness script.
    #
    # Asserted on captured plan TEXT, never on verify.sh source text.
    local verify="$REPO_ROOT/scripts/verify.sh"
    assert_file_exists "$verify" || return 1

    # The literal head every add_tool() line carries. Spelled out here rather
    # than sourced from verify.sh so this file is an INDEPENDENT oracle. Note
    # the `${VAR-}` form is matched LITERALLY: _LD_SCRUB is single-quoted in
    # verify.sh precisely so the variable NAME, not a host-specific value, lands
    # in the plan and --print-plan stays a hermetic, host-independent oracle.
    local scrub='export LD_LIBRARY_PATH="${REIFY_AMBIENT_LD_LIBRARY_PATH-}"; '

    local action plan cmds line stripped
    local n_ensure n_check n_control
    for action in "all --profile debug --scope all --include-infra" \
                  "test --profile both --scope all --include-infra" \
                  "lint --scope all --include-infra" \
                  "typecheck --scope all"; do
        # Retrying, completeness-guarded capture — same rationale/pattern as
        # test_verify_plan_includes_freshness_after_generation. $action word-splits
        # inside the inner bash -c (its unquoted $2), so no outer SC2086 exposure.
        capture_print_plan plan "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
            bash -c 'exec bash "$1" $2 --print-plan 2>/dev/null' _ "$verify" "$action" || true
        if ! plan_capture_complete "$plan"; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' --print-plan capture truncated after retries"
            return 1
        fi
        # Reason over the COMMANDS BLOCK only — the environment preamble carries
        # commented-out lines that are not plan leaves.
        cmds="${plan#*# --- commands}"

        n_ensure=0
        n_check=0
        n_control=0
        # Here-string, not a pipe: the loop must run in THIS shell or the counters
        # below are lost to a subshell. Fork-free line walking for the same reason
        # ts_offset_of is (an EINTR surface can flip an assertion under load).
        while IFS= read -r line; do
            case "$line" in
                *"./scripts/tree-sitter-freshness.sh ensure"*) n_ensure=$(( n_ensure + 1 )) ;;
                *"./scripts/tree-sitter-freshness.sh check"*)  n_check=$(( n_check + 1 )) ;;
                # Control: an ESTABLISHED add_tool() site, one line above the first
                # freshness leaf. Asserting the scrub on IT proves `$scrub` still
                # matches what verify.sh actually emits, so a drift in _LD_SCRUB's
                # spelling reports as "the literal moved" rather than as a false
                # accusation against the freshness leaves.
                *"./scripts/tree-sitter-generate.sh"*)
                    n_control=$(( n_control + 1 ))
                    stripped="${line#"$scrub"}"
                    if [[ "$stripped" == "$line" ]]; then
                        echo ""
                        echo "  ASSERTION FAILED: the CONTROL line — an established add_tool() site —"
                        echo "  does not carry the scrub literal this test matches on:"
                        echo "  |$line"
                        echo "  |$scrub  <- expected prefix"
                        echo "  verify.sh's _LD_SCRUB has changed shape; update the literal in this"
                        echo "  test rather than reading the freshness assertions below as real."
                        return 1
                    fi
                    continue
                    ;;
                *) continue ;;
            esac
            stripped="${line#"$scrub"}"
            if [[ "$stripped" == "$line" ]]; then
                echo ""
                echo "  ASSERTION FAILED: verify.sh '$action' emits a tree-sitter-freshness leaf"
                echo "  that is NOT LD-scrubbed — it was added with plain add() instead of add_tool():"
                echo "  |$line"
                echo "  Expected the line to begin with the literal:"
                echo "  |$scrub"
                echo "  scripts/tree-sitter-freshness.sh is pure shell and never reaches cargo, so"
                echo "  /opt/reify-deps/lib (a conda prefix that outranks DT_RUNPATH) would be"
                echo "  substituted underneath sha256sum/stat/find/touch. A '# ld-ok:' marker does"
                echo "  NOT satisfy this test — the scrub itself is the property under assertion."
                return 1
            fi
        done <<< "$cmds"

        # NON-VACUITY: without this, a plan carrying zero freshness leaves — or a
        # renamed script — sails through the loop above having asserted nothing.
        if [ "$n_ensure" -eq 0 ] || [ "$n_check" -eq 0 ]; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' plan has no freshness leaf to assert on"
            echo "  (matched 'ensure' lines: $n_ensure, 'check' lines: $n_check) — the scrub"
            echo "  assertion above would be vacuous. Both leaves are required on every"
            echo "  RUN_RUST plan; see test_verify_plan_includes_freshness_after_generation."
            return 1
        fi
        if [ "$n_control" -eq 0 ]; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' plan has no './scripts/tree-sitter-generate.sh'"
            echo "  leaf to use as the scrub-literal control, so a drift in verify.sh's _LD_SCRUB"
            echo "  spelling would be indistinguishable from a genuinely unscrubbed freshness leaf."
            return 1
        fi
    done
}

test_verify_plan_includes_freshness_after_generation() {
    # The guard is only worth anything if the gate RUNS it, and runs it in the
    # right place. Two orderings are load-bearing and both are asserted here:
    #   generate BEFORE freshness — src/parser.c must be current on disk before
    #     it is fingerprinted, or the check compares against a stale input set;
    #   freshness BEFORE the first cargo leaf — a force applied after the
    #     compile repairs nothing (cargo has already linked the stale archive).
    # Asserted on the emitted plan, not on verify.sh source text.
    local verify="$REPO_ROOT/scripts/verify.sh"
    assert_file_exists "$verify" || return 1

    local action plan cmds off_gen off_fresh off_cargo
    for action in "all --profile debug --scope all --include-infra" \
                  "test --profile both --scope all --include-infra" \
                  "lint --scope all --include-infra" \
                  "typecheck --scope all"; do
        # Retrying, completeness-guarded capture — same rationale/pattern as
        # test_orchestrator_includes_generation. $action word-splits into flags
        # inside the inner bash -c (its unquoted $2), so no outer SC2086 exposure.
        capture_print_plan plan "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
            bash -c 'exec bash "$1" $2 --print-plan 2>/dev/null' _ "$verify" "$action" || true
        if ! plan_capture_complete "$plan"; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' --print-plan capture truncated after retries"
            return 1
        fi

        # Reason over the COMMANDS BLOCK only. The environment preamble carries
        # `# . $HOME/.cargo/env`, so an offset computed over the whole capture
        # could pick up a comment line rather than a real cargo leaf.
        cmds="${plan#*# --- commands}"

        if [[ "$cmds" != *"tree-sitter-freshness.sh"* ]]; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' plan has no tree-sitter-freshness leaf"
            return 1
        fi
        # `ensure` (self-healing), not bare and not `check` (hard-fail): the merge
        # gate must repair a repairable condition rather than refuse to run.
        if [[ "$cmds" != *"./scripts/tree-sitter-freshness.sh ensure"* ]]; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' freshness leaf is not invoked in 'ensure' mode"
            return 1
        fi

        off_gen=$(ts_offset_of "$cmds" "tree-sitter-generate")
        off_fresh=$(ts_offset_of "$cmds" "tree-sitter-freshness")
        off_cargo=$(ts_offset_of "$cmds" "cargo ")
        if [ "$off_gen" -lt 0 ]; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' plan has no tree-sitter-generate leaf"
            return 1
        fi
        if [ "$off_gen" -ge "$off_fresh" ]; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' runs freshness BEFORE generate"
            echo "  (generate offset $off_gen, freshness offset $off_fresh) —"
            echo "  parser.c would be fingerprinted before it is regenerated."
            return 1
        fi
        if [ "$off_cargo" -lt 0 ]; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' plan has no cargo leaf to order against"
            return 1
        fi
        if [ "$off_fresh" -ge "$off_cargo" ]; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' runs freshness AFTER the first cargo leaf"
            echo "  (freshness offset $off_fresh, first cargo offset $off_cargo) —"
            echo "  a force applied after the compile repairs nothing."
            return 1
        fi

        # The POST-CONDITION leaf (#5629 review round 2). `ensure` only ATTEMPTS
        # the repair: it bumps mtimes and trusts cargo to act on them, and never
        # fails for a condition it believes it fixed. So a plan carrying `ensure`
        # alone has no evidence the rebuild actually happened — if the force
        # failed to trigger one, the gate goes green having linked an archive it
        # never compiled. `check` after the cargo wave is what turns the attempt
        # into an assertion.
        #
        # Required on EVERY RUN_RUST plan, action=test included (#5629 amendment
        # pass). The earlier carve-out excused action=test on the grounds that it
        # has no compile leaf before this pole — but the leaf is emitted AFTER
        # add_test_passes, and on an action=test plan add_test_passes emits
        # `cargo nextest run --workspace`, which compiles the parser. So the
        # test-only tier forced a rebuild via `ensure` and then asserted nothing,
        # carrying the whole one-level-up false GREEN this leaf exists to close.
        # The ORDERING arm is what makes the pre-build hard-fail concern moot:
        # wherever the leaf exists it must come after EVERY cargo leaf.
        local off_check off_last_cargo
        off_check=$(ts_offset_of "$cmds" "tree-sitter-freshness.sh check")
        off_last_cargo=$(ts_last_offset_of "$cmds" "cargo ")
        if [ "$off_check" -lt 0 ]; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' has no post-cargo"
            echo "  './scripts/tree-sitter-freshness.sh check' leaf — the plan only ATTEMPTS"
            echo "  the repair and never asserts the archive cargo linked is fresh."
            return 1
        fi
        # Against the LAST cargo leaf, not the first (#5629 review round 3).
        # The first-cargo form passes for a `check` emitted right after the
        # clippy wave — but clippy compiles into a different fingerprint dir
        # than the debug/release nextest builds that follow, so such a leaf
        # attests an archive no test binary ever links while the archives that
        # ARE linked go unexamined. That is the same false GREEN one level up.
        if [ "$off_check" -le "$off_last_cargo" ]; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' runs the freshness CHECK leaf at"
            echo "  offset $off_check, before/at the LAST cargo leaf ($off_last_cargo)."
            echo "  A cargo leaf that compiles the parser runs after the assertion, so the"
            echo "  archive actually linked is never attested — and an assertion placed"
            echo "  before the build would also hard-fail the very staleness 'ensure' had"
            echo "  just queued a repair for."
            return 1
        fi
        if [ "$off_check" -le "$off_cargo" ]; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' runs the freshness CHECK leaf at"
            echo "  offset $off_check, before/at the first cargo leaf ($off_cargo)."
            echo "  A post-condition asserted before the build proves nothing, and would"
            echo "  hard-fail the very staleness 'ensure' had just queued a repair for."
            return 1
        fi
        # ...and it must be a DIFFERENT leaf from the ensure one, not a rename.
        if [ "$off_check" -le "$off_fresh" ]; then
            echo ""
            echo "  ASSERTION FAILED: verify.sh '$action' emits 'check' at or before the"
            echo "  'ensure' leaf — the pre-build repair and the post-build assertion are"
            echo "  both required, in that order."
            return 1
        fi
    done

    # Negative: a docs-only scope classifies RUN_RUST=0 and must keep ZERO
    # command leaves — the freshness leaf has to be RUN_RUST-guarded exactly as
    # the generate leaf is, or every docs-only landing grows a command.
    #
    # NON-VACUITY IS ASSERTED FIRST (#5629 review round 4). This arm used to
    # stage with `git add ... || true` and then assert nothing about the index.
    # MEASURED with the same fixture and NOTHING staged, the plan reads
    # `# scope decision — RUN_RUST=0 RUN_GUI=0 RUN_OCCT_GATE=0` and
    # `# (no commands — nothing to verify for this action/scope)` with ZERO
    # `tree-sitter-freshness` matches — so an EMPTY index satisfies BOTH
    # assertions below, and the RUN_RUST guarding of the two freshness leaves
    # could be completely broken while this arm stayed green. A fixture that
    # cannot stage is a broken fixture, not a passing test: the staging failure
    # is fatal now, and the index is inspected before the plan is reasoned about.
    local fix add_out staged
    fix=$(mk_verify_fixture) || return 1
    add_out=$(mktemp)
    CLEANUP_ACTIONS+=("rm -f '$add_out'")
    printf 'docs\n' > "$fix/docs/note.md"
    if ! git -C "$fix" add docs/note.md >"$add_out" 2>&1; then
        echo ""
        echo "  ASSERTION FAILED: could not stage docs/note.md in the fixture ($fix)"
        echo "  --- git add output ---"; cat "$add_out"; echo "  --- end output ---"
        return 1
    fi
    staged=$(git -C "$fix" diff --cached --name-only)
    if [[ "$staged" != *"docs/note.md"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: the fixture index does not contain docs/note.md, so this"
        echo "  arm would be asserting on a plan classified from an EMPTY staged set —"
        echo "  which yields RUN_RUST=0 and no commands whatever the guard does."
        echo "  --- staged ---"; printf '%s\n' "$staged"
        return 1
    fi
    capture_print_plan plan "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
        bash -c 'cd "$1" && exec bash scripts/verify.sh all --profile debug --scope staged --include-infra --print-plan 2>/dev/null' \
        _ "$fix" || true
    if ! plan_capture_complete "$plan"; then
        echo ""
        echo "  ASSERTION FAILED: docs-only --print-plan capture truncated after retries"
        return 1
    fi
    if [[ "$plan" != *"RUN_RUST=0"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: docs-only fixture did not classify RUN_RUST=0 — negative arm is void"
        echo "$plan"
        return 1
    fi
    cmds="${plan#*# --- commands}"
    if [[ "$cmds" == *"tree-sitter-freshness"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: docs-only (RUN_RUST=0) plan contains the freshness leaf"
        echo "  — it must be guarded on RUN_RUST like tree-sitter-generate is."
        return 1
    fi

    # POSITIVE CONTROL — the same fixture, one variable changed: a Rust file is
    # added to the index. Without it the negative arm above can still pass for
    # the trivial reason that nothing was classified at all; with it, this
    # fixture is PROVEN to emit both freshness leaves the moment RUN_RUST=1, so
    # the RUN_RUST=0 result is the guard working rather than the classifier
    # idling. Staged AFTER the docs-only arm has been asserted, so the two index
    # states cannot mask each other. mk_verify_fixture itself needs no change —
    # only the mkdir -p is new — and the capture reuses capture_print_plan /
    # plan_capture_complete exactly as the first one does, inheriting the same
    # retry-on-truncation guarantee.
    mkdir -p "$fix/crates/probe/src"
    printf 'pub fn probe() -> u8 {\n    0\n}\n' > "$fix/crates/probe/src/lib.rs"
    if ! git -C "$fix" add crates/probe/src/lib.rs >"$add_out" 2>&1; then
        echo ""
        echo "  ASSERTION FAILED: could not stage crates/probe/src/lib.rs in the fixture ($fix)"
        echo "  --- git add output ---"; cat "$add_out"; echo "  --- end output ---"
        return 1
    fi
    staged=$(git -C "$fix" diff --cached --name-only)
    if [[ "$staged" != *"crates/probe/src/lib.rs"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: the fixture index does not contain crates/probe/src/lib.rs,"
        echo "  so the positive control cannot classify RUN_RUST=1 and proves nothing."
        echo "  --- staged ---"; printf '%s\n' "$staged"
        return 1
    fi
    capture_print_plan plan "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
        bash -c 'cd "$1" && exec bash scripts/verify.sh all --profile debug --scope staged --include-infra --print-plan 2>/dev/null' \
        _ "$fix" || true
    if ! plan_capture_complete "$plan"; then
        echo ""
        echo "  ASSERTION FAILED: positive-control --print-plan capture truncated after retries"
        return 1
    fi
    if [[ "$plan" != *"RUN_RUST=1"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: staging a Rust file did not classify RUN_RUST=1, so the"
        echo "  negative arm above is not evidence of anything — this fixture never"
        echo "  reaches the state the freshness leaves are emitted in."
        echo "$plan"
        return 1
    fi
    cmds="${plan#*# --- commands}"
    if [[ "$cmds" != *"./scripts/tree-sitter-freshness.sh ensure"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: RUN_RUST=1 staged-scope plan has no"
        echo "  './scripts/tree-sitter-freshness.sh ensure' leaf — a Rust change would be"
        echo "  verified without the stale-archive repair the docs-only arm excuses."
        echo "$cmds"
        return 1
    fi
    if [[ "$cmds" != *"./scripts/tree-sitter-freshness.sh check"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: RUN_RUST=1 staged-scope plan has no"
        echo "  './scripts/tree-sitter-freshness.sh check' leaf — the repair would be"
        echo "  attempted and never asserted."
        echo "$cmds"
        return 1
    fi
}

test_freshness_guard_is_routed_by_infra_test_map() {
    # A guard nothing selects is dead on arrival. verify.sh's select_infra_tests()
    # (scripts/verify.sh:~1188) matches the changed-file list against the LEFT
    # column of scripts/verify-pipeline-infra-tests.txt by EXACT path and appends
    # the right column to SELECTED_INFRA_GLOBS. Without a row per artifact, a
    # task-scope verify of a build.rs or scanner.c edit selects zero infra tests
    # and never runs this file.
    local map="$REPO_ROOT/scripts/verify-pipeline-infra-tests.txt"
    assert_file_exists "$map" || return 1

    local want="tests/infra/test_tree_sitter_pipeline.sh"
    local artifact rc=0
    for artifact in "tree-sitter-reify/build.rs" \
                    "tree-sitter-reify/src/scanner.c" \
                    "scripts/tree-sitter-generate.sh" \
                    "scripts/tree-sitter-freshness.sh"; do
        # A row that points at a path which no longer exists is a silently
        # disabled route: select_infra_tests compares against real changed
        # files, so a rename orphans the row with no other signal.
        if [ ! -f "$REPO_ROOT/$artifact" ]; then
            echo ""
            echo "  ASSERTION FAILED: routed artifact does not exist on disk: $artifact"
            rc=1
            continue
        fi
        # Parse with select_infra_tests' OWN shape — the same comment/blank
        # filter and the same `read -r artifact glob` two-field split — so this
        # cannot pass on a row verify.sh would not actually parse (e.g. one
        # commented out, or with the fields transposed).
        local _line _a _g found=0
        while IFS= read -r _line; do
            read -r _a _g <<< "$_line"
            [ -n "$_a" ] || continue
            [ -n "$_g" ] || continue
            if [ "$_a" = "$artifact" ] && [ "$_g" = "$want" ]; then
                found=1
                break
            fi
        done < <(grep -v '^\s*#' "$map" | grep -v '^\s*$')
        if [ "$found" -ne 1 ]; then
            echo ""
            echo "  ASSERTION FAILED: no parseable row '$artifact -> $want' in"
            echo "  scripts/verify-pipeline-infra-tests.txt — a scoped edit to"
            echo "  $artifact would run no tree-sitter guard at all."
            rc=1
        fi
    done
    return "$rc"
}

test_freshness_refuses_partial_fingerprint_on_unhashable_input() {
    # A file that EXISTS but cannot be hashed must never yield a manifest line
    # with an empty hash field.
    #
    # The original defect: `hash=$(compute_sha256 "$abs" | awk '{print $1}')`
    # tested nothing.  Command substitution reports only awk's status, awk
    # succeeds on empty input, and `set -e` is disabled inside ts_fingerprint
    # because BOTH call sites invoke it in a `||` list.  So an unreadable mode —
    # or a transient fork/EMFILE failure spawning the hasher, the same trigger
    # build.rs's sha256_of already retries against — produced the line
    # "  src/scanner.c" and STILL exited 0.  Downstream that is not a cosmetic
    # blemish: `check` renders it as a STALE verdict naming a file that has not
    # changed (a spurious merge-gate RED asserting a defect that does not exist),
    # and `ensure` writes the corrupt fingerprint into the ledger, so every later
    # run mismatches it and pays another full ~5 MB parser.c force.
    #
    # The contract asserted here is the one this whole script exists to hold:
    # "changed" and "could not tell" must stay distinguishable.  Failing closed
    # with a named diagnostic is correct; a silent empty field is not.
    assert_file_exists "$FRESHNESS_SCRIPT" || return 1

    if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
        echo "  SKIP: no sha256sum/shasum on PATH — the guard degrades to UNAVAILABLE here"
        return 0
    fi

    local tmp
    tmp=$(mktemp -d)
    # chmod back first: a mode-0 file in the tree is fine for rm -rf (the DIR is
    # writable) but restoring keeps the cleanup robust under any umask/FS.
    CLEANUP_ACTIONS+=("chmod -R u+rw '$tmp' 2>/dev/null; rm -rf '$tmp'")

    local ts="$tmp/ts" target="$tmp/target"
    mkdir -p "$ts/src/tree_sitter" "$target"
    cp "$TS_DIR/grammar.js"    "$ts/grammar.js"
    provision_fixture_parser_c "$ts/src/parser.c" || return 1
    cp "$TS_DIR/src/scanner.c" "$ts/src/scanner.c"
    cp "$TS_DIR"/src/tree_sitter/*.h "$ts/src/tree_sitter/"

    # scanner.c is the sharpest choice: it is the hand-written input whose
    # staleness this task exists to close, so a false verdict about it is the
    # exact failure mode under test.
    chmod a-r "$ts/src/scanner.c"

    # root (and some FS/capability configurations) ignore the mode bit entirely,
    # which would make every assertion below vacuously pass. Detect and skip.
    if head -c 1 "$ts/src/scanner.c" >/dev/null 2>&1; then
        echo "  SKIP: cannot make a file unreadable here (running as root, or FS ignores the mode bit)"
        return 0
    fi

    local rc=0 out
    out=$(REIFY_TS_FRESHNESS_TS_DIR="$ts" REIFY_TS_FRESHNESS_TARGET_DIR="$target" \
        bash "$FRESHNESS_SCRIPT" --print-fingerprint 2>&1) || rc=$?

    if [ "$rc" -eq 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: '--print-fingerprint' exited 0 with an unhashable input"
        echo "  Expected a non-zero exit: an input that cannot be hashed is NOT a"
        echo "  successfully-computed fingerprint."
        echo "  --- captured output ---"; echo "$out"; echo "  --- end output ---"
        return 1
    fi

    # The load-bearing assertion: no manifest line may carry an empty hash field.
    # Checked with a bash-native regex over the captured text rather than `grep -q`
    # in a pipe — `grep -q` exits early and SIGPIPEs its writer, which under
    # `set -o pipefail` yields 141 and silently inverts the verdict.
    local line
    while IFS= read -r line; do
        # Manifest lines are '<64-hex><2 spaces><relpath>'. Anything that looks
        # like a manifest line (ends in a known relpath) but lacks the hash is the
        # corruption. Diagnostics are prose and never match.
        if [[ "$line" =~ ^[[:space:]]*(src/[^[:space:]]+)$ ]]; then
            echo ""
            echo "  ASSERTION FAILED: emitted a manifest line with an EMPTY hash field:"
            printf '  %q\n' "$line"
            echo "  An empty hash is indistinguishable from a real content change; it"
            echo "  reds the gate for an unchanged file and poisons the ledger."
            return 1
        fi
    done <<< "$out"

    # The diagnostic must NAME the offending file, matching the missing-input
    # branch's behaviour — an operator has to know which file to chmod.
    if [[ "$out" != *"scanner.c"* ]]; then
        echo ""
        echo "  ASSERTION FAILED: failure diagnostic does not name the unhashable file"
        echo "  --- captured output ---"; echo "$out"; echo "  --- end output ---"
        return 1
    fi

    # `ensure` must not record a fingerprint it could not compute. A poisoned
    # ledger is the persistent half of this defect: it survives the run and makes
    # every subsequent invocation re-force.
    local ensure_rc=0 ensure_out
    ensure_out=$(REIFY_TS_FRESHNESS_TS_DIR="$ts" REIFY_TS_FRESHNESS_TARGET_DIR="$target" \
        bash "$FRESHNESS_SCRIPT" ensure 2>&1) || ensure_rc=$?

    if [ "$ensure_rc" -eq 0 ]; then
        echo ""
        echo "  ASSERTION FAILED: 'ensure' exited 0 with an unhashable input"
        echo "  --- captured output ---"; echo "$ensure_out"; echo "  --- end output ---"
        return 1
    fi

    local ledger="$target/.tree-sitter-freshness.ledger"
    if [ -e "$ledger" ]; then
        echo ""
        echo "  ASSERTION FAILED: 'ensure' wrote a ledger from an uncomputable fingerprint:"
        echo "  $ledger"
        echo "  --- ledger content (cat -A) ---"; cat -A "$ledger"; echo "  --- end ---"
        return 1
    fi

    return 0
}


# --- Main ---
run_tests
