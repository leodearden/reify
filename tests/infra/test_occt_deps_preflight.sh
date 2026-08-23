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
#   7. ACCEPTED SONAME: a Debian-shaped chain whose first-level link target
#      carries the FIRST value of OCCT_ACCEPTED_SONAMES => exit 0.
#   8. UNACCEPTED SONAME: version 0.0 (never a real OCCT release, so this case
#      survives any future pin bump) => non-zero, output names OCCT, the
#      resolved version, and the accepted set.
#   9. CONDA-SHAPED ONE-LEVEL SYMLINK: `libTKernel.so -> libTKernel.so.7.9.3`,
#      the exact layout live at /opt/reify-deps/lib => resolves to `7.9.3`,
#      not accepted, non-zero naming 7.9.3. Pins that the guard takes the
#      trailing segment VERBATIM, exactly as read_soname_version documents.
#  10. UNDETERMINABLE SONAME: `libTKernel.so` as a REGULAR FILE => non-zero.
#      This is the state where find() still reports the dir resolved (it only
#      tests .exists()), has_occt IS set, and build.rs silently falls back to
#      the literal string "7.8" — i.e. links a version nobody verified.
#  11. CROSS-ARTIFACT PIN: the version scripts/setup-dev.sh's OCCT block
#      expects from dpkg is a MEMBER of OCCT_ACCEPTED_SONAMES.
#
# The accepted-SONAME value is DERIVED from the guard, never hardcoded here, so
# a legitimate future pin bump stays a one-line diff in one file. Every derived
# parse asserts non-empty first.
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
SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"

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

# _mk_conda_lib_fixture <name> <version> — dir under $_TMPDIR reproducing the
# conda-forge / /opt/reify-deps layout: ONE level, `libTKernel.so ->
# libTKernel.so.<v>` where <v> is itself the full three-segment version. The
# first-level target's suffix is therefore the whole version verbatim.
_mk_conda_lib_fixture() {
    local d="$_TMPDIR/$1" v="$2"
    mkdir -p "$d"
    : > "$d/libTKernel.so.$v"
    ln -sfn "libTKernel.so.$v" "$d/libTKernel.so"
    printf '%s' "$d"
}

# _mk_plainfile_lib_fixture <name> — dir whose libTKernel.so is a REGULAR FILE,
# not a symlink. The sentinel exists (so find() resolves the dir and has_occt
# IS set) but no SONAME can be read from it.
_mk_plainfile_lib_fixture() {
    local d="$_TMPDIR/$1"
    mkdir -p "$d"
    : > "$d/libTKernel.so"
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

# _extract_bash_array <VAR> — elements of the named bash array, one per line,
# in declaration order, from shell source on stdin. Handles both the one-line
# `VAR=(a b c)` and the multi-line form.
_extract_bash_array() {
    awk -v var="$1" '
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
    '
}

# _bash_guard_array <VAR> — the named array declared anywhere in
# scripts/check-manifold-deps.sh.
_bash_guard_array() {
    _extract_bash_array "$1" < "$GUARD"
}

# _bash_occt_array <VAR> — the named array as declared INSIDE the
# `occt-candidates` marker block, so a same-named array elsewhere in the file
# can never satisfy the parity parse.
_bash_occt_array() {
    sed -n '/# BEGIN occt-candidates/,/# END occt-candidates/p' "$GUARD" \
        | _extract_bash_array "$1"
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

# _setup_dev_occt_version — the version scripts/setup-dev.sh's OCCT block
# expects back from dpkg. Anchored to that section's `# ---------- OCCT`
# banner and bounded by the next banner, so the parse cannot drift onto an
# unrelated `installed_ver` comparison elsewhere in the script.
_setup_dev_occt_version() {
    awk '
        index($0, "# ---------- OCCT") { insec = 1; next }
        insec && /^# ---------- / { exit }
        insec && index($0, "installed_ver") {
            if (match($0, /"\$installed_ver"[[:space:]]*=[[:space:]]*"[^"]+"/)) {
                seg = substr($0, RSTART, RLENGTH)
                n = split(seg, parts, "\"")
                print parts[4]
                exit
            }
        }
    ' "$SETUP_DEV"
}

# Derived once, up here, because section 5's positive-control fixture also has
# to carry an ACCEPTED version once the SONAME pin exists — hardcoding it in
# two places would turn a legitimate pin bump into a multi-file edit. The
# non-empty assert lives with the SONAME section below.
_ACCEPTED_SONAMES="$(_bash_guard_array OCCT_ACCEPTED_SONAMES)"
_ACCEPTED_FIRST="$(printf '%s\n' "$_ACCEPTED_SONAMES" | head -1)"

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

# Built at an ACCEPTED version: the SONAME pin is never reached by the
# include-only case (presence resolution fails first), but the positive control
# at the end of this section runs the guard to completion and must stay green
# across a legitimate future pin bump.
_LIB_OK="$(_mk_lib_fixture inconly-lib "$_ACCEPTED_FIRST")"
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

# ---------------------------------------------------------------------------
# 7. SONAME pin — the resolved version must be a declared, accepted one.
#
# All fixtures pair their lib dir with an include dir that DOES carry the
# header sentinel, so the presence arm is satisfied and the SONAME rule is the
# only thing under test.
# ---------------------------------------------------------------------------
echo ""
echo "--- 7: SONAME pin — accepted / unaccepted / conda-shaped / undeterminable ---"

assert "OCCT_ACCEPTED_SONAMES parses non-empty from the guard (anchor found, pin not vacuous)" \
    test -n "$_ACCEPTED_SONAMES"

_SON_INC="$(_mk_include_fixture soname-include)"

# 7 — accepted.
_SON_OK="$(_mk_lib_fixture soname-accepted "$_ACCEPTED_FIRST")"
assert "guard exits 0 for an accepted SONAME ('$_ACCEPTED_FIRST', Debian two-hop chain)" \
    _guard_exits_zero "$_SON_OK" "$_SON_INC"

# 8 — unaccepted. 0.0 can never be a legitimate OCCT release, so this case
# survives any future widening of the accepted set.
_SON_BAD="$(_mk_lib_fixture soname-unaccepted 0.0)"
assert "guard exits NON-zero for an unaccepted SONAME (0.0)" \
    _guard_exits_nonzero "$_SON_BAD" "$_SON_INC"

assert "guard output NAMES OCCT, the resolved version (0.0) and the accepted set" \
    _guard_output_names "$_SON_BAD" "$_SON_INC" "OCCT" "0.0" "$_ACCEPTED_FIRST"

# 9 — conda-shaped one-level symlink, the exact layout at /opt/reify-deps/lib.
_SON_CONDA="$(_mk_conda_lib_fixture soname-conda 7.9.3)"
assert "guard exits NON-zero for the conda one-level layout (libTKernel.so -> libTKernel.so.7.9.3)" \
    _guard_exits_nonzero "$_SON_CONDA" "$_SON_INC"

assert "guard output NAMES the verbatim trailing segment 7.9.3 (not 7.9, not 7)" \
    _guard_output_names "$_SON_CONDA" "$_SON_INC" "7.9.3"

# 10 — undeterminable: sentinel present but not a symlink. find() resolves the
# dir, has_occt IS set, and build.rs silently substitutes a hard-coded version.
_SON_PLAIN="$(_mk_plainfile_lib_fixture soname-plainfile)"
assert "guard exits NON-zero when libTKernel.so is a regular file (no readable SONAME)" \
    _guard_exits_nonzero "$_SON_PLAIN" "$_SON_INC"

assert "guard output says the SONAME could not be determined and names the path" \
    _guard_output_names "$_SON_PLAIN" "$_SON_INC" "could not determine" "$_SON_PLAIN/libTKernel.so"

# ---------------------------------------------------------------------------
# 8. CROSS-ARTIFACT PIN — setup-dev.sh's dpkg expectation vs. the accepted set.
#
# setup-dev.sh installs OCCT out of band and is not part of the verify plan, so
# nothing else forces the two to agree. If they drift, setup-dev.sh provisions
# a version the gate will then reject.
# ---------------------------------------------------------------------------
echo ""
echo "--- 8: cross-artifact pin — setup-dev.sh's OCCT version is in the accepted set ---"

_SETUP_DEV_VER="$(_setup_dev_occt_version)"

assert "scripts/setup-dev.sh exists" \
    test -f "$SETUP_DEV"

assert "setup-dev.sh's OCCT block yields a version (anchor '# ---------- OCCT' + installed_ver found)" \
    test -n "$_SETUP_DEV_VER"

if ! printf '%s\n' "$_ACCEPTED_SONAMES" | grep -qxF -- "$_SETUP_DEV_VER"; then
    echo "  OCCT version drift: setup-dev.sh provisions '$_SETUP_DEV_VER', accepted set is:"
    printf '%s\n' "$_ACCEPTED_SONAMES" | sed 's/^/    /'
fi
assert "setup-dev.sh's OCCT version ('$_SETUP_DEV_VER') is a member of OCCT_ACCEPTED_SONAMES" \
    bash -c 'printf "%s\n" "$1" | grep -qxF -- "$2"' _ "$_ACCEPTED_SONAMES" "$_SETUP_DEV_VER"

test_summary
