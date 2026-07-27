#!/usr/bin/env bash
# Unit tests for tests/infra/nextest_absent_lib.sh — the shared nextest-absent
# simulation harness (task 5602).
#
# These tests exercise the RUNTIME BEHAVIOUR of the lib and of the environment
# it constructs: that cargo-nextest is genuinely unreachable under it, that the
# rest of the toolchain still EXECUTES (not merely resolves), that the harness
# does not perturb the toolchain enough to provoke a rustup sync, and that a
# nested init from within an already-constructed env still yields a usable env.
#
# Auto-discovered by run_all.sh (matches test_*.sh); registered in
# tests/infra/run-all-classification.manifest as `pool` — same reasons as its
# siblings test_load_tolerance_lib.sh and test_plan_capture_lib.sh: it is
# hermetic (its own mktemp workdir, no lane-shared state) and it never nests a
# suite that mutates the working tree.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LIB="$SCRIPT_DIR/nextest_absent_lib.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== nextest_absent_lib.sh unit tests (task 5602) ==="

# -- Existence guard: lib must exist before sourcing ---------------------------
echo ""
echo "--- Existence: nextest_absent_lib.sh exists ---"

assert "nextest_absent_lib.sh file exists" \
    test -f "$LIB"

# Source the lib (bails out cleanly if it doesn't exist, rather than aborting
# before test_summary — a missing lib must still emit a parseable Results line).
if ! [ -f "$LIB" ]; then
    echo "FATAL: nextest_absent_lib.sh not found at $LIB — skipping remaining tests"
    test_summary
fi
# shellcheck source=tests/infra/nextest_absent_lib.sh
source "$LIB"

# -- Test 1: API surface -------------------------------------------------------
# `declare -F` rather than `grep -qF "name()"` on the file: grep would pass on a
# definition sitting inside a comment, a heredoc, or a branch that never runs,
# whereas declare -F asserts the function is actually DEFINED in the shell after
# sourcing — which is the contract a caller depends on.
echo ""
echo "--- Test 1: API surface (declare -F after sourcing) ---"

_defines() {
    bash -c 'source "$1" && declare -F "$2" >/dev/null' _ "$LIB" "$1"
}

for _fn in nextest_absent_init nextest_absent_available nextest_absent_reason \
           nx_run nx_which; do
    assert "nextest_absent_lib.sh defines $_fn()" _defines "$_fn"
done
unset _fn

# -- Test 2: source guard — double-source is a no-op ---------------------------
echo ""
echo "--- Test 2: source guard _REIFY_NEXTEST_ABSENT_LIB_SH_SOURCED ---"

assert "double-sourcing nextest_absent_lib.sh is a no-op (guard works)" \
    bash -c 'source "$1" && source "$1" && declare -F nextest_absent_init >/dev/null' _ "$LIB"

# -- Test 3: the constructed env is a genuine single-variable simulation -------
#
# These are the checks that stop every LATER test in this file — and every
# migrated call site — from passing while simulating nothing at all. A harness
# that no longer hides cargo-nextest, or that "works" only by breaking the rest
# of the toolchain, must fail HERE rather than silently reporting green.
echo ""
echo "--- Test 3: nextest_absent_init builds a genuine nextest-absent env ---"

VERIFY="$REPO_ROOT/scripts/verify.sh"

# Is cargo-nextest installed AMBIENTLY? If not, this host IS already a
# nextest-less host: the harness still works, but "the plan header WITHOUT the
# harness reads nextest=1" — the check that pins the simulation as MEANINGFUL —
# cannot hold, and asserting it would go RED with a failure that says nothing
# about the property under test. So that arm is asserted only where
# cargo-nextest really is installed, and skipped with a reason otherwise.
NX_AMBIENT_HAS_NEXTEST=0
if command -v cargo-nextest >/dev/null 2>&1; then
    NX_AMBIENT_HAS_NEXTEST=1
fi

nextest_absent_init

# (a) ABSENCE, correctly expressed as non-resolvability.
_t3a() { ! nx_which cargo-nextest; }

# (b)/(c) PRESENCE, and for that `command -v` is too weak: a harness that
# perturbs more than intended can leave a tool resolvable-but-unrunnable (a
# dangling symlink, a shim whose backing toolchain the harness has stranded),
# and we would then be simulating "the toolchain is broken" rather than the
# single intended variable "cargo-nextest is absent". So both are EXECUTED.
# `env` performs its own PATH lookup with the environment it sets, so
# `nx_run <tool> --version` subsumes the resolvability check rather than
# replacing it.
_t3b() { nx_run cargo --version; }
_t3c() { nx_run tree-sitter --version; }

# (d) The observable consequence the whole harness exists to produce.
_t3d() {
    local hdr
    hdr="$(nextest_absent_plan_header "$VERIFY")"
    printf '%s\n' "$hdr"
    case "$hdr" in *"nextest=0"*) return 0 ;; *) return 1 ;; esac
}
_t3e() {
    local hdr
    hdr="$(nextest_absent_plan_header_ambient "$VERIFY")"
    printf '%s\n' "$hdr"
    case "$hdr" in *"nextest=1"*) return 0 ;; *) return 1 ;; esac
}

assert "3a: cargo-nextest is NOT resolvable under the constructed env" _t3a
assert "3b: cargo still RUNS under the env (farm keeps the toolchain intact, not merely on PATH)" _t3b
assert "3c: tree-sitter still RUNS under the env (not stripped along with the cargo bin dir)" _t3c
assert "3d: verify.sh plan header reads nextest=0 UNDER the env" _t3d

if [ "$NX_AMBIENT_HAS_NEXTEST" -eq 1 ]; then
    assert "3e: verify.sh plan header reads nextest=1 WITHOUT the env (this host has cargo-nextest, so the simulation is meaningful)" _t3e
else
    echo "  SKIP: 3e (cargo-nextest is not installed ambiently on this host, so"
    echo "        there is nothing for the farm to hide and 'nextest=1 without the"
    echo "        env' cannot hold. 3a-3d still run — against a genuinely"
    echo "        nextest-less host rather than a simulated one.)"
fi

# -- Test 4: NEST-SAFETY — the assertion that protects the whole refactor ------
#
# test_verify_nextest_absent_suites.sh runs test_verify_semaphore_wiring.sh
# INSIDE its own nextest-absent env (its assert S2, pass floor 22). Once both
# are migrated, this lib runs inside ITSELF. Measured under the outer env:
# ${CARGO_HOME:-$HOME/.cargo}/bin resolves to $NX_HOME/.cargo/bin, which does
# NOT exist — so an implementation that treats "nothing to mirror" as a
# host-precondition SKIP would make the nested semaphore_wiring emit
# "Results: 0 passed, 0 failed", blow S2's floor of 22, and turn the outer
# suite RED. S2's floor is a live tripwire on exactly this refactor.
echo ""
echo "--- Test 4: nest-safety (init from within an already-constructed env) ---"

# The child script lives inside the outer env's workdir, so the lib's own
# EXIT/INT/TERM/HUP trap cleans it up — no second trap to collide with the
# first.
NX_CHILD="$NX_WORKDIR/nested_init_probe.sh"
cat > "$NX_CHILD" <<'NESTED_PROBE'
# Called via `nx_run bash <this>` — i.e. from INSIDE an already-constructed
# nextest-absent env. Builds a SECOND env from there and reports whether it is
# still a usable simulation. Exits non-zero naming the first broken conjunct.
set -euo pipefail
cd "$1"
source tests/infra/nextest_absent_lib.sh

echo "inner: HOME=$HOME CARGO_HOME=${CARGO_HOME:-(unset)}"
_ms="${CARGO_HOME:-$HOME/.cargo}/bin"
echo "inner: naive mirror source $_ms -> $([ -d "$_ms" ] && echo EXISTS || echo MISSING)"

nextest_absent_init
echo "inner: farm entries = $(find "$NX_FARM" -mindepth 1 | wc -l)"

if ! nextest_absent_available; then
    echo "inner: FAILED — nextest_absent_available is non-zero after a nested init"
    echo "inner: reason: $(nextest_absent_reason)"
    exit 1
fi
if ! nx_run cargo --version; then
    echo "inner: FAILED — cargo does not RUN under the nested env"
    exit 1
fi
if nx_which cargo-nextest; then
    echo "inner: FAILED — cargo-nextest is REACHABLE under the nested env"
    exit 1
fi
echo "inner: OK — nested env is a usable nextest-absent simulation"
NESTED_PROBE

_t4_nested() { nx_run bash "$NX_CHILD" "$REPO_ROOT"; }

assert "4a: a SECOND nextest_absent_init from inside an already-constructed env still yields a usable simulation" \
    _t4_nested

# NON-VACUITY for 4a's first conjunct. `nextest_absent_available` returning 0
# proves nothing while it is a stub that can only ever return 0 — a checker
# that cannot fail is exactly the vacuity it exists to prevent. So: make the
# env genuinely NOT a nextest-absent simulation (put a reachable cargo-nextest
# in the farm) and require that availability reports it, naming the conjunct.
_t4_negative() {
    local rc=0
    printf '#!/bin/sh\nexit 0\n' > "$NX_FARM/cargo-nextest"
    chmod +x "$NX_FARM/cargo-nextest"

    # Sanity: the sabotage actually took effect on this PATH.
    if ! nx_which cargo-nextest >/dev/null; then
        echo "sabotage did not take: cargo-nextest still unreachable with a stub in the farm"
        rm -f "$NX_FARM/cargo-nextest"
        return 1
    fi

    if nextest_absent_available; then
        echo "nextest_absent_available returned 0 on an env where cargo-nextest IS"
        echo "reachable — it is not keyed on the observable invariant, so every"
        echo "'available' answer elsewhere in this file is vacuous."
        rc=1
    else
        local reason
        reason="$(nextest_absent_reason)"
        echo "reason: $reason"
        case "$reason" in
            *cargo-nextest*) : ;;
            *) echo "reason does not name the failing conjunct (cargo-nextest reachable)"
               rc=1 ;;
        esac
    fi

    rm -f "$NX_FARM/cargo-nextest"
    # The env must be restored, or every later assert in this file inherits the
    # sabotage.
    if nx_which cargo-nextest >/dev/null; then
        echo "cleanup failed: cargo-nextest still reachable after removing the stub"
        rc=1
    fi
    return "$rc"
}

assert "4b: nextest_absent_available reports UNAVAILABLE (naming the conjunct) when cargo-nextest is reachable" \
    _t4_negative

test_summary
