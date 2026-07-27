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

# -- Test 5: toolchain hygiene -------------------------------------------------
#
# These exist because dropping them cost a MEASURED 935 MB download in 12
# seconds (task 5599). On a rustup host `cargo` is a symlink to `rustup`, which
# derives its toolchain store from $RUSTUP_HOME and falls back to
# $HOME/.rustup — so redirecting HOME (element 3) without carrying RUSTUP_HOME
# across (element 5) strands the shim and provokes a full toolchain sync into
# the throwaway HOME on the first cargo invocation.
#
# Asserted AFTER Tests 3 and 4, so every check that actually exercises the env
# with a real cargo invocation has already had its chance to provoke a sync.
#
# Two separate asserts because they fail differently: 5a names the exact
# mechanism, 5b is the blunt backstop for any OTHER way the harness might start
# writing into a directory it advertises as throwaway.
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

# How many asserts nextest_absent_assert_real must emit. The ambient-header
# check is meaningful only where cargo-nextest is actually installed, so it is
# conditional — same convention as 3e.
NX_REAL_EXPECTED=6
[ "$NX_AMBIENT_HAS_NEXTEST" -eq 1 ] && NX_REAL_EXPECTED=7

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

# -- Test 8: the consolidation guard -------------------------------------------
#
# Every test above this line checks that the lib WORKS. None of them checks that
# anything USES it — a lib with perfect unit tests and zero call sites is a
# fifth copy of the harness, not a consolidation. Test 8 is the only assertion in
# this file that the refactor actually happened, and the only thing standing
# between "consolidated" and "silently re-accreted" six months from now.
#
# It is deliberately STATIC (a scan of the call sites' source) rather than
# behavioural, because the property is about provenance, not behaviour: a
# hand-rolled harness that happens to work identically is exactly the duplication
# this task exists to remove, and no runtime observation can distinguish it from
# the shared one.
#
# WRITTEN BEFORE ANY MIGRATION, so all four asserts are RED here and steps 14-17
# each turn exactly one green. A half-finished consolidation is then VISIBLE as
# "3 of 4 asserts pass" rather than silently indistinguishable from a finished
# one.
echo ""
echo "--- Test 8: the three simulation call sites are consolidated onto the lib ---"

# The escape hatch, mirroring the PTODO detector's `// ptodo:allow — reason`
# convention (docs/prds/reify-audit-ptodo-detector.md §8): a future call site
# with a genuine reason to hand-roll one of these constructs marks the line and
# says why, rather than being forced to either fake a migration or delete the
# guard. Anything unmarked is a re-accretion.
NX_ALLOW_MARKER='nextest-absent-lib:allow'

# The re-accretion patterns — each names ONE construct the lib now owns, and
# each is drawn from a line that was actually present in a call site before the
# migration (measured at task 5602 HEAD=8cb994e2fc), so none of them is a
# hypothetical:
#
#   PATH="...:/usr/bin:/bin"  the naive hermetic-PATH recipe. This is the one
#                             that matters most: it is the recipe the lib's
#                             header argues AGAINST (it strips the cargo bin dir
#                             wholesale, taking tree-sitter with it), so a
#                             re-accretion here does not merely duplicate the
#                             harness, it duplicates the BROKEN harness.
#                             (was: semaphore_wiring:186, probe:177)
#   HOME="$..."               a bespoke temp-HOME redirect; nx_run owns this,
#                             together with the CARGO_HOME unset and the
#                             RUSTUP_HOME carry-across that must accompany it.
#                             Anchored on a preceding line-start/space/`(` so it
#                             does not match CARGO_HOME=/RUSTUP_HOME=/TMP_HOME=.
#                             (was: semaphore_wiring:186, probe:176)
#   ln -s                     a local symlink-farm loop.
#                             (was: nextest_absent_suites:167)
_NX_BESPOKE_RE='PATH="[^"]*:/usr/bin:/bin"|(^|[[:space:](])HOME="\$|ln -s '

# _nx_bespoke_hits <path> — print `<lineno>:<line>` for every re-accretion.
#
# COMMENT-ONLY LINES ARE EXEMPT, and not as a convenience: step 14 keeps
# test_verify_nextest_absent_suites.sh's WHY-NOT-THE-NAIVE-RECIPE prose, which
# quotes `PATH="$STUB:/usr/bin:/bin"` verbatim in order to argue against it.
# That prose is the reason the lib is shaped the way it is and must survive the
# migration; flagging it would push the next author to delete the explanation
# instead of the duplication. A comment cannot construct a harness, so dropping
# comment-only lines costs the scan nothing.
#
# grep -n runs against the RAW file and the filtering happens after, so the
# reported line numbers are the real ones a reader can jump to.
_nx_bespoke_hits() {
    grep -nE "$_NX_BESPOKE_RE" "$1" \
        | grep -vE '^[0-9]+:[[:space:]]*#' \
        | grep -vF "$NX_ALLOW_MARKER" \
        || true
}

# _nx_lib_sourced <path> — does <path> source nextest_absent_lib.sh?
_nx_lib_sourced() {
    grep -qE '^[[:space:]]*(source|\.)[[:space:]].*nextest_absent_lib\.sh' "$1"
}

# _nx_is_consolidated <path> — BOTH conjuncts, because either alone is
# satisfiable without migrating: a file can source the lib and still run its old
# harness (a no-op import), or drop its harness while quietly reimplementing it
# under new variable names. Reported separately so a failure names which half.
_nx_is_consolidated() {
    local file="$1" name rc=0 hits
    name="$(basename "$file")"

    [ -f "$file" ] || { echo "no such file: $file"; return 1; }

    if ! _nx_lib_sourced "$file"; then
        echo "$name does NOT source tests/infra/nextest_absent_lib.sh"
        rc=1
    fi

    hits="$(_nx_bespoke_hits "$file")"
    if [ -n "$hits" ]; then
        echo "$name still hand-rolls a harness the shared lib owns:"
        printf '%s\n' "$hits"
        echo "  -> migrate onto nextest_absent_init / nx_run, or mark a genuinely"
        echo "     legitimate line with a trailing '# $NX_ALLOW_MARKER — <reason>'"
        rc=1
    fi

    if [ "$rc" -eq 0 ]; then
        echo "$name: sources the lib, no bespoke harness"
    fi
    return "$rc"
}

# test_verify_retry_subset.sh is the odd one out and is asserted separately.
# It has NO simulation harness to consolidate — verified at task 5602 planning
# time, at both the commit the task description cites (e932e2865b) and main:
# zero PATH/HOME/stub/farm constructs. The task description's claim that it is a
# fourth simulation harness is wrong, and that correction is filed as esc-5602-4.
# What it DOES duplicate is the AMBIENT plan-header probe, so that — and only
# that — is what its migration removes. Applying the harness patterns to it
# would assert something already true and test nothing.
_nx_retry_subset_consolidated() {
    local file="$SCRIPT_DIR/test_verify_retry_subset.sh"
    local rc=0 hits

    [ -f "$file" ] || { echo "no such file: $file"; return 1; }

    if ! _nx_lib_sourced "$file"; then
        echo "test_verify_retry_subset.sh does NOT source tests/infra/nextest_absent_lib.sh"
        rc=1
    fi

    hits="$(grep -nF '*"nextest=1"*' "$file" \
        | grep -vE '^[0-9]+:[[:space:]]*#' \
        | grep -vF "$NX_ALLOW_MARKER" || true)"
    if [ -n "$hits" ]; then
        echo "test_verify_retry_subset.sh still open-codes the ambient nextest=1 header case:"
        printf '%s\n' "$hits"
        echo "  -> replace with nextest_available_ambient, keeping NEXTEST_AVAILABLE"
        echo "     and every 'if [ \"\$NEXTEST_AVAILABLE\" -eq 1 ]' guard as they are"
        rc=1
    fi

    if [ "$rc" -eq 0 ]; then
        echo "test_verify_retry_subset.sh: sources the lib, ambient probe delegated"
    fi
    return "$rc"
}

assert "8a: test_verify_nextest_absent_suites.sh is consolidated onto nextest_absent_lib.sh" \
    _nx_is_consolidated "$SCRIPT_DIR/test_verify_nextest_absent_suites.sh"

assert "8b: test_verify_semaphore_wiring.sh is consolidated onto nextest_absent_lib.sh" \
    _nx_is_consolidated "$SCRIPT_DIR/test_verify_semaphore_wiring.sh"

assert "8c: test_verify_nextest_probe.sh is consolidated onto nextest_absent_lib.sh" \
    _nx_is_consolidated "$SCRIPT_DIR/test_verify_nextest_probe.sh"

assert "8d: test_verify_retry_subset.sh delegates its ambient nextest probe to the lib" \
    _nx_retry_subset_consolidated

# NON-VACUITY for the guard itself. Once steps 14-17 land, 8a-8d report green
# forever — and a scanner that CANNOT fail reports exactly the same green while
# a hand-rolled harness creeps back in. So the detector is pointed at synthetic
# files carrying the constructs it claims to detect, and required to answer
# correctly in BOTH directions. Fixtures live inside the lib's own workdir, so
# the lib's EXIT/INT/TERM/HUP trap removes them (same idiom as Test 4's child
# probe — a second trap would replace the first rather than compose with it).
NX_GUARD_DIR="$NX_WORKDIR/consolidation-guard-fixtures"
mkdir -p "$NX_GUARD_DIR"

# (e) The detector FIRES. Three fixtures, one per way a migration can be faked:
# harness kept, lib not sourced, and farm loop kept.
_t8e() {
    local rc=0 f

    f="$NX_GUARD_DIR/kept_harness.sh"
    cat > "$f" <<'FIXTURE'
source "$SCRIPT_DIR/nextest_absent_lib.sh"
OUT="$(HOME="$_TMP_HOME" PATH="$_STUB:/usr/bin:/bin" bash "$VERIFY" test --print-plan)"
FIXTURE
    if _nx_is_consolidated "$f" >/dev/null 2>&1; then
        echo "detector PASSED a file that sources the lib but kept its hand-rolled"
        echo "temp-HOME + naive-PATH harness — a no-op import reads as migrated."
        rc=1
    fi

    f="$NX_GUARD_DIR/no_source.sh"
    printf 'echo "clean, but never sources the lib"\n' > "$f"
    if _nx_is_consolidated "$f" >/dev/null 2>&1; then
        echo "detector PASSED a file that does not source the lib at all —"
        echo "the sources-lib conjunct is not being checked."
        rc=1
    fi

    f="$NX_GUARD_DIR/kept_farm.sh"
    cat > "$f" <<'FIXTURE'
source "$SCRIPT_DIR/nextest_absent_lib.sh"
for _e in "$CARGO_BIN_DIR"/*; do ln -s "$_e" "$FARM/$(basename "$_e")"; done
FIXTURE
    if _nx_is_consolidated "$f" >/dev/null 2>&1; then
        echo "detector PASSED a file that kept its local symlink-farm loop."
        rc=1
    fi

    [ "$rc" -eq 0 ] && echo "detector fires on all three fake-migration shapes"
    return "$rc"
}

# (f) ...and does NOT fire on the two exemptions, which is the half that keeps
# 8a-8d from being unsatisfiable-in-practice. Without the comment exemption the
# only way to make 8a green would be to delete the WHY-NOT-THE-NAIVE-RECIPE
# prose — i.e. the guard would destroy the explanation it depends on.
_t8f() {
    local rc=0 f

    f="$NX_GUARD_DIR/prose_only.sh"
    cat > "$f" <<'FIXTURE'
# WHY NOT THE NAIVE `PATH="$STUB:/usr/bin:/bin"` RECIPE: it strips the cargo bin
# dir wholesale, and `ln -s` farms are the lib's job now.
source "$SCRIPT_DIR/nextest_absent_lib.sh"
nextest_absent_init
FIXTURE
    if ! _nx_is_consolidated "$f" >/dev/null 2>&1; then
        echo "detector FIRED on comment-only prose. The migrated suites must keep"
        echo "their WHY-NOT-THE-NAIVE-RECIPE rationale, which quotes the very"
        echo "recipe it argues against:"
        _nx_bespoke_hits "$f"
        rc=1
    fi

    f="$NX_GUARD_DIR/escaped.sh"
    cat > "$f" <<FIXTURE
source "\$SCRIPT_DIR/nextest_absent_lib.sh"
OUT="\$(HOME="\$_T" bash "\$V")"  # $NX_ALLOW_MARKER — pins HOME-only behaviour
FIXTURE
    if ! _nx_is_consolidated "$f" >/dev/null 2>&1; then
        echo "detector FIRED on a line carrying the '$NX_ALLOW_MARKER' escape —"
        echo "a future legitimate exception would have no way to declare itself."
        _nx_bespoke_hits "$f"
        rc=1
    fi

    [ "$rc" -eq 0 ] && echo "comment prose and the allow-escape are both exempt"
    return "$rc"
}

assert "8e: the consolidation detector FIRES on a faked migration (kept harness / no source / kept farm)" _t8e
assert "8f: the detector is exempt on comment-only prose and on '# $NX_ALLOW_MARKER' lines" _t8f

test_summary
