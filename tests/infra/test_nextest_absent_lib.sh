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

for _fn in nextest_absent_init nextest_absent_cleanup nextest_absent_available \
           nextest_absent_reason nx_run nx_which; do
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
#
# Delegated WHOLESALE to nextest_absent_assert_real rather than re-stated. An
# earlier draft of this file open-coded H1-H5 here as _t3a.._t3e — the same
# `! nx_which cargo-nextest`, the same executed `cargo`/`tree-sitter` version
# probes, the same plan-header `case`, the same ambient skip arm. In the file
# whose whole thesis is "four call sites hand-rolled the same harness, so
# consolidate it", that was a fifth hand-rolled copy of the realness checks, free
# to drift from the lib's (adding an H8 would have updated one and not the
# other). Test 7 keeps the complementary job: pinning assert_real's COUNTER SHAPE
# and proving it can actually fail.
echo ""
echo "--- Test 3: nextest_absent_init builds a genuine nextest-absent env ---"

VERIFY="$REPO_ROOT/scripts/verify.sh"

# Is cargo-nextest installed AMBIENTLY? If not, this host IS already a
# nextest-less host: the harness still works, but "the plan header WITHOUT the
# harness reads nextest=1" — the check that pins the simulation as MEANINGFUL —
# cannot hold. nextest_absent_assert_real makes that arm conditional on exactly
# this condition; Test 7's expected counter shape has to agree with it, which is
# why the probe stays here.
NX_AMBIENT_HAS_NEXTEST=0
if command -v cargo-nextest >/dev/null 2>&1; then
    NX_AMBIENT_HAS_NEXTEST=1
fi

# Same shape, same reason, for H3: tree-sitter is an optional CLI, so on a host
# without it the farm has nothing to mirror and "the farm did not strip it"
# cannot hold. nextest_absent_assert_real skips that arm there; Test 7's
# expected counter has to agree, or 7a goes red on exactly the hosts the guard
# was added to protect.
NX_AMBIENT_HAS_TREE_SITTER=0
if command -v tree-sitter >/dev/null 2>&1; then
    NX_AMBIENT_HAS_TREE_SITTER=1
fi

nextest_absent_init

nextest_absent_assert_real "$VERIFY"

# -- Test 4: NEST-SAFETY — the assertion that protects the whole refactor ------
#
# test_verify_nextest_absent_suites.sh runs test_verify_semaphore_wiring.sh
# INSIDE its own nextest-absent env (its assert S2, pass floor 22), so this lib
# runs inside ITSELF. Asserts that a second nextest_absent_init from within an
# already-constructed env still yields a usable simulation — the mirror-source
# resolution chain and the empty-farm degrade that make that hold are documented
# at nextest_absent_init in the lib.
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
# The sabotage uses the lib's own presence-marker API rather than its own
# printf/chmod — writing a second stub-installer here would be one more
# hand-rolled copy of a construct the lib already owns, and it would keep passing
# if the lib's version regressed.
_t4_negative() {
    local rc=0
    nextest_absent_farm_add_nextest_stub || {
        echo "nextest_absent_farm_add_nextest_stub failed"
        return 1
    }

    # Sanity: the sabotage actually took effect on this PATH.
    if ! nx_which cargo-nextest >/dev/null; then
        echo "sabotage did not take: cargo-nextest still unreachable with a stub in the farm"
        nextest_absent_farm_rm_nextest_stub
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

    nextest_absent_farm_rm_nextest_stub
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

# -- Test 5: toolchain hygiene -------------------------------------------------
#
# The two hygiene predicates as STANDALONE API (nextest_absent_assert_real
# already runs them as H6/H7; these arms pin them as callable on their own), plus
# the non-vacuity arms that no other test covers. Why they exist at all — the
# stranded-rustup mechanism and the 935 MB measurement — is documented at
# "Toolchain hygiene" in tests/infra/nextest_absent_lib.sh.
#
# Asserted AFTER Tests 3 and 4, so every check that actually exercises the env
# with a real cargo invocation has already had its chance to provoke a sync.
echo ""
echo "--- Test 5: the env does not perturb the toolchain (no rustup sync) ---"

assert "5a: no .rustup appeared in the throwaway HOME (RUSTUP_HOME was carried across)" \
    nextest_absent_no_rustup_sync

assert "5b: the throwaway HOME is still under the size ceiling" \
    nextest_absent_home_is_small

# NON-VACUITY for both. A hygiene predicate that cannot fail is worth nothing —
# it would report green on the very 935 MB sync it exists to catch. Each arm
# below manufactures the exact condition the predicate claims to detect and
# requires a non-zero return, then restores the env.
_t5c_negative() {
    local rc=0
    mkdir -p "$NX_HOME/.rustup/toolchains"
    if nextest_absent_no_rustup_sync >/dev/null 2>&1; then
        echo "nextest_absent_no_rustup_sync returned 0 with $NX_HOME/.rustup present"
        echo "— it cannot detect the toolchain sync it exists to catch."
        rc=1
    fi
    rm -rf "$NX_HOME/.rustup"
    nextest_absent_no_rustup_sync >/dev/null 2>&1 || {
        echo "cleanup failed: predicate still reports a .rustup after removal"
        rc=1
    }
    return "$rc"
}

_t5d_negative() {
    local rc=0
    # DISCRIMINATION, not just failure. Checking only "fails at a 0 KB ceiling"
    # would itself be vacuous: a predicate that does not exist returns 127, and
    # a predicate hard-wired to `return 1` returns 1 — both satisfy it while
    # detecting nothing. So require BOTH directions from the same knob.
    if NEXTEST_ABSENT_HOME_MAX_KB=0 nextest_absent_home_is_small >/dev/null 2>&1; then
        echo "nextest_absent_home_is_small returned 0 against a 0 KB ceiling"
        echo "— it does not honour NEXTEST_ABSENT_HOME_MAX_KB, so the ceiling is"
        echo "decorative and a real sync would not trip it."
        rc=1
    fi
    if ! NEXTEST_ABSENT_HOME_MAX_KB=1048576 nextest_absent_home_is_small >/dev/null 2>&1; then
        echo "nextest_absent_home_is_small returned non-zero against a 1 GB ceiling"
        echo "— it is not measuring the throwaway HOME at all (or does not exist),"
        echo "so its 0 KB failure above proves nothing."
        rc=1
    fi
    return "$rc"
}

assert "5c: nextest_absent_no_rustup_sync FAILS when a .rustup is present (it can actually detect a sync)" \
    _t5c_negative

assert "5d: nextest_absent_home_is_small honours NEXTEST_ABSENT_HOME_MAX_KB (a 0 KB ceiling fails)" \
    _t5d_negative

# -- Test 6: the farm-overlay API ---------------------------------------------
#
# This is what lets test_verify_nextest_probe.sh SHARE the lib instead of
# forking it. That suite does not test "nextest is absent" — it tests
# verify.sh's retry/hard-fail behaviour when the probe FAILS while cargo-nextest
# is PRESENT (its cycles i, ii, iv, v); only cycle iii is genuine absence. A
# harness that could only express absence would force a fork. Exposing the farm
# as an overlayable directory plus an add/remove pair for the cargo-nextest
# presence marker makes BOTH states parameters of one harness.
echo ""
echo "--- Test 6: farm overlay + the cargo-nextest presence marker ---"

# Fresh env: Test 6 deliberately overlays `cargo` itself, so it must not leave
# that overlay in place for later sections. nextest_absent_init tears the
# previous workdir down before building the new one.
nextest_absent_init

NX_MARKER="__reify_5602_overlay_marker__"
NX_OVERLAY="$NX_WORKDIR/fake-cargo.sh"
cat > "$NX_OVERLAY" <<OVERLAY
#!/usr/bin/env bash
if [ "\${1:-}" = "$NX_MARKER" ]; then
    echo "$NX_MARKER"
    exit 0
fi
exit 7
OVERLAY

# (a) The overlay must WIN under nx_run. This proves farm-first PATH ordering
# shadows BOTH the filtered cargo bin dir and any other cargo later in PATH —
# measured on this host, /home/leo/.local/bin/cargo exists and is what ambient
# `command -v cargo` resolves to, so this is not a theoretical ordering.
_t6a() {
    local out
    nextest_absent_farm_put cargo "$NX_OVERLAY" || return 1
    out="$(nx_run cargo "$NX_MARKER" 2>&1)" || {
        echo "overlaid cargo did not run: $out"
        return 1
    }
    printf '%s\n' "$out"
    [ "$out" = "$NX_MARKER" ]
}

# (b) Putting OVER an existing farm entry must succeed. The farm entry for
# `cargo` is already a mirrored symlink, and a bare `cp`/`ln -s` onto an
# existing symlink fails — so this is the arm that catches the naive
# implementation.
_t6b() {
    nextest_absent_farm_put cargo "$NX_OVERLAY" || {
        echo "second nextest_absent_farm_put over an existing entry failed"
        return 1
    }
    local out
    out="$(nx_run cargo "$NX_MARKER" 2>&1)" || return 1
    [ "$out" = "$NX_MARKER" ]
}

# (c) The presence marker as a PARAMETER — exactly the probe's
# cycle-(iii)-vs-the-rest distinction. Toggled repeatedly, because an
# implementation that works once but not twice would silently corrupt the
# probe's five-cycle sequence.
_t6c() {
    local i
    for i in 1 2 3; do
        nextest_absent_farm_add_nextest_stub || return 1
        nx_which cargo-nextest >/dev/null || {
            echo "toggle $i: cargo-nextest NOT reachable after add_nextest_stub"
            return 1
        }
        nextest_absent_farm_rm_nextest_stub || return 1
        if nx_which cargo-nextest >/dev/null; then
            echo "toggle $i: cargo-nextest STILL reachable after rm_nextest_stub"
            return 1
        fi
    done
    echo "3 add/remove toggles, presence tracked exactly"
    return 0
}

# (d) The plain env must be nextest-absent with NO caller action — the stub
# stays OUT of the farm by default. Asserted after (c) so it also proves the
# toggles left no residue.
_t6d() { ! nx_which cargo-nextest; }

assert "6a: nextest_absent_farm_put makes the overlaid executable WIN under nx_run" _t6a
assert "6b: nextest_absent_farm_put over an existing farm entry succeeds (mirrored symlink is replaced)" _t6b
assert "6c: add/rm_nextest_stub toggle cargo-nextest presence, idempotently across repeats" _t6c
assert "6d: the plain env is nextest-absent with no caller action (stub is out of the farm by default)" _t6d

# -- Test 7: the caller-facing convenience helpers -----------------------------
echo ""
echo "--- Test 7: nextest_absent_assert_real / nextest_available_ambient ---"

# Fresh env — Test 6 left an overlaid fake `cargo` in the farm.
nextest_absent_init

# nextest_absent_assert_real emits its checks as assert() calls against the
# SOURCING suite's PASS/FAIL counters (rather than returning a boolean), which
# is what keeps test_verify_nextest_absent_suites.sh's H-section pass counts —
# and therefore its S1-S4 floors — meaningful after migration. So it must be
# exercised in a CHILD suite whose counters we can read back.
NX_REAL_CHILD="$NX_WORKDIR/assert_real_probe.sh"
cat > "$NX_REAL_CHILD" <<'REAL_PROBE'
# argv: <repo-root> <sabotage: 0|1>
set -euo pipefail
cd "$1"
source tests/infra/test_helpers.sh
source tests/infra/nextest_absent_lib.sh
nextest_absent_init
[ "$2" = "1" ] && nextest_absent_farm_add_nextest_stub
nextest_absent_assert_real
# Report the counters on a machine-readable line INSTEAD of test_summary, so
# this probe never exits non-zero merely for having observed a FAIL.
echo "COUNTERS: pass=$PASS fail=$FAIL"
REAL_PROBE

# How many asserts nextest_absent_assert_real must emit. Two of the seven are
# conditional on host shape — same convention as 3e:
#   H3 (tree-sitter EXECUTES)      only where tree-sitter is installed ambiently
#   H5 (ambient header nextest=1)  only where cargo-nextest is installed ambiently
# Both are meaningless where the tool they pin is absent, so the floor is the
# five unconditional checks (H1, H2, H4, H6, H7) plus whichever arms apply.
NX_REAL_EXPECTED=5
[ "$NX_AMBIENT_HAS_TREE_SITTER" -eq 1 ] && NX_REAL_EXPECTED=$((NX_REAL_EXPECTED + 1))
[ "$NX_AMBIENT_HAS_NEXTEST" -eq 1 ] && NX_REAL_EXPECTED=$((NX_REAL_EXPECTED + 1))

_nx_counters() { bash "$NX_REAL_CHILD" "$REPO_ROOT" "$1" 2>&1 | grep -E '^COUNTERS:' | tail -1; }

# (a) HEALTHY env: exactly the expected number of PASSes, zero FAILs.
_t7a() {
    local line pass fail
    line="$(_nx_counters 0)"
    echo "$line"
    pass="$(printf '%s\n' "$line" | sed -n 's/^COUNTERS: pass=\([0-9]\{1,\}\) fail=\([0-9]\{1,\}\)$/\1/p')"
    fail="$(printf '%s\n' "$line" | sed -n 's/^COUNTERS: pass=\([0-9]\{1,\}\) fail=\([0-9]\{1,\}\)$/\2/p')"
    [ -n "$pass" ] && [ -n "$fail" ] || { echo "no parseable COUNTERS line"; return 1; }
    [ "$fail" -eq 0 ] || { echo "expected 0 FAILs on a healthy env, got $fail"; return 1; }
    [ "$pass" -eq "$NX_REAL_EXPECTED" ] || {
        echo "expected exactly $NX_REAL_EXPECTED PASSes, got $pass — the H-section"
        echo "count is what keeps the migrated suites' S1-S4 floors meaningful."
        return 1
    }
    return 0
}

# (b) BROKEN env: must register a FAIL — not abort the suite, not pass silently.
# THIS is the point. A realness checker that cannot fail is exactly the vacuity
# it exists to prevent.
_t7b() {
    local line pass fail
    line="$(_nx_counters 1)"
    echo "$line"
    pass="$(printf '%s\n' "$line" | sed -n 's/^COUNTERS: pass=\([0-9]\{1,\}\) fail=\([0-9]\{1,\}\)$/\1/p')"
    fail="$(printf '%s\n' "$line" | sed -n 's/^COUNTERS: pass=\([0-9]\{1,\}\) fail=\([0-9]\{1,\}\)$/\2/p')"
    [ -n "$pass" ] && [ -n "$fail" ] || {
        echo "no parseable COUNTERS line — assert_real ABORTED the suite instead"
        echo "of registering a FAIL against its counters."
        return 1
    }
    [ "$fail" -ge 1 ] || {
        echo "a farm containing a reachable cargo-nextest is NOT a real"
        echo "nextest-absent env, yet assert_real registered 0 FAILs."
        return 1
    }
    return 0
}

# (c) nextest_available_ambient — the idiom migrating off
# test_verify_retry_subset.sh:74-81. Must agree with the ambient plan header.
_t7c() {
    local hdr want
    hdr="$(nextest_absent_plan_header_ambient)"
    case "$hdr" in *"nextest=1"*) want=0 ;; *) want=1 ;; esac
    echo "ambient header: $hdr (expect rc=$want)"
    if nextest_available_ambient; then
        [ "$want" -eq 0 ]
    else
        [ "$want" -eq 1 ]
    fi
}

assert "7a: nextest_absent_assert_real emits exactly $NX_REAL_EXPECTED PASSes / 0 FAILs on a healthy env" _t7a
assert "7b: nextest_absent_assert_real registers a FAIL (not an abort, not a silent pass) on a broken env" _t7b
assert "7c: nextest_available_ambient agrees with the ambient verify.sh plan header" _t7c

# -- Test 8: consolidation guard — no suite re-open-codes the availability probe
#
# WHAT THIS PINS. Task 5602 consolidated the "capture a verify.sh --print-plan,
# grep '^# verify.sh plan', case on *"nextest=1"*" idiom into this lib, and task
# 5644 migrated the seven suites that had each re-derived it. Nothing so far
# stops an EIGHTH copy accreting — the idiom is four lines and looks local, which
# is exactly how it reached seven copies. This is that stop.
#
# BOTH DIRECTIONS, per suite. A purely negative "has no open-coded case arm"
# check is satisfied by DELETING the probe outright, which would leave
# NEXTEST_AVAILABLE unbound (a `set -u` abort) or pinned to 0 — silently dropping
# every assert the guards protect. That is the vacuous green documented at
# nextest_absent_lib.sh:700-707. So each suite must ALSO be shown to source the
# lib, i.e. to have replaced the idiom rather than removed it.
#
# WHY A LITERAL LIST AND NOT tests/infra/*.sh. The idiom's own string legitimately
# appears inside nextest_absent_lib.sh (:491, :497, :543, :567) and inside this
# file (:457, :520) — a glob would flag the lib that IS the fix. The list is the
# suites that carry a NEXTEST_AVAILABLE host probe; adding one to it is the
# deliberate act of putting a new suite under the guard.
echo ""
echo "--- Test 8: no tests/infra suite re-open-codes the nextest availability probe ---"

# The suites carrying a NEXTEST_AVAILABLE host probe: the seven migrated by task
# 5644 plus test_verify_retry_subset.sh, migrated by 5602 as the template.
_NX_PROBE_SUITES=(
    test_run_offline_deep.sh
    test_verify_gate_exclude_heavy.sh
    test_verify_offline_partition.sh
    test_verify_retry_failed_only.sh
    test_verify_role_prio.sh
    test_verify_semaphore_e2e.sh
    test_verify_test_threads.sh
    test_verify_retry_subset.sh
)

# The open-coded arm, as a regex over the file text: `*"nextest=1"*)`. Matching
# the QUOTED glob arm rather than the bare string `nextest=1` is what keeps this
# from firing on a comment or an echo that merely mentions the header.
_NX_OPEN_CODED_RE='\*"nextest=1"\*'
_NX_LIB_SOURCE_LINE='source "$SCRIPT_DIR/nextest_absent_lib.sh"'

# (neg) No open-coded case arm. On failure name the file AND the matching line,
# so the diagnostic points at the cause rather than just the verdict — the
# convention the lib's own predicates follow (nextest_absent_lib.sh:630-631).
_t8_no_open_coded() {
    local suite="$SCRIPT_DIR/$1" hits
    [ -f "$suite" ] || { echo "covered suite not found at $suite"; return 1; }
    hits="$(grep -nE "$_NX_OPEN_CODED_RE" "$suite" || true)"
    [ -z "$hits" ] || {
        echo "$1 open-codes the nextest availability probe:"
        printf '%s\n' "$hits"
        echo "-> replace it with nextest_available_in_plan \"\$PLAN\" (when a plan is"
        echo "   already captured) or nextest_available_ambient \"\$VERIFY\" (when it"
        echo "   is not), from tests/infra/nextest_absent_lib.sh."
        return 1
    }
    return 0
}

# (pos) The probe was REPLACED, not removed — the suite sources the lib.
_t8_sources_lib() {
    local suite="$SCRIPT_DIR/$1"
    [ -f "$suite" ] || { echo "covered suite not found at $suite"; return 1; }
    grep -qF "$_NX_LIB_SOURCE_LINE" "$suite" || {
        echo "$1 does not source nextest_absent_lib.sh."
        echo "-> a suite that merely DELETED its probe would satisfy the negative"
        echo "   check while leaving NEXTEST_AVAILABLE unbound or pinned to 0,"
        echo "   silently dropping every assert its guards protect."
        return 1
    }
    return 0
}

for _nx_suite in "${_NX_PROBE_SUITES[@]}"; do
    assert "8: $_nx_suite does not open-code the *\"nextest=1\"* availability probe" \
        _t8_no_open_coded "$_nx_suite"
    assert "8: $_nx_suite sources nextest_absent_lib.sh (probe replaced, not removed)" \
        _t8_sources_lib "$_nx_suite"
done
unset _nx_suite

# (vac) NEGATIVE CONTROL — the guard is now GREEN on every covered suite, which
# is precisely when it stops being able to show that it still detects anything.
# All those greens are equally consistent with a working detector and with one
# whose grep quietly stopped matching (a renamed helper, an escaping change in
# _NX_OPEN_CODED_RE, a `grep -F` slipped in where -E was meant). Same failure
# mode this file already names for the realness checker at :430-432 — "A
# realness checker that cannot fail is exactly the vacuity it exists to
# prevent", asserted there by _t7b — so it gets the same treatment here.
#
# Hand the detector a synthetic EIGHTH copy of the idiom and require it to
# report that copy.
#
# The fixture is a `mktemp -d` UNDER $NX_WORKDIR, so the lib's own
# EXIT/INT/TERM/HUP trap reclaims it: the reason Test 4's child probe lives
# there too (:127-129) is that a second trap of our own would CLOBBER the
# lib's rather than compose with it, which is the whole subject of Test 10.
_NX_T8_VAC_DIR="$(mktemp -d "$NX_WORKDIR/t8_vacuity.XXXXXX")"
_NX_T8_DIRTY="$_NX_T8_VAC_DIR/test_verify_synthetic_eighth_copy.sh"

# Written as the seven suites migrated by task 5644 actually looked, so the
# control exercises the shape that really accretes rather than a toy string.
# Never executed — the detector only ever READS it.
cat > "$_NX_T8_DIRTY" <<'DIRTY_SUITE'
#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
source "$SCRIPT_DIR/test_helpers.sh"

_HEADER="$(bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan 2>/dev/null \
    | grep '^# verify.sh plan')" || true
NEXTEST_AVAILABLE=0
case "$_HEADER" in
    *"nextest=1"*) NEXTEST_AVAILABLE=1 ;;
esac
DIRTY_SUITE

_t8_detector_reports_a_dirty_suite() {
    local out rc=0 name
    name="$(basename "$_NX_T8_DIRTY")"
    out="$(_nx_probe_suite_is_clean "$_NX_T8_DIRTY" 2>&1)" || rc=$?
    printf '%s\n' "$out"
    [ "$rc" -ne 0 ] || {
        echo "the detector PASSED a file that open-codes the probe AND never"
        echo "sources the lib. It is no longer detecting anything, so every"
        echo "green above is vacuous."
        return 1
    }
    # A non-zero rc on its own is NOT enough. An undefined or renamed predicate
    # also exits non-zero — 127, "command not found" — which would let this
    # control go green while observing nothing at all. So require the same
    # evidence the per-suite arms print: the offending FILE, and the offending
    # LINE.
    case "$out" in
        *"$name"*) ;;
        *) echo "detector reported rc=$rc but its diagnostic never names $name"
           return 1 ;;
    esac
    case "$out" in
        *'nextest=1'*) ;;
        *) echo "detector reported rc=$rc but its diagnostic never quotes the"
           echo "offending line — so it did not actually match the idiom."
           return 1 ;;
    esac
    return 0
}

assert "8: the detector REPORTS a synthetic eighth copy of the idiom (negative control)" \
    _t8_detector_reports_a_dirty_suite

# -- Test 9: the plan header is located by CONTENT, not by position ------------
#
# nextest_absent_plan_header{,_ambient} used to extract the header with
# `sed -n '1p'`. Measured today the header IS line 1 — so the bug this test pins
# is LATENT, and latent is exactly when it is cheap to close: the day verify.sh
# emits anything before it (a warning, a deprecation notice, an apply_env
# diagnostic), positional extraction returns that line instead, every
# `case ... nextest=1` rejects it, nextest_available_ambient reports "no nextest",
# test_verify_retry_subset.sh sets NEXTEST_AVAILABLE=0 — and every assert behind
# its guards is SILENTLY DROPPED. That is a vacuous green, the failure class this
# whole task family exists to prevent, and it would announce itself nowhere.
#
# So the extraction is pointed at stub verify.sh scripts that print a preamble
# line first, and required to find the header anyway.
echo ""
echo "--- Test 9: plan-header extraction survives a preamble line ---"

NX_STUB_DIR="$NX_WORKDIR/header-stubs"
mkdir -p "$NX_STUB_DIR"

# $1 = path, $2 = the nextest digit to advertise. The preamble is deliberately
# the FIRST line, which is the whole point.
_nx_write_preamble_stub() {
    cat > "$1" <<STUB
#!/usr/bin/env bash
echo "warning: apply_env — /etc/reify/env not readable, continuing"
echo "# verify.sh plan — action=test profile=debug scope=all include_infra=0 nextest=$2 role=task"
echo "# scope decision — RUN_RUST=1 RUN_GUI=1"
STUB
    chmod +x "$1"
}

NX_STUB_N0="$NX_STUB_DIR/verify_preamble_n0.sh"
NX_STUB_N1="$NX_STUB_DIR/verify_preamble_n1.sh"
NX_STUB_NOHDR="$NX_STUB_DIR/verify_no_header.sh"
_nx_write_preamble_stub "$NX_STUB_N0" 0
_nx_write_preamble_stub "$NX_STUB_N1" 1
printf '#!/usr/bin/env bash\necho "warning: no plan was produced"\n' > "$NX_STUB_NOHDR"
chmod +x "$NX_STUB_NOHDR"

# (a)/(b) BOTH extractors — they are separate functions and a fix applied to one
# only would leave the other positional.
_t9a() {
    local hdr; hdr="$(nextest_absent_plan_header "$NX_STUB_N0")"
    printf 'header: %s\n' "$hdr"
    case "$hdr" in "# verify.sh plan"*"nextest=0"*) return 0 ;; *) return 1 ;; esac
}
_t9b() {
    local hdr; hdr="$(nextest_absent_plan_header_ambient "$NX_STUB_N1")"
    printf 'header: %s\n' "$hdr"
    case "$hdr" in "# verify.sh plan"*"nextest=1"*) return 0 ;; *) return 1 ;; esac
}

# (c) The consequence that actually matters: retry_subset's availability probe
# must still answer correctly in BOTH directions through a preamble. One
# direction alone is satisfied by a function hard-wired to that answer.
_t9c() {
    nextest_available_ambient "$NX_STUB_N1" || {
        echo "nextest_available_ambient said NO on a preamble+nextest=1 plan —"
        echo "every assert behind test_verify_retry_subset.sh's NEXTEST_AVAILABLE"
        echo "guards would be silently dropped."
        return 1
    }
    if nextest_available_ambient "$NX_STUB_N0"; then
        echo "nextest_available_ambient said YES on a nextest=0 plan — it is not"
        echo "reading the header at all, so its YES above proves nothing."
        return 1
    fi
    return 0
}

# (d) No header at all -> empty, and reported as unavailable rather than
# matching some unrelated line that happens to be present.
_t9d() {
    local hdr; hdr="$(nextest_absent_plan_header_ambient "$NX_STUB_NOHDR")"
    printf 'header: [%s]\n' "$hdr"
    [ -z "$hdr" ] || { echo "expected an empty header from a headerless plan"; return 1; }
    ! nextest_available_ambient "$NX_STUB_NOHDR"
}

assert "9a: nextest_absent_plan_header finds the header behind a preamble line" _t9a
assert "9b: nextest_absent_plan_header_ambient finds the header behind a preamble line" _t9b
assert "9c: nextest_available_ambient answers correctly in BOTH directions through a preamble" _t9c
assert "9d: a plan with no header yields an empty header and reports unavailable" _t9d

# -- Test 10: trap ownership — the lib composes, it does not clobber -----------
#
# bash traps REPLACE rather than compose, so an unconditional
# `trap 'rm -rf "$NX_WORKDIR"' EXIT` in nextest_absent_init silently disarms
# whatever the sourcing suite had already registered. test_verify_semaphore_
# wiring.sh registers `trap cleanup EXIT` for its throwaway git repos at suite
# start — under the clobbering version those repos leaked unless that file
# hand-patched around the lib, and the hand-patch in turn disarmed the LIB's
# trap. Nothing covered either direction, so a regression in either shipped
# green. These arms are that coverage.
echo ""
echo "--- Test 10: trap composition (handler registered before / after init) ---"

NX_TRAP_DIR="$NX_WORKDIR/trap-probes"
mkdir -p "$NX_TRAP_DIR"

# (a) BEFORE init -> composed. Both handlers must fire, and the lib's teardown
# must run FIRST (the caller's handler observes the workdir already gone), which
# is what lets a caller's handler assume the lib has finished with it.
NX_TRAP_BEFORE="$NX_TRAP_DIR/before.sh"
cat > "$NX_TRAP_BEFORE" <<'TRAP_BEFORE'
# argv: <repo-root> <marker-file>
set -euo pipefail
cd "$1"
_m="$2"
caller_cleanup() {
    if [ -d "${NX_WORKDIR:-/nonexistent}" ]; then
        echo "CALLER_SAW_WORKDIR" >> "$_m"
    else
        echo "CALLER_SAW_NO_WORKDIR" >> "$_m"
    fi
    echo "CALLER_RAN" >> "$_m"
}
trap caller_cleanup EXIT
source tests/infra/nextest_absent_lib.sh
nextest_absent_init
echo "WORKDIR=$NX_WORKDIR" >> "$_m"
TRAP_BEFORE

_t10a() {
    local m="$NX_TRAP_DIR/before.marker" wd rc=0
    rm -f "$m"
    bash "$NX_TRAP_BEFORE" "$REPO_ROOT" "$m" || { echo "probe exited non-zero"; return 1; }
    cat "$m"

    grep -q '^CALLER_RAN$' "$m" || {
        echo "the caller's pre-init EXIT handler did NOT fire — nextest_absent_init"
        echo "clobbered it instead of composing with it."
        rc=1
    }
    grep -q '^CALLER_SAW_NO_WORKDIR$' "$m" || {
        echo "the caller's handler ran while NX_WORKDIR still existed — the lib's"
        echo "teardown is not sequenced before the composed handler."
        rc=1
    }
    wd="$(sed -n 's/^WORKDIR=//p' "$m" | tail -1)"
    [ -n "$wd" ] || { echo "probe never reported its workdir"; return 1; }
    if [ -d "$wd" ]; then
        echo "the lib's own workdir LEAKED: $wd"
        rm -rf "$wd"
        rc=1
    fi
    return "$rc"
}

# (b) AFTER init -> caller-owned, which is the documented half of the contract:
# bash offers no retroactive compose, so the caller's handler replaces the
# dispatcher and MUST call nextest_absent_cleanup. This arm pins that the exposed
# function is what makes that possible — before it existed, the only recourse was
# to reach into the lib's internals for NX_WORKDIR.
NX_TRAP_AFTER="$NX_TRAP_DIR/after.sh"
cat > "$NX_TRAP_AFTER" <<'TRAP_AFTER'
# argv: <repo-root> <marker-file>
set -euo pipefail
cd "$1"
_m="$2"
source tests/infra/nextest_absent_lib.sh
nextest_absent_init
echo "WORKDIR=$NX_WORKDIR" >> "$_m"
late_cleanup() { nextest_absent_cleanup; echo "LATE_RAN" >> "$_m"; }
trap late_cleanup EXIT
TRAP_AFTER

_t10b() {
    local m="$NX_TRAP_DIR/after.marker" wd rc=0
    rm -f "$m"
    bash "$NX_TRAP_AFTER" "$REPO_ROOT" "$m" || { echo "probe exited non-zero"; return 1; }
    cat "$m"

    grep -q '^LATE_RAN$' "$m" || { echo "the caller's post-init handler did not fire"; rc=1; }
    wd="$(sed -n 's/^WORKDIR=//p' "$m" | tail -1)"
    [ -n "$wd" ] || { echo "probe never reported its workdir"; return 1; }
    if [ -d "$wd" ]; then
        echo "nextest_absent_cleanup did not remove the workdir when called from a"
        echo "caller-owned handler: $wd"
        rm -rf "$wd"
        rc=1
    fi
    return "$rc"
}

# (c) The composition is armed on ALL FOUR signals, not just EXIT — verify.sh
# wraps infra tests in `timeout --kill-after`, so an outer kill must still tear
# the temp tree down AND still run the caller's handler. Exercised by having the
# probe signal ITSELF, once per child per signal, so a lib that replays on only
# one of the three cannot pass an arm whose title claims all three.
#
# WHY THE PROBE SIGKILLS ITSELF AFTERWARDS — this is the whole difference between
# this arm and a vacuous one. _nextest_absent_trap_dispatch deliberately does not
# re-raise, so once the handler returns the shell RESUMES, runs to the end and
# exits NORMALLY — firing the EXIT trap and appending CALLER_RAN whether or not
# the signal dispatch replayed anything at all. Measured: the first version of
# this arm reported PASS against a lib that stashed nothing for INT/TERM/HUP, for
# exactly that reason. SIGKILL is uncatchable and untrappable, so it removes the
# second path outright: past that line CALLER_RAN can only have been written by
# the <signal> dispatch. SURVIVED_KILL is asserted ABSENT rather than assumed, so
# a probe that somehow outlived its own KILL is reported instead of silently
# restoring the vacuity. rc is deliberately never asserted on — the probe is
# EXPECTED to die 137.
NX_TRAP_SIG="$NX_TRAP_DIR/signal.sh"
cat > "$NX_TRAP_SIG" <<'TRAP_SIG'
# argv: <repo-root> <marker-file> <signal>
set -euo pipefail
cd "$1"
_m="$2"
_sig="$3"
caller_cleanup() { echo "CALLER_RAN" >> "$_m"; }
trap caller_cleanup EXIT
source tests/infra/nextest_absent_lib.sh
nextest_absent_init
echo "WORKDIR=$NX_WORKDIR" >> "$_m"
kill -"$_sig" $$
echo "RESUMED" >> "$_m"
kill -KILL $$
echo "SURVIVED_KILL" >> "$_m"
TRAP_SIG

_t10c() {
    local sig m wd rc=0
    for sig in INT TERM HUP; do
        m="$NX_TRAP_DIR/signal-$sig.marker"
        rm -f "$m"
        # 2>/dev/null: the parent shell reports the child's death as "Killed",
        # which is the expected outcome here, not evidence.
        bash "$NX_TRAP_SIG" "$REPO_ROOT" "$m" "$sig" 2>/dev/null || true
        echo "--- SIG$sig ---"
        if [ ! -f "$m" ]; then
            echo "SIG$sig: the probe wrote no marker at all"
            rc=1
            continue
        fi
        cat "$m"

        if grep -q '^SURVIVED_KILL$' "$m"; then
            echo "SIG$sig: the probe outlived its own SIGKILL, so CALLER_RAN could"
            echo "have come from the EXIT trap — this arm would be vacuous."
            rc=1
        fi
        if ! grep -q '^CALLER_RAN$' "$m"; then
            echo "SIG$sig: the caller's pre-init handler never fired on SIG$sig. The"
            echo "lib stashes a handler PER SIGNAL and this caller registered one"
            echo "only for EXIT, so SIG$sig's stash is empty and the dispatcher ran"
            echo "lib teardown alone — the documented 'upgraded to all four signals'"
            echo "half of the trap contract (nextest_absent_lib.sh, TRAP OWNERSHIP)"
            echo "is not implemented."
            rc=1
        fi
        wd="$(sed -n 's/^WORKDIR=//p' "$m" | tail -1)"
        if [ -z "$wd" ]; then
            echo "SIG$sig: probe never reported its workdir"
            rc=1
        elif [ -d "$wd" ]; then
            echo "SIG$sig: the signal left the lib's own workdir behind: $wd"
            rm -rf "$wd"
            rc=1
        fi
    done
    return "$rc"
}

# (d) The REAL consumer's shape, not a reduced probe, and the filesystem
# consequence rather than a marker.
#
# test_verify_semaphore_wiring.sh keeps throwaway git repos on a _TMPDIRS array
# behind a bare `trap cleanup EXIT` registered at :24 — BEFORE the lib's init at
# :198 — and verify.sh runs that suite under `timeout --kill-after=60 <n>m`. So
# the operational question is not "did a marker get appended" but "when the
# timeout fires, do the CALLER's directories get removed, or only the lib's?"
# This arm requires the caller's dir to be gone; 10c would still pass if the
# replay ran but the caller's own state leaked.
#
# Shaped deliberately like that file: the same `[@]+` empty-array guard, the
# trap registered before the lib is sourced, and the dir pushed AFTER init (the
# real suite creates its repos in Sections 2/3, long after). The push-after-init
# ordering also pins that the stash captures the handler by NAME — `cleanup` is
# resolved at dispatch time and must therefore see the dirs added since.
NX_TRAP_CONSUMER="$NX_TRAP_DIR/consumer.sh"
cat > "$NX_TRAP_CONSUMER" <<'TRAP_CONSUMER'
# argv: <repo-root> <marker-file>
set -euo pipefail
cd "$1"
_m="$2"
_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT
source tests/infra/nextest_absent_lib.sh
nextest_absent_init
echo "WORKDIR=$NX_WORKDIR" >> "$_m"
_dir="$(mktemp -d)"
_TMPDIRS+=("$_dir")
echo "CALLERDIR=$_dir" >> "$_m"
kill -TERM $$
kill -KILL $$
echo "SURVIVED_KILL" >> "$_m"
TRAP_CONSUMER

_t10d() {
    local m="$NX_TRAP_DIR/consumer.marker" wd caller_dir rc=0
    rm -f "$m"
    bash "$NX_TRAP_CONSUMER" "$REPO_ROOT" "$m" 2>/dev/null || true
    if [ ! -f "$m" ]; then
        echo "the probe wrote no marker at all"
        return 1
    fi
    cat "$m"

    if grep -q '^SURVIVED_KILL$' "$m"; then
        echo "the probe outlived its own SIGKILL — its EXIT trap could have done"
        echo "the cleanup, which would make this arm vacuous."
        rc=1
    fi

    caller_dir="$(sed -n 's/^CALLERDIR=//p' "$m" | tail -1)"
    if [ -z "$caller_dir" ]; then
        echo "probe never reported its own temp dir"
        rc=1
    elif [ -d "$caller_dir" ]; then
        echo "SIGTERM LEAKED the CALLER's temp dir: $caller_dir"
        echo "The lib tore its own workdir down but did not replay the caller's"
        echo "EXIT handler on TERM, so a timeout kill of a suite shaped like"
        echo "test_verify_semaphore_wiring.sh strands every throwaway repo it made."
        rm -rf "$caller_dir"
        rc=1
    fi

    wd="$(sed -n 's/^WORKDIR=//p' "$m" | tail -1)"
    if [ -z "$wd" ]; then
        echo "probe never reported the lib's workdir"
        rc=1
    elif [ -d "$wd" ]; then
        echo "SIGTERM left the lib's own workdir behind: $wd"
        rm -rf "$wd"
        rc=1
    fi
    return "$rc"
}

assert "10a: a handler registered BEFORE nextest_absent_init still fires, after the lib's own teardown" _t10a
assert "10b: nextest_absent_cleanup lets a handler registered AFTER init tear the env down itself" _t10b
assert "10c: the composed trap is armed on INT/TERM/HUP as well as EXIT" _t10c
assert "10d: a timeout kill of a semaphore_wiring-shaped consumer removes the CALLER's temp dirs too" _t10d

test_summary
