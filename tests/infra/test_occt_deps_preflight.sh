#!/usr/bin/env bash
# tests/infra/test_occt_deps_preflight.sh
#
# Guard for the OCCT arm of scripts/check-manifold-deps.sh (task #6343).
#
# WHY this guard exists. `reify_build_utils::find(NativeDep::Occt)` returns
# `None` when EITHER the header dir or the lib dir is unresolved, and
# `crates/reify-kernel-occt/build.rs` responds with a `cargo:warning` plus a
# bare `return` — no `has_occt` cfg. That silently DELETES the crate's
# `#[cfg(all(test, has_occt))]` modules and its ~25 `#![cfg(has_occt)]`
# integration binaries: the suite reports ZERO tests, not zero failures, and
# the verify gate stays green. This test pins the preflight that converts that
# vacuity into a red gate.
#
# Script under test: scripts/check-manifold-deps.sh (emitted by
# scripts/verify.sh as a plan entry whenever RUN_RUST=1, so the arm it guards
# runs on every --scope all / merge-gate verify).
#
# Assertions:
#   1. scripts/check-manifold-deps.sh exists and is executable.
#   2. BASELINE: with no OCCT env overrides the guard exits 0 on this host
#      (doubles as the live "is OCCT actually installed here" probe).
#   3. ABSENCE: both override dirs empty => non-zero, output names OCCT.
#   4. LIB-ONLY MISSING: headers present, libs absent => non-zero, output
#      names libTKernel.so and the offending lib dir.
#   5. INCLUDE-ONLY MISSING: libs present, headers absent => non-zero, output
#      names Standard_Failure.hxx and the offending include dir. This is the
#      mixed case that silently produces a stub build.
#   6. PARITY (anti-drift): the marker-delimited OCCT declarations in
#      scripts/check-manifold-deps.sh equal `NativeDep::Occt`'s arms in
#      crates/reify-build-utils/src/lib.rs — both candidate lists INCLUDING
#      ORDER (system paths must stay ahead of /opt/reify-deps' OCCT 7.9) and
#      both sentinel names. Rust is the source of truth; bash is a declared
#      mirror. Both parses must yield a non-empty result, so a renamed anchor
#      fails loudly instead of passing vacuously.
#
# Hermeticity: `pool`. Pure bash + filesystem. Every negative case is driven
# through the OCCT_LIB_DIR / OCCT_INCLUDE_DIR overrides the BUILD already
# honours, pointed at `mktemp -d` fixtures under $_TMPDIR — no bespoke
# test-only env seam, no cargo, no npm, no network, no host mutation. The
# guard is deliberately stricter than `find_dir_with_override` here (it
# demands the sentinel inside an override rather than trusting the path),
# which is exactly what makes these cases drivable.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

GUARD="$REPO_ROOT/scripts/check-manifold-deps.sh"
RUST_SRC="$REPO_ROOT/crates/reify-build-utils/src/lib.rs"

_TMPDIR="$(mktemp -d)"
trap 'rm -rf "$_TMPDIR"' EXIT

echo "=== OCCT deps preflight tests ==="

# ---------------------------------------------------------------------------
# Fixture + invocation helpers
# ---------------------------------------------------------------------------

# _mk_include_fixture <name> — mktemp-style dir under $_TMPDIR containing the
# OCCT include sentinel. Prints the path.
_mk_include_fixture() {
    local d="$_TMPDIR/$1"
    mkdir -p "$d"
    : > "$d/Standard_Failure.hxx"
    printf '%s' "$d"
}

# _mk_lib_fixture <name> <version> — dir under $_TMPDIR reproducing the Debian
# two-hop OCCT chain that reify_build_utils' own unit fixture models
# (crates/reify-build-utils/src/lib.rs, read_soname_version tests):
#   libTKernel.so -> libTKernel.so.<v> -> libTKernel.so.<v>.1
# so the FIRST-level link target's suffix is exactly <version>. Prints the path.
_mk_lib_fixture() {
    local d="$_TMPDIR/$1" v="$2"
    mkdir -p "$d"
    : > "$d/libTKernel.so.$v.1"
    ln -sfn "libTKernel.so.$v.1" "$d/libTKernel.so.$v"
    ln -sfn "libTKernel.so.$v" "$d/libTKernel.so"
    printf '%s' "$d"
}

# _mk_empty_fixture <name> — empty dir under $_TMPDIR. Prints the path.
_mk_empty_fixture() {
    local d="$_TMPDIR/$1"
    mkdir -p "$d"
    printf '%s' "$d"
}

# _guard_exits_zero <lib_dir> <include_dir>
_guard_exits_zero() {
    [ -x "$GUARD" ] || return 1
    OCCT_LIB_DIR="$1" OCCT_INCLUDE_DIR="$2" bash "$GUARD" >/dev/null
}

# _guard_exits_nonzero <lib_dir> <include_dir>
#
# Guarded on `-x "$GUARD"` first: a missing or unrunnable script also exits
# non-zero, which would otherwise false-pass every negation below.
_guard_exits_nonzero() {
    [ -x "$GUARD" ] || return 1
    ! OCCT_LIB_DIR="$1" OCCT_INCLUDE_DIR="$2" bash "$GUARD" >/dev/null 2>&1
}

# _guard_output_names <lib_dir> <include_dir> <needle>...
# Combined stdout+stderr of the guard must contain every needle (literal).
_guard_output_names() {
    local libdir="$1" incdir="$2"
    shift 2
    [ -x "$GUARD" ] || return 1
    local out needle
    out="$(OCCT_LIB_DIR="$libdir" OCCT_INCLUDE_DIR="$incdir" bash "$GUARD" 2>&1 || true)"
    for needle in "$@"; do
        printf '%s' "$out" | grep -qF -- "$needle" || return 1
    done
    return 0
}

# ---------------------------------------------------------------------------
# Parity parsers
#
# Both sides must FAIL LOUDLY (empty result -> a named assert failure) when
# their anchor is not found. A parity test that silently degrades to
# "" == "" would recreate the exact class of vacuity this task fixes.
# ---------------------------------------------------------------------------

# _rust_occt_list <fn-name> — the ordered `NativeDep::Occt => &[...]` string
# literals from the named fn block in crates/reify-build-utils/src/lib.rs, one
# per line. Anchored to `fn <name>` and bounded by that fn's closing brace, so
# the four `NativeDep::Occt =>` arms in the file can never be confused.
_rust_occt_list() {
    awk -v fname="fn $1" '
        index($0, fname) { infn = 1; next }
        infn && /^    }$/ { exit }
        infn && index($0, "NativeDep::Occt =>") { inarm = 1 }
        inarm {
            n = split($0, parts, "\"")
            for (i = 2; i <= n; i += 2) print parts[i]
            if (index($0, "]")) exit
        }
    ' "$RUST_SRC"
}

# _rust_occt_scalar <fn-name> — the single string literal on the
# `NativeDep::Occt =>` arm of the named fn block. Same anchoring rules.
_rust_occt_scalar() {
    awk -v fname="fn $1" '
        index($0, fname) { infn = 1; next }
        infn && /^    }$/ { exit }
        infn && index($0, "NativeDep::Occt =>") {
            n = split($0, parts, "\"")
            if (n >= 2) print parts[2]
            exit
        }
    ' "$RUST_SRC"
}

# _bash_occt_array <VAR> — elements of the named bash array declared inside
# scripts/check-manifold-deps.sh's `occt-candidates` marker block, one per
# line, in declaration order. Handles both one-line and multi-line array forms.
_bash_occt_array() {
    awk -v var="$1" '
        index($0, "# BEGIN occt-candidates") { inblk = 1; next }
        index($0, "# END occt-candidates") { exit }
        !inblk { next }
        !inarr && $0 ~ ("^[[:space:]]*" var "=\\(") {
            inarr = 1
            sub(/^[^(]*\(/, "")
        }
        inarr {
            line = $0
            sub(/#.*/, "", line)
            closed = (index(line, ")") > 0)
            sub(/\).*/, "", line)
            gsub(/"/, "", line)
            n = split(line, toks, /[[:space:]]+/)
            for (i = 1; i <= n; i++) if (toks[i] != "") print toks[i]
            if (closed) exit
        }
    ' "$GUARD"
}

# _bash_occt_scalar <VAR> — value of the named scalar assignment inside the
# `occt-candidates` marker block.
_bash_occt_scalar() {
    awk -v var="$1" '
        index($0, "# BEGIN occt-candidates") { inblk = 1; next }
        index($0, "# END occt-candidates") { exit }
        !inblk { next }
        $0 ~ ("^[[:space:]]*" var "=") {
            line = $0
            sub(/^[[:space:]]*[A-Za-z_][A-Za-z0-9_]*=/, "", line)
            sub(/[[:space:]]*#.*/, "", line)
            gsub(/"/, "", line)
            gsub(/^[[:space:]]+|[[:space:]]+$/, "", line)
            print line
            exit
        }
    ' "$GUARD"
}

# ---------------------------------------------------------------------------
# 1. Guard script exists and is executable
# ---------------------------------------------------------------------------
echo ""
echo "--- 1: scripts/check-manifold-deps.sh exists and is executable ---"

assert "scripts/check-manifold-deps.sh exists" \
    test -f "$GUARD"

assert "scripts/check-manifold-deps.sh is executable" \
    test -x "$GUARD"

# ---------------------------------------------------------------------------
# 2. BASELINE — the guard passes on this host with no OCCT overrides.
#    This IS the live "is OCCT installed" probe the task exists to add.
# ---------------------------------------------------------------------------
echo ""
echo "--- 2: baseline — guard exits 0 on the real host (OCCT present) ---"

assert "guard exits 0 with no OCCT env overrides (real host OCCT install)" \
    env -u OCCT_LIB_DIR -u OCCT_INCLUDE_DIR bash "$GUARD"

# ---------------------------------------------------------------------------
# 3. ABSENCE — neither headers nor libs resolvable
# ---------------------------------------------------------------------------
echo ""
echo "--- 3: absence — both OCCT dirs empty => red gate naming OCCT ---"

_EMPTY_LIB="$(_mk_empty_fixture absent-lib)"
_EMPTY_INC="$(_mk_empty_fixture absent-include)"

assert "guard exits NON-zero when both OCCT_LIB_DIR and OCCT_INCLUDE_DIR lack their sentinels" \
    _guard_exits_nonzero "$_EMPTY_LIB" "$_EMPTY_INC"

assert "guard output NAMES OCCT when neither half resolves" \
    _guard_output_names "$_EMPTY_LIB" "$_EMPTY_INC" "OCCT"

# ---------------------------------------------------------------------------
# 4. LIB-ONLY MISSING — headers resolve, libs do not
# ---------------------------------------------------------------------------
echo ""
echo "--- 4: lib-only missing => red gate naming libTKernel.so and the dir ---"

_INC_OK="$(_mk_include_fixture libonly-include)"
_LIB_MISSING="$(_mk_empty_fixture libonly-lib)"

assert "guard exits NON-zero when only the OCCT lib dir lacks libTKernel.so" \
    _guard_exits_nonzero "$_LIB_MISSING" "$_INC_OK"

assert "guard output NAMES libTKernel.so and the offending lib dir" \
    _guard_output_names "$_LIB_MISSING" "$_INC_OK" "libTKernel.so" "$_LIB_MISSING"

# ---------------------------------------------------------------------------
# 5. INCLUDE-ONLY MISSING — libs resolve, headers do not.
#    find() returns None when EITHER half is unresolved, so this mixed case
#    is a silent stub build today.
# ---------------------------------------------------------------------------
echo ""
echo "--- 5: include-only missing => red gate naming Standard_Failure.hxx ---"

# 7.8 is the measured first-level SONAME suffix on this host. The SONAME pin
# is never reached by this case (presence resolution fails first), so the value
# is incidental here — it just keeps the fixture shaped like a real install.
_LIB_OK="$(_mk_lib_fixture inconly-lib 7.8)"
_INC_MISSING="$(_mk_empty_fixture inconly-include)"

assert "guard exits NON-zero when only the OCCT include dir lacks Standard_Failure.hxx" \
    _guard_exits_nonzero "$_LIB_OK" "$_INC_MISSING"

assert "guard output NAMES Standard_Failure.hxx and the offending include dir" \
    _guard_output_names "$_LIB_OK" "$_INC_MISSING" "Standard_Failure.hxx" "$_INC_MISSING"

assert "guard exits 0 when BOTH override dirs carry their sentinels (positive control)" \
    _guard_exits_zero "$_LIB_OK" "$_INC_OK"

# ---------------------------------------------------------------------------
# 6. PARITY — the bash mirror equals NativeDep::Occt, order included
# ---------------------------------------------------------------------------
echo ""
echo "--- 6: parity — bash occt-candidates block mirrors NativeDep::Occt ---"

_RUST_LIB_CANDS="$(_rust_occt_list lib_candidates)"
_RUST_INC_CANDS="$(_rust_occt_list include_candidates)"
_RUST_LIB_SENT="$(_rust_occt_scalar lib_sentinel)"
_RUST_INC_SENT="$(_rust_occt_scalar include_sentinel)"

_BASH_LIB_CANDS="$(_bash_occt_array OCCT_LIB_CANDIDATES)"
_BASH_INC_CANDS="$(_bash_occt_array OCCT_INCLUDE_CANDIDATES)"
_BASH_LIB_SENT="$(_bash_occt_scalar OCCT_LIB_SENTINEL)"
_BASH_INC_SENT="$(_bash_occt_scalar OCCT_INCLUDE_SENTINEL)"

# Anchor-integrity asserts FIRST: without these, a renamed fn or a dropped
# marker block degrades every comparison below to "" == "" and the whole
# parity section passes while guarding nothing.
assert "Rust parse of NativeDep::Occt lib_candidates is non-empty (anchor 'fn lib_candidates' found)" \
    test -n "$_RUST_LIB_CANDS"
assert "Rust parse of NativeDep::Occt include_candidates is non-empty (anchor 'fn include_candidates' found)" \
    test -n "$_RUST_INC_CANDS"
assert "Rust parse of NativeDep::Occt lib_sentinel is non-empty (anchor 'fn lib_sentinel' found)" \
    test -n "$_RUST_LIB_SENT"
assert "Rust parse of NativeDep::Occt include_sentinel is non-empty (anchor 'fn include_sentinel' found)" \
    test -n "$_RUST_INC_SENT"
assert "bash parse of OCCT_LIB_CANDIDATES is non-empty (occt-candidates marker block found)" \
    test -n "$_BASH_LIB_CANDS"
assert "bash parse of OCCT_INCLUDE_CANDIDATES is non-empty (occt-candidates marker block found)" \
    test -n "$_BASH_INC_CANDS"
assert "bash parse of OCCT_LIB_SENTINEL is non-empty (occt-candidates marker block found)" \
    test -n "$_BASH_LIB_SENT"
assert "bash parse of OCCT_INCLUDE_SENTINEL is non-empty (occt-candidates marker block found)" \
    test -n "$_BASH_INC_SENT"

# Order-sensitive comparison: the priority order IS the invariant (system
# paths ahead of /opt/reify-deps/lib, which ships gmsh's transitive OCCT 7.9).
_parity_diff() {
    diff <(printf '%s\n' "$1") <(printf '%s\n' "$2") 2>&1 || true
}

_LIB_CAND_DIFF="$(_parity_diff "$_RUST_LIB_CANDS" "$_BASH_LIB_CANDS")"
if [ -n "$_LIB_CAND_DIFF" ]; then
    echo "  OCCT lib-candidate drift (< reify-build-utils, > check-manifold-deps.sh):"
    printf '%s\n' "$_LIB_CAND_DIFF" | sed 's/^/    /'
fi
assert "bash OCCT_LIB_CANDIDATES equals NativeDep::Occt lib_candidates, order included" \
    test -z "$_LIB_CAND_DIFF"

_INC_CAND_DIFF="$(_parity_diff "$_RUST_INC_CANDS" "$_BASH_INC_CANDS")"
if [ -n "$_INC_CAND_DIFF" ]; then
    echo "  OCCT include-candidate drift (< reify-build-utils, > check-manifold-deps.sh):"
    printf '%s\n' "$_INC_CAND_DIFF" | sed 's/^/    /'
fi
assert "bash OCCT_INCLUDE_CANDIDATES equals NativeDep::Occt include_candidates, order included" \
    test -z "$_INC_CAND_DIFF"

assert "bash OCCT_LIB_SENTINEL ('$_BASH_LIB_SENT') equals NativeDep::Occt lib_sentinel ('$_RUST_LIB_SENT')" \
    test "$_BASH_LIB_SENT" = "$_RUST_LIB_SENT"

assert "bash OCCT_INCLUDE_SENTINEL ('$_BASH_INC_SENT') equals NativeDep::Occt include_sentinel ('$_RUST_INC_SENT')" \
    test "$_BASH_INC_SENT" = "$_RUST_INC_SENT"

test_summary
