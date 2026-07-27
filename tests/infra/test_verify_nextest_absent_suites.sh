#!/usr/bin/env bash
# tests/infra/test_verify_nextest_absent_suites.sh — regression guard for
# host-independence of FOUR NAMED plan-oracle infra suites on a nextest-LESS
# host (task 5599):
#
#     tests/infra/test_verify_compile_gate.sh
#     tests/infra/test_verify_semaphore_wiring.sh
#     tests/infra/test_verify_offline_partition.sh
#     tests/infra/test_verify_semaphore_e2e.sh
#
# Those four, and ONLY those four — see "NOT COVERED HERE" below. This file
# does NOT claim that `tests/infra/run_all.sh` as a whole is green without
# cargo-nextest.
#
# PROBLEM. scripts/verify.sh gracefully falls back to emitting `cargo test`
# instead of `cargo nextest run` when cargo-nextest is genuinely absent from
# PATH (plan header `nextest=0`). The four suites above used to hard-code the
# literal string `cargo nextest run` inside their `bash -c` assert bodies, so
# they FAILed spuriously on such a host — the assert is checking an
# ordering/precedence property of the emitted plan that holds identically on
# the cargo-test fallback path, not anything nextest-specific. Worse, several
# other asserts passed VACUOUSLY there (their grep matched nothing), silently
# testing nothing.
#
# This suite turns the previously-manual acceptance ritual into a mechanical
# check: it builds a nextest-absent environment ONCE and runs each covered
# suite under it, asserting each reaches test_summary with rc=0, reports
# "0 failed", AND still runs at least its pinned floor of asserts (so a future
# change that guards coverage away instead of fixing it fails loudly rather
# than reporting a vacuous green — see _suite_is_clean_without_nextest).
#
# NOT COVERED HERE (measured under this exact harness at task/5599
# HEAD=7adf5995f2; both files are outside task 5599's module locks, so they
# could not be fixed there — follow-up filed as ticket
# tkt_0RRQHFMP224R3KNR2572TKCG3J):
#   tests/infra/test_verify_scope.sh          rc=1, 133 passed, 10 failed
#   tests/infra/test_verify_failfast_order.sh rc=1,  38 passed,  2 failed
# Two more plan-oracle suites are GREEN under the harness but were not audited
# for the vacuity class above, because at least some of their nextest-string
# assertions are NEGATIVE greps: tests/infra/test_occt_gated_scope.sh (49/0)
# and tests/infra/test_release_mode_in_test_command.sh (9/0). All four are in
# the follow-up ticket's scope; when one is fixed, add it as a new S-row here.
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
#   (5) RUSTUP_HOME, by contrast, IS carried across, resolved once while HOME
#       is still the real home. On a rustup host `cargo` is a symlink to
#       `rustup`, which derives its toolchain store from $RUSTUP_HOME and falls
#       back to $HOME/.rustup — so (3) alone strands the shim and it downloads
#       a whole fresh toolchain on the first cargo invocation (measured: 935 MB
#       into the temp HOME within 12 seconds, `cargo --version` not yet done).
#       This does NOT weaken the simulation: cargo-nextest is a standalone
#       binary in ~/.cargo/bin, not under ~/.rustup, so preserving the toolchain
#       store cannot un-hide it — H1 and H4 pin that it stays hidden. See
#       H6/H7, which fail if this is ever dropped again.
#
# NON-VACUITY SELF-CHECKS. Before covering any suite, the harness is checked
# against itself: cargo-nextest must be genuinely unreachable under it, `cargo`
# and `tree-sitter` must both still RUN under it (executability, not merely
# `command -v` resolvability — a harness where cargo resolves but cannot
# actually execute would be simulating "the toolchain is broken" rather than
# the intended single variable), the plan header under it must read nextest=0,
# the plan header WITHOUT it must read nextest=1, and the harness must not have
# perturbed the toolchain enough to provoke a rustup toolchain sync into its
# temp HOME. Without these a broken harness (e.g. one that no longer hides
# cargo-nextest) would let this whole suite pass while simulating nothing at
# all — or would "work" only by breaking something other than nextest.
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

echo "=== the four covered plan-oracle infra suites are host-independent on a nextest-less host (task 5599) ==="

# ---------------------------------------------------------------------------
# Harness construction (once, at suite start)
# ---------------------------------------------------------------------------

CARGO_BIN_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"

# HOST PRECONDITION (skip, do NOT fail). Without a ~/.cargo/bin to mirror there
# is nothing to build the symlink farm from. That is a property of the HOST,
# not a defect in the code under test, so it must not surface as a red suite —
# and it especially must not `exit 1` BEFORE test_summary, which would leave
# run_all.sh (or any suite nesting this one the way this one nests others) with
# a non-zero rc and no "Results:" line at all, i.e. indistinguishable from a
# genuine mid-suite abort. Emit an explicit SKIP and a clean summary instead.
if [ ! -d "$CARGO_BIN_DIR" ]; then
    echo ""
    echo "SKIP: harness unavailable on this host (cargo bin dir not found at" \
         "$CARGO_BIN_DIR — nothing to mirror into the nextest-absent symlink farm)"
    echo "      Reporting a clean summary: this is a host limitation, not a"
    echo "      defect in the suites under test."
    test_summary
    exit 0
fi

# Is cargo-nextest installed AMBIENTLY? If not, this host IS already a
# nextest-less host: the harness still works (and the S-section coverage below
# is arguably more direct), but H5 — "the plan header WITHOUT the harness reads
# nextest=1", the check that pins the simulation as MEANINGFUL here — cannot
# hold, and asserting it would go RED with a failure that says nothing about
# the property under test. So H5 is asserted only in the branch where
# cargo-nextest really is installed, and skipped with a reason otherwise.
NX_AMBIENT_HAS_NEXTEST=0
if command -v cargo-nextest >/dev/null 2>&1; then
    NX_AMBIENT_HAS_NEXTEST=1
fi

NX_WORKDIR="$(mktemp -d)"
NX_FARM="$NX_WORKDIR/cargo-bin-farm"
NX_HOME="$NX_WORKDIR/home"
mkdir -p "$NX_FARM" "$NX_HOME"
# INT/TERM/HUP as well as EXIT: verify.sh wraps each selected infra test in
# `timeout --kill-after=60 <n>m` and run_all.sh applies a 30m cap, so an outer
# timeout kill would otherwise leak the whole temp tree.
trap 'rm -rf "$NX_WORKDIR"' EXIT INT TERM HUP

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

# (5) Resolve RUSTUP_HOME ONCE, HERE — while $HOME is still the REAL home.
# Capturing it into a variable rather than inlining the expansion in the env
# line below is deliberate: inline, the ${RUSTUP_HOME:-$HOME/.rustup} default
# would be read against whichever HOME the reader assumes is in scope, and the
# whole point is that it must be the real one, not the redirect.
#
# Set only when the resolved store actually exists, so a non-rustup host
# (distro-packaged cargo, no ~/.rustup) is left completely unperturbed. Either
# form is safe — a non-rustup cargo ignores the variable — but this keeps the
# harness's footprint to exactly what the host needs.
NX_RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
[ -d "$NX_RUSTUP_HOME" ] || NX_RUSTUP_HOME=""

# nx_run <cmd...> — run a command under the nextest-absent environment.
# HOME is redirected (3), CARGO_HOME is deliberately left unset (4), and
# RUSTUP_HOME is carried across so the rustup shim is not stranded (5).
nx_run() {
    if [ -n "$NX_RUSTUP_HOME" ]; then
        env -u CARGO_HOME RUSTUP_HOME="$NX_RUSTUP_HOME" \
            HOME="$NX_HOME" PATH="$NX_PATH" "$@"
    else
        env -u CARGO_HOME HOME="$NX_HOME" PATH="$NX_PATH" "$@"
    fi
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

# H1 checks ABSENCE, which is correctly expressed as non-resolvability.
_h1_check() { ! nx_which cargo-nextest; }

# H2/H3 check PRESENCE, and for that `command -v` is too weak: a harness that
# perturbs more than intended can leave a tool resolvable-but-unrunnable (a
# dangling symlink, a shim whose backing toolchain the harness has stranded),
# and the suite would then be simulating "the toolchain is broken" instead of
# the single intended variable "cargo-nextest is absent" — the vacuity class
# this section exists to rule out. So both are EXECUTED. `env` performs its own
# PATH lookup with the environment it sets, so `nx_run <tool> --version`
# subsumes the resolvability check rather than replacing it.
_h2_check() { nx_run cargo --version; }
_h3_check() { nx_run tree-sitter --version; }

# H6/H7 — the harness must not provoke a rustup TOOLCHAIN SYNC.
#
# On a rustup host `~/.cargo/bin/cargo` is a symlink to `rustup`, and rustup
# derives its store from $RUSTUP_HOME, falling back to $HOME/.rustup when that
# is unset. The harness redirects HOME (element (3) above) — so unless it also
# carries RUSTUP_HOME across, the shim finds no toolchain under the redirected
# HOME and downloads a fresh one on the FIRST cargo invocation. That is not a
# hypothetical: measured on this host, a bounded 12-second probe of
# `cargo --version` under a RUSTUP_HOME-less harness wrote 935 MB into the temp
# HOME and had still not printed a version when it was killed.
#
# Two separate asserts because they fail differently. H6 names the mechanism
# exactly (a .rustup store appeared where none should be) and is the assert
# that pins THIS defect; H7 is the blunt backstop that catches any other way
# the harness might start writing hundreds of megabytes into a directory it
# advertises as a throwaway.
#
# H7's ceiling: on a correctly-isolated harness the temp HOME holds 4 KB after
# H2-H4 (measured) — the ceiling is ~12000x that, so it will not flap on
# incidental dotfile writes, while still tripping within the first second of a
# toolchain sync.
NX_HOME_MAX_KB=51200

_h6_check() {
    if [ -e "$NX_HOME/.rustup" ]; then
        echo "$NX_HOME/.rustup EXISTS — the harness stranded rustup and provoked"
        echo "a toolchain sync. cargo is a rustup shim and rustup resolves its"
        echo "store from \$RUSTUP_HOME, defaulting to \$HOME/.rustup; the harness"
        echo "redirects HOME, so RUSTUP_HOME must be carried across explicitly."
        echo "Contents:"
        ls -la "$NX_HOME/.rustup" 2>&1 | head -20
        return 1
    fi
    echo "no $NX_HOME/.rustup — the harness did not provoke a toolchain sync"
    return 0
}

_h7_check() {
    local kb
    kb="$(du -sk "$NX_HOME" 2>/dev/null | awk 'NR==1 {print $1}')"
    if [ -z "$kb" ]; then
        echo "could not measure the temp HOME size at $NX_HOME"
        return 1
    fi
    echo "temp HOME $NX_HOME holds ${kb} KB (ceiling ${NX_HOME_MAX_KB} KB)"
    if [ "$kb" -gt "$NX_HOME_MAX_KB" ]; then
        echo "-> the harness is writing a large amount into the throwaway HOME it"
        echo "   redirects to. It is perturbing more than the single intended"
        echo "   variable (cargo-nextest absent). Largest entries:"
        du -sk "$NX_HOME"/* "$NX_HOME"/.[!.]* 2>/dev/null | sort -rn | head -10
        return 1
    fi
    return 0
}

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

# _suite_is_clean_without_nextest <basename> <pass-floor> — run
# tests/infra/<basename> under the nextest-absent env and succeed ONLY if ALL
# THREE hold: the exit rc is 0, the final "Results:" line reports 0 failures,
# and it reports at least <pass-floor> passing asserts.
#
# WHY THE FLOOR. rc=0 + "0 failed" alone says nothing about how many asserts
# actually RAN. A covered suite fixes its host-dependence either by widening a
# grep (coverage preserved) or by wrapping the assert in a NEXTEST_AVAILABLE
# guard (coverage deliberately dropped on this path) — and nothing stops a
# future edit from widening a guard until it wraps the whole suite. That suite
# would still report "rc=0, 0 failed" while checking nothing, which is exactly
# the vacuity failure mode this file exists to prevent. The floor is the
# measured nextest-less pass count at the time of writing; a drop below it
# fails loudly. It is a FLOOR, not equality, so legitimately ADDING asserts to
# a covered suite does not require touching this file.
#
# WHY THE COUNTS ARE PARSED RATHER THAN grep'd. `grep -q '0 failed'` is an
# unanchored substring match: "Results: 5 passed, 10 failed" contains the
# substring "0 failed" and would pass. Today the rc=0 conjunct masks that
# (test_summary exits 1 whenever FAIL>0), but the failure-count check is
# precisely the check that must still hold in the case where a suite reports
# failures yet exits 0 anyway — a suite that stops calling test_summary, or
# whose rc gets swallowed. So both numbers are extracted from the anchored,
# whole-line shape that test_helpers.sh:63 emits and compared numerically. A
# Results line that does not match that shape leaves both empty, which the
# -n guards below reject rather than silently reading as 0.
#
# On failure, echo the captured `FAIL:` lines (and the Results line) so
# assert()'s tail-50 dump names the offending asserts rather than just
# reporting a bare non-zero rc.
_suite_is_clean_without_nextest() {
    local basename="$1"
    local floor="$2"
    local suite="$SCRIPT_DIR/$basename"
    local out rc results passed failed

    [ -f "$suite" ] || {
        echo "ERROR: covered suite not found at $suite"
        return 1
    }

    set +e
    out="$(nx_run bash "$suite" 2>&1)"
    rc=$?
    set -e

    results="$(printf '%s\n' "$out" | grep -E '^Results:' | tail -1)"
    passed="$(printf '%s\n' "$results" \
        | sed -n 's/^Results: \([0-9]\{1,\}\) passed, \([0-9]\{1,\}\) failed$/\1/p')"
    failed="$(printf '%s\n' "$results" \
        | sed -n 's/^Results: \([0-9]\{1,\}\) passed, \([0-9]\{1,\}\) failed$/\2/p')"

    if [ "$rc" -eq 0 ] && [ -n "$passed" ] && [ -n "$failed" ] \
       && [ "$failed" -eq 0 ] && [ "$passed" -ge "$floor" ]; then
        echo "$basename: rc=$rc  $results  (>= floor $floor)"
        return 0
    fi

    echo "$basename FAILED under the nextest-absent harness: rc=$rc (pass floor $floor)"
    echo "  ${results:-(no Results: line — suite aborted before test_summary)}"
    if [ -z "$passed" ] || [ -z "$failed" ]; then
        echo "  -> the Results line does not match the canonical"
        echo "     'Results: <N> passed, <M> failed' shape from test_helpers.sh"
    elif [ "$failed" -ne 0 ]; then
        echo "  -> $failed assert(s) failed on the nextest-less path"
    elif [ "$passed" -lt "$floor" ]; then
        echo "  -> COVERAGE SHRANK: only $passed assert(s) ran, floor is $floor."
        echo "     The suite is green but is checking LESS than it used to on a"
        echo "     nextest-less host — a guard was widened instead of a grep."
        echo "     Fix the suite, or re-pin the floor here with the reason."
    fi
    printf '%s\n' "$out" | grep -E '^\s*FAIL:' || true
    return 1
}

assert "H1: cargo-nextest is NOT resolvable under the nextest-absent harness env" \
    _h1_check

assert "H2: cargo still RUNS under the harness env (farm keeps the toolchain intact, not merely on PATH)" \
    _h2_check

assert "H3: tree-sitter still RUNS under the harness env (not stripped with ~/.cargo/bin)" \
    _h3_check

assert "H4: verify.sh plan header reads nextest=0 UNDER the harness" \
    _h4_check

# H5 is conditional on the host actually having cargo-nextest — see the
# NX_AMBIENT_HAS_NEXTEST comment above. test_helpers.sh has no SKIP counter, so
# a skipped assert simply does not increment PASS (same convention as the
# guarded regions in test_verify_offline_partition.sh).
if [ "$NX_AMBIENT_HAS_NEXTEST" -eq 1 ]; then
    assert "H5: verify.sh plan header reads nextest=1 WITHOUT the harness (this host has cargo-nextest, so the simulation is meaningful)" \
        _h5_check
else
    echo "  SKIP: H5 (harness unavailable on this host: cargo-nextest is not installed"
    echo "        ambiently, so there is nothing for the farm to hide and 'nextest=1"
    echo "        without the harness' cannot hold. The S-section below still runs —"
    echo "        against a genuinely nextest-less host rather than a simulated one.)"
fi

# H6/H7 are asserted LAST in this section, so that every check which actually
# exercises the harness (H2, H3 and H4 — H5 is deliberately ambient) has
# already run and any toolchain sync they would provoke has had its chance to
# land. H4's plan capture already runs verify.sh under the harness, so these
# two cost nothing beyond a stat and a du.
assert "H6: the harness did NOT provoke a rustup toolchain sync (no .rustup in its temp HOME)" \
    _h6_check

assert "H7: the harness's temp HOME is still small (it perturbs only cargo-nextest's visibility)" \
    _h7_check

# ---------------------------------------------------------------------------
# S: the covered plan-oracle suites are clean on a nextest-less host
# ---------------------------------------------------------------------------

echo ""
echo "--- S: covered suites reach test_summary with rc=0 / 0 FAIL / >= pass floor without cargo-nextest ---"

# PASS FLOORS — the nextest-less pass count measured for each suite at
# task/5599 HEAD=7adf5995f2 (see _suite_is_clean_without_nextest for why a
# bare "0 failed" is not sufficient). Where the floor is BELOW the suite's
# ambient nextest-ful count, the difference is the asserts deliberately
# guarded away as nextest-only, and the delta is recorded here so a further
# shrink is visible as a diff to this table rather than as silence:
#   compile_gate      35 nextest-less / 35 ambient  (all recovered by widening)
#   semaphore_wiring  22 nextest-less / 22 ambient  (all recovered by widening)
#   offline_partition 30 nextest-less / 35 ambient  (5 guarded: -E heavy-filter
#                                                    asserts, no fallback shape)
#   semaphore_e2e     65 nextest-less / 65 ambient  (1 guarded --config-file
#                                                    assert, replaced 1:1 by a
#                                                    fallback-shape else arm)
assert "S1: test_verify_compile_gate.sh reaches test_summary with rc=0 / 0 FAIL / >= 35 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_compile_gate.sh 35

assert "S2: test_verify_semaphore_wiring.sh reaches test_summary with rc=0 / 0 FAIL / >= 22 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_semaphore_wiring.sh 22

assert "S3: test_verify_offline_partition.sh reaches test_summary with rc=0 / 0 FAIL / >= 30 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_offline_partition.sh 30

# S4 is the reason the harness uses a symlink farm rather than the naive
# PATH="$STUB:/usr/bin:/bin" recipe (see the header). test_verify_semaphore_e2e.sh
# gates Sections A/B/C/F1/H behind ensure_tree_sitter_ready, and the tree-sitter
# CLI lives in ~/.cargo/bin alongside cargo-nextest — stripping that directory
# wholesale would add 5 "tree-sitter artifacts not ready" failures that have
# nothing to do with nextest, making "0 FAIL" unreachable. H3 above pins that
# tree-sitter still resolves under the harness, so a regression in the farm
# surfaces there rather than as a confusing failure here.
assert "S4: test_verify_semaphore_e2e.sh reaches test_summary with rc=0 / 0 FAIL / >= 65 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_semaphore_e2e.sh 65

assert "S5: test_verify_scope.sh reaches test_summary with rc=0 / 0 FAIL / >= 153 passed on a nextest-less host" \
    _suite_is_clean_without_nextest test_verify_scope.sh 153

test_summary
