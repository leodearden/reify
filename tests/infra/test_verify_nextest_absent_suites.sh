#!/usr/bin/env bash
# tests/infra/test_verify_nextest_absent_suites.sh — regression guard for
# host-independence of the plan-oracle infra suites on a nextest-LESS host
# (task 5599).
#
# PROBLEM. scripts/verify.sh gracefully falls back to emitting `cargo test`
# instead of `cargo nextest run` when cargo-nextest is genuinely absent from
# PATH (plan header `nextest=0`). Several tests/infra plan-oracle suites used
# to hard-code the literal string `cargo nextest run` inside their `bash -c`
# assert bodies, so they FAILed spuriously on such a host — the assert is
# checking an ordering/precedence property of the emitted plan that holds
# identically on the cargo-test fallback path, not anything nextest-specific.
# Worse, several other asserts passed VACUOUSLY there (their grep matched
# nothing), silently testing nothing.
#
# This suite turns the previously-manual acceptance ritual into a mechanical
# check: it builds a nextest-absent environment ONCE and runs each covered
# suite under it, asserting each reaches test_summary with rc=0 AND reports
# "0 failed".
#
# WHY NOT THE NAIVE `PATH="$STUB:/usr/bin:/bin"` RECIPE. The obvious harness
# (stub `cargo` + fresh HOME + PATH cut down to /usr/bin:/bin) does yield
# nextest=0, but it strips ~/.cargo/bin WHOLESALE — and the `tree-sitter` CLI
# lives there too. That makes test_verify_semaphore_e2e.sh's suite-start
# ensure_tree_sitter_ready gate fail, so its Sections A/B/C/F1/H FAIL loudly
# ("tree-sitter artifacts not ready") for reasons that have nothing to do with
# nextest — 5 extra failures that all PASS on the normal host. Under that
# harness the acceptance target ("0 FAIL") is unreachable, and the confound is
# invisible unless you already know where tree-sitter lives.
#
# HARNESS ACTUALLY USED (single-variable: "cargo-nextest is not installed",
# and nothing else):
#   (1) a symlink farm mirroring ~/.cargo/bin MINUS cargo-nextest;
#   (2) PATH = farm : (the real PATH with the ~/.cargo/bin element filtered
#       out), so the rest of the toolchain — notably tree-sitter — still
#       resolves;
#   (3) HOME = a temp dir, so verify.sh's apply_env() finds no
#       $HOME/.cargo/env to source (sourcing it would re-prepend the real
#       ~/.cargo/bin and un-hide cargo-nextest);
#   (4) CARGO_HOME deliberately NOT exported — cargo resolves `cargo-<subcmd>`
#       from $CARGO_HOME/bin in ADDITION to PATH, so pointing it at the real
#       ~/.cargo makes cargo-nextest reappear and flips the header back to
#       nextest=1 (observed).
#
# NON-VACUITY SELF-CHECKS. Before covering any suite, the harness is checked
# against itself: cargo-nextest must be genuinely unreachable under it, `cargo`
# and `tree-sitter` must both still resolve, the plan header under it must read
# nextest=0, and the plan header WITHOUT it must read nextest=1. Without these
# a broken harness (e.g. one that no longer hides cargo-nextest) would let this
# whole suite pass while simulating nothing at all.
#
# Modeled on tests/infra/test_verify_nextest_probe.sh:79-140 (temp HOME + PATH
# shim dir + cleanup EXIT trap) — but substituting the symlink farm for the
# bare stub dir, per the tree-sitter confound above.
#
# Compile-free with respect to THIS file's own harness (verify.sh --print-plan
# is pure bash string-building); the nested suites do whatever they already do.
#
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh); registered in
# tests/infra/run-all-classification.manifest as `intra-run-serial`, because it
# nests test_verify_semaphore_e2e.sh which is itself intra-run-serial (it
# mutates lane-shared state: working-tree parser.c, CoW target/). Running this
# file from a `pool` member would let that mutation race other pool members.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
VERIFY="$REPO_ROOT/scripts/verify.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== plan-oracle infra suites are host-independent on a nextest-less host (task 5599) ==="

# ---------------------------------------------------------------------------
# Harness construction (once, at suite start)
# ---------------------------------------------------------------------------

CARGO_BIN_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"
[ -d "$CARGO_BIN_DIR" ] || {
    echo "ERROR: cargo bin dir not found at $CARGO_BIN_DIR — cannot build the"
    echo "       nextest-absent symlink farm without something to mirror."
    exit 1
}

NX_WORKDIR="$(mktemp -d)"
NX_FARM="$NX_WORKDIR/cargo-bin-farm"
NX_HOME="$NX_WORKDIR/home"
mkdir -p "$NX_FARM" "$NX_HOME"
trap 'rm -rf "$NX_WORKDIR"' EXIT

# (1) Mirror every ~/.cargo/bin entry into the farm EXCEPT cargo-nextest.
for _entry in "$CARGO_BIN_DIR"/*; do
    [ -e "$_entry" ] || continue          # unexpanded glob on an empty dir
    _base="$(basename "$_entry")"
    [ "$_base" = "cargo-nextest" ] && continue
    ln -s "$_entry" "$NX_FARM/$_base"
done
unset _entry _base

# (2) PATH = farm : (real PATH minus the ~/.cargo/bin element).
_filtered_path=""
while IFS= read -r _p; do
    [ -z "$_p" ] && continue
    [ "$_p" = "$CARGO_BIN_DIR" ] && continue
    _filtered_path="${_filtered_path:+$_filtered_path:}$_p"
done < <(printf '%s\n' "$PATH" | tr ':' '\n')
NX_PATH="$NX_FARM:$_filtered_path"
unset _p _filtered_path

# nx_run <cmd...> — run a command under the nextest-absent environment.
# HOME is redirected (3) and CARGO_HOME is deliberately left unset (4).
nx_run() {
    env -u CARGO_HOME HOME="$NX_HOME" PATH="$NX_PATH" "$@"
}

# ---------------------------------------------------------------------------
# Non-vacuity self-checks — these are what stop this suite from passing by
# simulating nothing.
# ---------------------------------------------------------------------------

echo ""
echo "--- H: the nextest-absent harness genuinely simulates a nextest-less host ---"

# nx_which <name> — resolve <name> on the harness PATH, printing the resolved
# path. `command` is a SHELL BUILTIN, so it must be run via `bash -c` under
# nx_run; `nx_run command -v foo` would hand "command" to env(1) as an
# executable name and fail unconditionally — which would make the H1 absence
# check pass VACUOUSLY (exactly the failure mode these self-checks exist to
# rule out).
nx_which() { nx_run bash -c 'command -v "$1"' _ "$1"; }

_h1_check() { ! nx_which cargo-nextest; }
_h2_check() { nx_which cargo; }
_h3_check() { nx_which tree-sitter; }

# The plan is captured WHOLE and the header extracted afterwards (rather than
# piping verify.sh straight into `head -1`), so verify.sh never takes SIGPIPE
# — under `set -o pipefail` that would surface as a spurious pipeline failure.
_plan_header_under_harness() {
    local full
    full="$(nx_run bash "$VERIFY" test --scope all --print-plan 2>/dev/null)"
    printf '%s\n' "$full" | sed -n '1p'
}
_plan_header_ambient() {
    local full
    full="$(bash "$VERIFY" test --scope all --print-plan 2>/dev/null)"
    printf '%s\n' "$full" | sed -n '1p'
}

_h4_check() {
    local hdr
    hdr="$(_plan_header_under_harness)"
    printf '%s\n' "$hdr"
    case "$hdr" in *"nextest=0"*) return 0 ;; *) return 1 ;; esac
}
_h5_check() {
    local hdr
    hdr="$(_plan_header_ambient)"
    printf '%s\n' "$hdr"
    case "$hdr" in *"nextest=1"*) return 0 ;; *) return 1 ;; esac
}

# ---------------------------------------------------------------------------
# Covered-suite checker
# ---------------------------------------------------------------------------

# _suite_is_clean_without_nextest <basename> — run tests/infra/<basename>
# under the nextest-absent env and succeed ONLY if BOTH the exit rc is 0 AND
# the final "Results:" line reports "0 failed". On failure, echo the captured
# `FAIL:` lines (and the Results line) so assert()'s tail-50 dump names the
# offending asserts rather than just reporting a bare non-zero rc.
_suite_is_clean_without_nextest() {
    local basename="$1"
    local suite="$SCRIPT_DIR/$basename"
    local out rc results

    [ -f "$suite" ] || {
        echo "ERROR: covered suite not found at $suite"
        return 1
    }

    set +e
    out="$(nx_run bash "$suite" 2>&1)"
    rc=$?
    set -e

    results="$(printf '%s\n' "$out" | grep -E '^Results:' | tail -1)"

    if [ "$rc" -eq 0 ] && printf '%s\n' "$results" | grep -q '0 failed'; then
        echo "$basename: rc=$rc  $results"
        return 0
    fi

    echo "$basename FAILED under the nextest-absent harness: rc=$rc"
    echo "  ${results:-(no Results: line — suite aborted before test_summary)}"
    printf '%s\n' "$out" | grep -E '^\s*FAIL:' || true
    return 1
}

assert "H1: cargo-nextest is NOT resolvable under the nextest-absent harness env" \
    _h1_check

assert "H2: cargo IS still resolvable under the harness env (farm keeps the toolchain)" \
    _h2_check

assert "H3: tree-sitter IS still resolvable under the harness env (not stripped with ~/.cargo/bin)" \
    _h3_check

assert "H4: verify.sh plan header reads nextest=0 UNDER the harness" \
    _h4_check

assert "H5: verify.sh plan header reads nextest=1 WITHOUT the harness (this host has cargo-nextest, so the simulation is meaningful)" \
    _h5_check

# ---------------------------------------------------------------------------
# S: the covered plan-oracle suites are clean on a nextest-less host
# ---------------------------------------------------------------------------

echo ""
echo "--- S: covered suites reach test_summary with rc=0 / 0 FAIL without cargo-nextest ---"

assert "S1: test_verify_compile_gate.sh reaches test_summary with rc=0 / 0 FAIL on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_compile_gate.sh

assert "S2: test_verify_semaphore_wiring.sh reaches test_summary with rc=0 / 0 FAIL on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_semaphore_wiring.sh

assert "S3: test_verify_offline_partition.sh reaches test_summary with rc=0 / 0 FAIL on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_offline_partition.sh

# S4 is the reason the harness uses a symlink farm rather than the naive
# PATH="$STUB:/usr/bin:/bin" recipe (see the header). test_verify_semaphore_e2e.sh
# gates Sections A/B/C/F1/H behind ensure_tree_sitter_ready, and the tree-sitter
# CLI lives in ~/.cargo/bin alongside cargo-nextest — stripping that directory
# wholesale would add 5 "tree-sitter artifacts not ready" failures that have
# nothing to do with nextest, making "0 FAIL" unreachable. H3 above pins that
# tree-sitter still resolves under the harness, so a regression in the farm
# surfaces there rather than as a confusing failure here.
assert "S4: test_verify_semaphore_e2e.sh reaches test_summary with rc=0 / 0 FAIL on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_semaphore_e2e.sh

test_summary
