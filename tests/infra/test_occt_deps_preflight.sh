#!/usr/bin/env bash
# tests/infra/test_occt_deps_preflight.sh
#
# Guard for the NATIVE-DEP PREFLIGHT arms of scripts/check-manifold-deps.sh —
# all three of them: OCCT (task #6343), Gmsh and OpenVDB (task #6493).
#
# FILENAME IS DELIBERATELY UNCHANGED despite the widened scope.
# tests/infra/run-all-classification.manifest and
# scripts/verify-pipeline-infra-tests.txt both already reference this path;
# renaming would churn two gates for no behavioural gain.
#
# WHY this guard exists: a missing native dep is SILENT, not merely cryptic,
# and all three build.rs scripts stay deliberately fail-OPEN — so the GATE
# lives outside the build, in scripts/check-manifold-deps.sh. That script's
# header states the mechanism under "THE SILENT-VACUITY RULE"; this file pins
# the preflight that converts the vacuity into a red gate.
#
# Script under test: scripts/check-manifold-deps.sh (emitted by
# scripts/verify.sh as a plan entry whenever RUN_RUST=1, so the arms it guards
# run on every --scope all / merge-gate verify).
#
# Sections:
#   0. SELF-CHECK, the harness itself. The needle-containment primitive
#      `_out_contains` must report a PRESENT needle as a match and a genuinely
#      absent one as a miss, and must contain no `|` pipeline at all
#      (deterministic structural pin on the hazard CLASS). A
#      `printf | grep -q` under this file's `set -o pipefail` reports a MATCH
#      as a MISS ~0.14% of the time — grep short-circuits, the builtin writer
#      takes SIGPIPE, pipefail surfaces the 141. Also pins that a needle miss
#      EMITS evidence, and that a match emits nothing (the green output shape
#      is byte-parsed by run_all.sh's cause_hint and dark-factory's
#      classifier).
#   R. READER: scripts/native_deps_table.py, driven only through its CLI. On
#      the real crates/reify-build-utils/native-deps.toml it exits 0, and its
#      eval'd output declares every row's columns as <ROW^^>_<COLUMN^^>. A
#      missing file, malformed TOML, a missing required column, an unknown
#      column and a non-identifier row key each exit non-zero naming the
#      offender. Values survive eval literally (no expansion, no word
#      splitting), and absent optional columns are emitted EMPTY, so a
#      same-named variable in the caller's environment cannot leak through.
#   1. scripts/check-manifold-deps.sh exists and is executable.
#   2. OCCT ABSENCE: both override dirs empty => non-zero, output names OCCT.
#   3. OCCT LIB-ONLY MISSING: headers present, libs absent => non-zero, output
#      names libTKernel.so and the offending lib dir.
#   4. OCCT INCLUDE-ONLY MISSING: libs present, headers absent => non-zero,
#      output names Standard_Failure.hxx and the offending include dir. This is
#      the mixed case that silently produces a stub build.
#   6. OCCT SONAME pin — ACCEPTED + RECORDING (a Debian-shaped chain carrying
#      the first OCCT_ACCEPTED_SONAMES value => exit 0, and the guard prints
#      the version and both dirs); PATCH-SHAPED (`-> ....<accepted>.1`, a
#      repackaging of the same OCCT => still exit 0, verbatim segment still
#      recorded — the pin is on MAJOR.MINOR precisely so this non-event cannot
#      hard-stop every RUN_RUST=1 verify); UNACCEPTED (0.0, never a real
#      release, so the case survives any future pin bump => non-zero naming the
#      version and the accepted set); CONDA-SHAPED one-hop (`-> ....7.9.3`, the
#      live /opt/reify-deps layout => names 7.9.3 VERBATIM, not 7.9);
#      UNDETERMINABLE (sentinel is a REGULAR FILE => non-zero, because find()
#      still reports the dir resolved, has_occt IS set, and build.rs falls back
#      to a hard-coded version — linking something nobody verified).
#   7. CROSS-ARTIFACT PIN: the version scripts/setup-dev.sh's OCCT block
#      expects from dpkg projects (major.minor) into OCCT_ACCEPTED_SONAMES.
#      Both sides are projected, so the accepted set stays free to hold a
#      three-segment SONAME even though setup-dev.sh's `grep -oP '\d+\.\d+'`
#      can only ever yield major.minor.
#   8. GMSH PRESENCE: lib-only missing / include-only missing => non-zero,
#      output names libgmsh.so or gmshc.h and the offending dir; both present
#      => exit 0.
#   9. OPENVDB PRESENCE: the same three, naming libopenvdb.so or the NESTED
#      openvdb/openvdb.h sentinel.
#  11. GREEN-PATH RECORDING for the two new arms: on the all-present path the
#      guard names each dep's resolved dirs AND the SONAME read from the
#      FIRST-level symlink target (gmsh one hop => 4.15.2; openvdb two hops =>
#      13.0, NOT the 13.0.0 `readlink -f` would give). RECORDED only —
#      deliberately not pinned to an accepted set, because unlike OCCT neither
#      build.rs splices a version into any link directive. The other half of
#      that asymmetry is pinned here too: a regular-file (non-symlink) dev
#      sentinel is FATAL for OCCT (section 6) but must stay NON-fatal for these
#      two — exit 0, recorded as `<Label> unknown at ` — driven from the same
#      _mk_plainfile_lib_fixture so the two expectations cannot be silently
#      unified by a later dedup.
#  12. EVERY ROW GATES: for each row of the real native-deps.toml (rows, env
#      names and sentinels all read through the reader), an EMPTY
#      <DEP>_LIB_DIR, then <DEP>_INCLUDE_DIR, with every other dep healthy,
#      reds the guard naming that row's own sentinel and the offending dir. A
#      row whose arm declares data but gates nothing fails here.
#  13. OCCT XDE TOOLKITS (step-assembly-import α): for each toolkit the occt
#      row's xde_toolkits lists, an OCCT lib dir lacking lib<TK>.so.<soname>
#      => non-zero, output names the missing file and the dir; a complete
#      install => exit 0 with every resolved toolkit file RECORDED. The OCCT
#      lib fixture builders model a complete install (they materialise every
#      toolkit), so every other OCCT positive control stays about what it
#      tests.
#  14. TABLE-DRIVEN DISPATCH, through fixture tables: exported-EMPTY overrides
#      fall through to the table's candidates (the bash half of the
#      empty-override rule); a fourth row with no arm reds naming the row; an
#      arm with no row reds naming the arm; a malformed or missing table reds
#      as unreadable naming the path; the occt row's snap table resolves a
#      fixture snap root, and the same layout without it does not resolve.
#
# The accepted-SONAME value is DERIVED from the guard, and every per-dep name
# from the reader, never hardcoded here, so a legitimate future pin bump or
# table edit stays a one-file diff. Every derived value asserts non-empty
# first.
#
# Hermeticity: `pool`. Pure bash + filesystem + the stdlib reader — no cargo,
# no npm, no network. Every case is driven through the <DEP>_LIB_DIR /
# <DEP>_INCLUDE_DIR overrides the BUILD already honours (each row's
# include_env / lib_env), pointed at `mktemp -d` fixtures under $_TMPDIR. The
# guard is deliberately stricter than the build here (it demands the sentinel
# inside an override rather than trusting the path), which is exactly what
# makes those cases drivable. The guard's ONE fixture seam of its own is
# REIFY_NATIVE_DEPS_TABLE, which points it at a fixture table; section 14 uses
# it to reach what no override can (a fourth row, a missing row, a malformed
# table, the snap fallback). This file unsets it at the top, so every other
# case reads the real table.
#
# KNOWN, DELIBERATE CAVEAT: check-manifold-deps.sh is ONE script with
# SEQUENTIAL arms — manifold prebuilt, tbb pin, OCCT, Gmsh, OpenVDB — and the
# first failure exits, so no arm runs unless every arm ahead of it passed.
# Consequences, in both directions:
#   - Every Gmsh case supplies healthy OCCT fixtures, and every OpenVDB case
#     supplies healthy OCCT *and* Gmsh fixtures. Without them a
#     `_guard_env_exits_nonzero` assert would PASS on an upstream arm's exit
#     and test nothing at all — the same vacuity class this file exists to
#     close. Every NEGATIVE case additionally pairs its exit-code assert with
#     an output assert naming a DEP-SPECIFIC string, so a failure that really
#     came from upstream stays attributable.
#   - The manifold-prebuilt and tbb-pin arms have no override seam, so every
#     positive control still depends on a healthy /opt/reify-deps — including
#     the tbb arm's `mkdir -p /opt/reify-deps/tbb-pin` self-heal, which writes
#     outside $_TMPDIR. A broken deps tree surfaces here as a preflight
#     failure naming the responsible arm.
# There is deliberately no unqualified live-host probe in this file:
# scripts/verify.sh already emits this guard as a plan entry on every
# RUN_RUST=1 verify, which is where "are these deps actually installed on this
# host" is answered for real.
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
SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"
READER="$REPO_ROOT/scripts/native_deps_table.py"
NATIVE_DEPS_TABLE="$REPO_ROOT/crates/reify-build-utils/native-deps.toml"

# An ambient value would point every real-table case below at another table.
unset REIFY_NATIVE_DEPS_TABLE

_TMPDIR="$(mktemp -d)"
trap 'rm -rf "$_TMPDIR"' EXIT

echo "=== native-dep preflight tests (OCCT + Gmsh + OpenVDB) ==="

# ---------------------------------------------------------------------------
# The real table, read ONCE through the reader the guard uses.
#
# Every per-dep name below — rows, env names, sentinels, snap subdirs, XDE
# toolkits — comes from these declarations rather than being listed again
# here. If the read fails there is nothing to derive the cases from, so the
# suite stops with a named failure instead of a bare `set -u` abort later.
# ---------------------------------------------------------------------------
_TABLE_RC=0
_TABLE_DECLS="$(python3 -I "$READER" "$NATIVE_DEPS_TABLE")" || _TABLE_RC=$?

_table_read_ok() {
    [ "$_TABLE_RC" -eq 0 ] && [ -n "$_TABLE_DECLS" ]
}

assert "reader exits 0 on the real native-deps.toml and declares its rows (exit $_TABLE_RC)" \
    _table_read_ok

if ! _table_read_ok; then
    echo "  every case below derives its per-dep names from that read; stopping"
    test_summary
    exit 1
fi
eval "$_TABLE_DECLS"

# The OCCT XDE toolkits the guard requires, from the occt row. Up here because
# every OCCT lib fixture below materialises them.
_OCCT_XDE_TOOLKITS=("${OCCT_XDE_TOOLKITS[@]}")

# ---------------------------------------------------------------------------
# Fixture + invocation helpers
# ---------------------------------------------------------------------------

# _mk_include_fixture <name> [<sentinel>] — dir under $_TMPDIR containing the
# named include sentinel (default: OCCT's Standard_Failure.hxx). Prints the
# path.
#
# The sentinel may be a NESTED path — OpenVDB's is `openvdb/openvdb.h`, not a
# bare filename — so its parent dirs are created too. A flat fixture would make
# the OpenVDB positive control fail for entirely the wrong reason.
_mk_include_fixture() {
    local d="$_TMPDIR/$1" sentinel="${2:-Standard_Failure.hxx}"
    local parent="$d"
    case "$sentinel" in */*) parent="$d/${sentinel%/*}" ;; esac
    mkdir -p "$parent"
    : > "$d/$sentinel"
    printf '%s' "$d"
}

# _mk_lib_fixture <name> <version> [<sentinel>] [<leaf>] — dir under $_TMPDIR
# reproducing the Debian TWO-HOP chain that reify_build_utils' own unit fixture
# models (crates/reify-build-utils/src/lib.rs, read_soname_version tests):
#   <sentinel> -> <sentinel>.<v> -> <sentinel>.<v>.<leaf>
# so the FIRST-level link target's suffix is exactly <version> (which is the
# whole point: `readlink -f` would yield <v>.<leaf> instead). Sentinel defaults
# to OCCT's libTKernel.so; leaf defaults to 1.
#
# <leaf> exists so a fixture can reproduce OpenVDB's LIVE chain at
# /opt/reify-deps/lib byte-for-byte — libopenvdb.so -> libopenvdb.so.13.0 ->
# libopenvdb.so.13.0.0 — rather than a near-miss ending in .1. Prints the path.
#
# An OCCT fixture (sentinel libTKernel.so) models a COMPLETE install: it also
# carries each _OCCT_XDE_TOOLKITS member as `lib<TK>.so.<v> ->
# lib<TK>.so.<v>.<leaf>`, the verbatim name build.rs links.
_mk_lib_fixture() {
    local d="$_TMPDIR/$1" v="$2" sentinel="${3:-libTKernel.so}" leaf="${4:-1}" tk
    mkdir -p "$d"
    : > "$d/$sentinel.$v.$leaf"
    ln -sfn "$sentinel.$v.$leaf" "$d/$sentinel.$v"
    ln -sfn "$sentinel.$v" "$d/$sentinel"
    if [ "$sentinel" = libTKernel.so ]; then
        for tk in "${_OCCT_XDE_TOOLKITS[@]}"; do
            : > "$d/lib$tk.so.$v.$leaf"
            ln -sfn "lib$tk.so.$v.$leaf" "$d/lib$tk.so.$v"
        done
    fi
    printf '%s' "$d"
}

# _mk_occt_lib_fixture_without <name> <version> <toolkit> — a healthy OCCT lib
# fixture at <version> with that one XDE toolkit's files removed. Prints the
# path.
_mk_occt_lib_fixture_without() {
    local d v="$2" tk="$3"
    d="$(_mk_lib_fixture "$1" "$v")"
    rm -f "$d/lib$tk.so.$v" "$d/lib$tk.so.$v.1"
    printf '%s' "$d"
}

# _mk_patchlink_lib_fixture <name> <majmin> — a repackaging that points the
# dev symlink ONE HOP FURTHER than Debian's, `libTKernel.so ->
# libTKernel.so.<majmin>.1`, so the first-level target's suffix is
# `<majmin>.1` on a functionally identical OCCT. The pin is on major.minor
# exactly so this shape stays green. The XDE toolkits carry the same
# `<majmin>.1` suffix, which is what build.rs would link verbatim.
_mk_patchlink_lib_fixture() {
    local d="$_TMPDIR/$1" v="$2" tk
    mkdir -p "$d"
    : > "$d/libTKernel.so.$v.1"
    ln -sfn "libTKernel.so.$v.1" "$d/libTKernel.so"
    for tk in "${_OCCT_XDE_TOOLKITS[@]}"; do
        : > "$d/lib$tk.so.$v.1"
    done
    printf '%s' "$d"
}

# _mk_conda_lib_fixture <name> <version> [<sentinel>] — dir under $_TMPDIR
# reproducing the conda-forge / /opt/reify-deps layout: ONE hop,
# `<sentinel> -> <sentinel>.<v>` where <v> is itself the full version. The
# first-level target's suffix is therefore the whole version verbatim. This is
# also gmsh's live shape at /opt/reify-deps/lib
# (libgmsh.so -> libgmsh.so.4.15.2). Sentinel defaults to OCCT's libTKernel.so.
_mk_conda_lib_fixture() {
    local d="$_TMPDIR/$1" v="$2" sentinel="${3:-libTKernel.so}"
    mkdir -p "$d"
    : > "$d/$sentinel.$v"
    ln -sfn "$sentinel.$v" "$d/$sentinel"
    printf '%s' "$d"
}

# _mk_plainfile_lib_fixture <name> [<sentinel>] — dir whose lib sentinel is a
# REGULAR FILE, not a symlink. The sentinel exists (so find() resolves the dir
# and the dep's has_* cfg IS set) but no SONAME can be read from it. Sentinel
# defaults to OCCT's libTKernel.so, matching the other _mk_*_lib_fixture
# helpers.
#
# The <sentinel> parameter exists so this one fixture can drive BOTH SIDES of
# the deliberate OCCT-vs-Gmsh/OpenVDB asymmetry on an undeterminable SONAME:
# fatal for OCCT (section 6), NON-fatal for gmsh and openvdb (section 11). Those
# are opposite expectations over the identical on-disk shape, so pinning them
# from the same fixture is what makes the asymmetry a tested contract rather
# than a comment. Prints the path.
_mk_plainfile_lib_fixture() {
    local d="$_TMPDIR/$1" sentinel="${2:-libTKernel.so}"
    mkdir -p "$d"
    : > "$d/$sentinel"
    printf '%s' "$d"
}

# _mk_empty_fixture <name> — empty dir under $_TMPDIR. Prints the path.
_mk_empty_fixture() {
    local d="$_TMPDIR/$1"
    mkdir -p "$d"
    printf '%s' "$d"
}

# --- SHARED HEALTHY DOWNSTREAM FIXTURES ------------------------------------
#
# Built ONCE, up here, and fed to EVERY green path in this file — including the
# OCCT ones in sections 4 and 6, which are textually far above the sections
# that test gmsh and openvdb.
#
# WHY, and it is not cosmetic: check-manifold-deps.sh is one script whose arms
# run in declaration order, and the OCCT arm no longer exits the script when it
# passes. So an OCCT positive control that supplies only OCCT_LIB_DIR /
# OCCT_INCLUDE_DIR runs the Gmsh and OpenVDB arms with NO overrides at all,
# resolving against live /opt/reify-deps. On a host where the conda env is
# absent or half-provisioned, an OCCT assert then goes RED naming OCCT and the
# OCCT fixture dirs — misattributed to the one dep it is not about. That is the
# same misattribution class this file's own design notes say must not happen,
# and section 8's Gmsh positive control already avoids it in the other
# direction; these fixtures apply the same reasoning backwards.
#
# The OCCT-positional wrappers below splice these in automatically, so the
# property holds BY CONSTRUCTION for every existing and future OCCT assert
# rather than one call site at a time.
#
# KNOWN CAVEAT, unchanged: the manifold-prebuilt and tbb-pin arms have no
# override seam at all, so they still read live host state. Gmsh and OpenVDB
# DO have one, which is exactly why leaning on live state for them is not
# excused by that caveat.

# Gmsh's live shape at /opt/reify-deps/lib is ONE hop
# (libgmsh.so -> libgmsh.so.4.15.2), so the fixture uses that layout.
_GMSH_LIB_OK="$(_mk_conda_lib_fixture gmsh-lib-ok 4.15.2 libgmsh.so)"
_GMSH_INC_OK="$(_mk_include_fixture gmsh-include-ok gmshc.h)"

# OpenVDB's live shape at /opt/reify-deps/lib is TWO hops
# (libopenvdb.so -> libopenvdb.so.13.0 -> libopenvdb.so.13.0.0), so the
# FIRST-level target yields `13.0` — not the `13.0.0` `readlink -f` would give.
# Its include sentinel is a NESTED path, `openvdb/openvdb.h`, not a bare
# filename; _mk_include_fixture creates the parent dir for it.
_OPENVDB_LIB_OK="$(_mk_lib_fixture openvdb-lib-ok 13.0 libopenvdb.so 0)"
_OPENVDB_INC_OK="$(_mk_include_fixture openvdb-include-ok openvdb/openvdb.h)"

_GMSH_OK=(GMSH_LIB_DIR="$_GMSH_LIB_OK" GMSH_INCLUDE_DIR="$_GMSH_INC_OK")
_OPENVDB_OK=(OPENVDB_LIB_DIR="$_OPENVDB_LIB_OK" OPENVDB_INCLUDE_DIR="$_OPENVDB_INC_OK")

# Every arm DOWNSTREAM of OCCT, healthy. Spliced into the OCCT-positional
# wrappers below.
_DOWNSTREAM_OK=("${_GMSH_OK[@]}" "${_OPENVDB_OK[@]}")

# --- dep-generic guard invocation ------------------------------------------
#
# check-manifold-deps.sh is ONE script with SEQUENTIAL arms — manifold
# prebuilt, tbb pin, OCCT, Gmsh, OpenVDB — and any arm exiting non-zero means
# every arm after it never runs. A `_guard_env_exits_nonzero` assert on a
# DOWNSTREAM dep would then PASS for entirely the wrong reason (the upstream
# arm's exit) and test nothing at all — the same vacuity class this whole file
# exists to close. So the env-list form below takes the FULL override set
# explicitly, and each dep's section supplies healthy fixtures for every arm
# ahead of it rather than relying on live host state.

# _guard_run <VAR=VALUE>... — run the guard under exactly these overrides.
# Combined stdout+stderr on stdout; the guard's own exit status is returned.
_guard_run() {
    env "$@" bash "$GUARD" 2>&1
}

# _guard_env_exits_zero <VAR=VALUE>...
_guard_env_exits_zero() {
    [ -x "$GUARD" ] || return 1
    _guard_run "$@" >/dev/null
}

# _guard_env_exits_nonzero <VAR=VALUE>...
#
# Guarded on `-x "$GUARD"` first: a missing or unrunnable script also exits
# non-zero, which would otherwise false-pass every negation below.
_guard_env_exits_nonzero() {
    [ -x "$GUARD" ] || return 1
    ! _guard_run "$@" >/dev/null
}

# _guard_env_output_names <VAR=VALUE>... -- <needle>...
# Combined stdout+stderr of the guard must contain every needle (literal).
#
# ON A MISS it ECHOES the offending needle, the guard's exit status, the full
# override set it ran under, and the entire captured guard output, then returns
# 1. WHY: test_helpers.sh's assert() dumps its per-assert tmpfile only when that
# file is non-empty (`[ -s "$_f" ]`), so a helper that swallows the guard output
# into a shell variable and returns 1 silently produces a FAIL line with NO
# evidence attached — the reader cannot distinguish "the guard printed the wrong
# thing" from "the guard printed nothing" from "the guard was right and the
# harness misreported it". That gap is what kept the pipefail/SIGPIPE defect
# (see _out_contains) unroot-caused for a full task cycle.
#
# Emission is on the FAILURE path ONLY, so an all-green suite stays
# byte-for-byte unchanged — run_all.sh's cause_hint and dark-factory's
# classifier both parse this file's green output shape. Every continuation line
# carries the NON-whitespace `  | ` prefix test_helpers.sh documents:
# dark-factory's slot-timeout classifier is `^[ \t]*`-anchored, so a captured
# @@REIFY_SLOT_TIMEOUT@@ sentinel reproduced at column 0 (or merely indented)
# would misclassify the whole merge verify as semaphore starvation.
_guard_env_output_names() {
    [ -x "$GUARD" ] || return 1
    local -a envs=()
    while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do
        envs+=("$1")
        shift
    done
    [ "${1:-}" = "--" ] && shift
    local out needle rc=0 e line
    out="$(_guard_run "${envs[@]}")" || rc=$?
    for needle in "$@"; do
        if ! _out_contains "$out" "$needle"; then
            echo "  | needle NOT FOUND in the guard output: $needle"
            echo "  | guard exit status: $rc"
            for e in "${envs[@]}"; do
                echo "  | override: $e"
            done
            echo "  | ---- captured guard output ----"
            # Fork-free, and deliberately NOT `printf | sed`: this is the
            # failure path, where losing the dump to a pipefail surprise is
            # worst. A herestring is not a pipeline, so nothing here is
            # exposed to the hazard _out_contains documents.
            while IFS= read -r line; do
                echo "  | $line"
            done <<< "$out"
            echo "  | ---- end captured guard output ----"
            return 1
        fi
    done
    return 0
}

# --- OCCT-positional wrappers ----------------------------------------------
# Thin adapters over the env-list forms above, kept so the OCCT sections read
# as they did before the file grew two more deps.
#
# EACH ONE SPLICES IN "${_DOWNSTREAM_OK[@]}" — the healthy Gmsh and OpenVDB
# fixtures built at the top of this file. That is what keeps an OCCT assert
# about OCCT: without it a positive control runs the two downstream arms with
# no overrides, resolving against live /opt/reify-deps, and a half-provisioned
# conda env reds an OCCT assert naming OCCT. Doing it HERE rather than at each
# call site makes the property hold for every future OCCT assert too, and it
# strengthens the negatives as well — a red now provably comes from the OCCT
# arm rather than from a downstream arm the case was not testing.

# _guard_exits_zero <lib_dir> <include_dir>
_guard_exits_zero() {
    _guard_env_exits_zero OCCT_LIB_DIR="$1" OCCT_INCLUDE_DIR="$2" "${_DOWNSTREAM_OK[@]}"
}

# _guard_exits_nonzero <lib_dir> <include_dir>
_guard_exits_nonzero() {
    _guard_env_exits_nonzero OCCT_LIB_DIR="$1" OCCT_INCLUDE_DIR="$2" "${_DOWNSTREAM_OK[@]}"
}

# _out_contains <haystack> <needle> — literal (grep -F semantics) substring
# containment, used by every output assert in this file.
#
# FORK-FREE BY CONSTRUCTION, and it must stay that way. The obvious
# `printf '%s' "$haystack" | grep -qF -- "$needle"` is WRONG under this file's
# `set -euo pipefail` (line 96): `grep -q` exits the instant it matches and
# closes the pipe, the bash-builtin `printf` writer is then killed by SIGPIPE
# (141), and pipefail makes the PIPELINE report printf's 141 even though grep
# exited 0 — so a MATCH is reported as a MISS. Measured over 20000 iterations
# on the real ~1.1KB guard payload: 28 misses, PIPESTATUS "141 0" (writer
# killed, grep succeeded) EVERY time — 0.14% per call, ~2.8% per run across
# this file's ~20 output asserts. That is the entire observed flake, and
# because the miss is silent it named a DIFFERENT assertion on each run.
#
# The `case` form has no pipeline, so no pipefail exposure at all, and no fork.
# The needle is QUOTED inside the pattern, which is what keeps matching LITERAL
# (grep -F semantics) rather than glob. Section 0's
# `_containment_has_no_pipeline` pins this deterministically — do not
# "simplify" it back into a pipeline.
_out_contains() {
    case "$1" in
        *"$2"*) return 0 ;;
        *)      return 1 ;;
    esac
}

# _lines_contain_exact <newline-separated-lines> <needle> — whole-LINE exact
# containment, i.e. `grep -qxF` semantics, with the same fork-free
# no-pipeline construction and for the same reason as _out_contains above.
# Both sides are wrapped in a newline so the pattern can only match a COMPLETE
# line, never a substring of one (which is exactly what `-x` buys).
_lines_contain_exact() {
    case $'\n'"$1"$'\n' in
        *$'\n'"$2"$'\n'*) return 0 ;;
        *)                return 1 ;;
    esac
}

# _guard_output_names <lib_dir> <include_dir> <needle>...
_guard_output_names() {
    local libdir="$1" incdir="$2"
    shift 2
    _guard_env_output_names OCCT_LIB_DIR="$libdir" OCCT_INCLUDE_DIR="$incdir" \
        "${_DOWNSTREAM_OK[@]}" -- "$@"
}

# _majmin_lines — MAJOR.MINOR projection of each non-empty line on stdin,
# mirroring the guard's occt_majmin(). Used for the cross-artifact pin so both
# sides are compared the way the guard itself compares them.
_majmin_lines() {
    local _v
    while IFS= read -r _v; do
        [ -n "$_v" ] || continue
        printf '%s\n' "$_v" | cut -d. -f1,2
    done
}

# ---------------------------------------------------------------------------
# Single-side parsers: data only the guard or setup-dev.sh declares.
#
# Each must FAIL LOUDLY (empty result -> a named assert failure) when its
# anchor is not found, so a renamed anchor cannot pass vacuously.
# ---------------------------------------------------------------------------

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

# Derived once, up here, because every OCCT positive-control fixture has to
# carry an ACCEPTED version — hardcoding it in two places would turn a
# legitimate pin bump into a multi-file edit. The non-empty assert lives with
# the SONAME section below.
_ACCEPTED_SONAMES="$(_bash_guard_array OCCT_ACCEPTED_SONAMES)"
_ACCEPTED_FIRST="$(printf '%s\n' "$_ACCEPTED_SONAMES" | head -1)"

# Healthy UPSTREAM OCCT, built at the DERIVED accepted version rather than a
# hardcoded 7.8, so a legitimate future pin bump stays a one-line diff in one
# file. Fed to every section that tests a DOWNSTREAM dep, for the mirror-image
# reason _DOWNSTREAM_OK exists: the OCCT arm runs FIRST, so without healthy
# OCCT overrides a `_guard_env_exits_nonzero` assert on gmsh or openvdb would
# pass on the OCCT arm's exit and test nothing at all.
#
# Defined here rather than beside _DOWNSTREAM_OK only because it needs
# $_ACCEPTED_FIRST, which is derived just above.
_UPSTREAM_OCCT_LIB="$(_mk_lib_fixture upstream-occt-lib "$_ACCEPTED_FIRST")"
_UPSTREAM_OCCT_INC="$(_mk_include_fixture upstream-occt-include)"
_OCCT_OK=(OCCT_LIB_DIR="$_UPSTREAM_OCCT_LIB" OCCT_INCLUDE_DIR="$_UPSTREAM_OCCT_INC")

# EVERY arm with an override seam, healthy. The all-green baseline that
# section 12's every-row loop perturbs one dep at a time.
_ALL_DEPS_OK=("${_OCCT_OK[@]}" "${_DOWNSTREAM_OK[@]}")

# ---------------------------------------------------------------------------
# 0. SELF-CHECK — the needle-containment primitive itself.
#
# Placed AHEAD of every guard case deliberately: a broken containment primitive
# would otherwise surface as a random guard assertion failing, and be
# attributed to the guard (or to /opt/reify-deps contention, or to arm
# ordering) rather than to the test harness. That misattribution is exactly
# what kept this flake unroot-caused for a full task cycle.
#
# WHAT IS UNDER TEST: `_out_contains` must report containment DETERMINISTICALLY.
# A `printf | grep -q` implementation does not: `grep -q` exits the instant it
# matches and closes the pipe, the bash-builtin `printf` writer is killed by
# SIGPIPE (141), and `set -o pipefail` (line 96 of this file) makes the
# PIPELINE report 141 — so a MATCH is returned as a MISS. Measured over 20000
# iterations on the real ~1.1KB guard payload: 28 misses, PIPESTATUS "141 0"
# (writer killed, grep succeeded) every time — 0.14% per call, ~2.8% per run
# across this file's ~20 output asserts.
#
# THE REGRESSION PIN IS STRUCTURAL, NOT STATISTICAL.
# `_containment_has_no_pipeline` reads the LIVE function body and refuses any
# `|` at all: the hazard CLASS — every pipeline, not just the one `grep`
# instance that happened to be measured — caught deterministically and
# instantly. An earlier draft also ran a 5000-call stress loop against a
# payload/needle pair proven to match; it was strictly weaker on every axis
# (documented ~9e-4 false-pass probability, ~1s on every RUN_RUST=1 verify, and
# blind to a non-grep pipeline) and is gone. What remains is deterministic: the
# needle IS present (pipefail-free oracle), the primitive says so, a genuinely
# absent needle still MISSES, and the body carries no pipeline.
# ---------------------------------------------------------------------------
echo ""
echo "--- 0: self-check — the needle-containment primitive is not itself flaky ---"

# A real guard payload (lib resolves, headers do not), captured ONCE — the same
# ~1.1KB stdout+stderr every output assert in this file is matched against.
_SELF_LIB_OK="$(_mk_lib_fixture selfcheck-lib "$_ACCEPTED_FIRST")"
_SELF_INC_MISSING="$(_mk_empty_fixture selfcheck-include)"
_SELF_PAYLOAD="$(OCCT_LIB_DIR="$_SELF_LIB_OK" OCCT_INCLUDE_DIR="$_SELF_INC_MISSING" bash "$GUARD" 2>&1 || true)"
_SELF_NEEDLE="Standard_Failure.hxx"

assert "self-check captured a non-empty real guard payload to match against" \
    test -n "$_SELF_PAYLOAD"

# Reference oracle, asserted BEFORE the primitive is exercised so the assert
# below can only fail because the primitive is broken, never because the needle
# is genuinely absent. Run under `bash -c` (default shell options — no
# pipefail) using bash's own fork-free `==` pattern match, which has no pipeline
# and cannot take SIGPIPE.
assert "self-check needle '$_SELF_NEEDLE' is genuinely PRESENT in that payload (bash [[ == * ]] oracle)" \
    bash -c '[[ "$1" == *"$2"* ]]' _ "$_SELF_PAYLOAD" "$_SELF_NEEDLE"

assert "_out_contains reports that genuinely PRESENT needle as a match" \
    _out_contains "$_SELF_PAYLOAD" "$_SELF_NEEDLE"

# Negative control, run in THIS shell (the primitive is a shell function, not
# an exported command, so it is not reachable from a `bash -c` child): the
# primitive must still MISS a needle that is genuinely absent. Without this, a
# bare `return 0` would satisfy the positive assert above.
_containment_negative_control() {
    ! _out_contains "$_SELF_PAYLOAD" "__no_such_needle_6493__"
}

assert "_out_contains still reports a genuinely ABSENT needle as a miss (negative control)" \
    _containment_negative_control

# _containment_has_no_pipeline — DETERMINISTIC structural pin on the hazard
# CLASS. `declare -f` re-renders the live function body, so this reads the
# primitive actually in force rather than a grep of the file, and it refuses
# ANY `|` — not merely the measured `| grep` — because every pipeline under
# this file's `set -o pipefail` carries the identical SIGPIPE exposure. This is
# what stops the fork-free form being "simplified" back into a pipeline.
#
# A `case` ALTERNATION pattern (`*a* | *b*)`) would trip it too. That is an
# accepted and LOUD false positive: the assert dumps the body it rejected, and
# a literal-containment primitive has no need of alternation.
_containment_has_no_pipeline() {
    local body
    body="$(declare -f _out_contains)" || return 1
    [ -n "$body" ] || return 1
    case "$body" in
        *'|'*)
            echo "_out_contains' body contains a pipeline ('|'):"
            printf '%s\n' "$body"
            return 1
            ;;
    esac
    return 0
}

assert "_out_contains' body contains no '|' pipeline at all (no pipefail/SIGPIPE exposure)" \
    _containment_has_no_pipeline

# --- A MISS must be DIAGNOSABLE, not silent.
#
# tests/infra/test_helpers.sh's assert() dumps captured evidence only when the
# checker actually wrote to its per-assert tmpfile (`[ -s "$_f" ]`). A
# `_guard_output_names` that swallows the guard output into a shell variable
# and then `return 1`s writes NOTHING, so the FAIL line reads
# "  FAIL: guard output NAMES <x>" with no captured-output block at all — the
# reader cannot tell whether the guard printed the wrong thing, printed
# nothing, or (as it turned out) printed exactly the right thing and the
# harness misreported it. That evidence gap is precisely why the SIGPIPE defect
# above survived a full task cycle unroot-caused.
#
# Behavioural, not prose: WHETHER evidence is emitted and whether it carries
# the two things a reader needs (which needle was missing, and what the guard
# actually said). Exact wording is deliberately not pinned.
_MISS_NEEDLE="__absent_needle_6493__"
_SELF_PAYLOAD_FIRST_LINE="${_SELF_PAYLOAD%%$'\n'*}"

assert "self-check has a non-empty first payload line to look for in the diagnostic" \
    test -n "$_SELF_PAYLOAD_FIRST_LINE"

# stdout AND stderr — the helper is free to use either; what matters is that
# assert()'s tmpfile (which captures both) ends up non-empty.
_MISS_DIAG="$(_guard_output_names "$_SELF_LIB_OK" "$_SELF_INC_MISSING" "$_MISS_NEEDLE" 2>&1 || true)"

assert "_guard_output_names EMITS evidence on a needle miss (assert's on-FAIL dump has something to show)" \
    test -n "$_MISS_DIAG"

assert "that evidence NAMES the needle that was missing ('$_MISS_NEEDLE')" \
    _out_contains "$_MISS_DIAG" "$_MISS_NEEDLE"

assert "that evidence carries the CAPTURED guard output (at least its first line)" \
    _out_contains "$_MISS_DIAG" "$_SELF_PAYLOAD_FIRST_LINE"

# The other half of the contract: an all-green run must stay byte-for-byte
# unchanged, because run_all.sh's cause_hint and dark-factory's classifier both
# parse this file's output shape. So the helper must emit on the FAILURE path
# only.
_no_emission_on_match() {
    local out
    out="$(_guard_output_names "$_SELF_LIB_OK" "$_SELF_INC_MISSING" "$_SELF_NEEDLE" 2>&1)" || return 1
    [ -z "$out" ] || {
        echo "_guard_output_names emitted on the SUCCESS path, which would change"
        echo "the green output shape run_all.sh and dark-factory parse:"
        printf '%s\n' "$out"
        return 1
    }
}

assert "_guard_output_names emits NOTHING when every needle matches (green shape unchanged)" \
    _no_emission_on_match

# ---------------------------------------------------------------------------
# R. READER — scripts/native_deps_table.py, through its CLI only.
#
# The guard evals this reader's stdout, so the reader is where the table's
# invariants are enforced: a malformed or hostile table must exit non-zero
# rather than emit declarations, and anything it does emit must be inert
# under eval. Every eval below runs in a SUBSHELL checker (`f() ( ... )`), so
# no declaration leaks into this file's own namespace.
# ---------------------------------------------------------------------------
echo ""
echo "--- R: reader — native_deps_table.py declares every row, rejects bad tables ---"

# _toml_min_row <key> [<omitted-column>] — a minimal VALID row for reader
# fixtures, every required column present (minus <omitted-column>, if given)
# and no optional one. Env and cfg names derive from <key>.
_toml_min_row() {
    local key="$1" omit="${2:-}" upper="${1^^}" col
    local -A cols=(
        [include_env]="\"${upper}_INCLUDE_DIR\""
        [lib_env]="\"${upper}_LIB_DIR\""
        [cfg]="\"has_${key}\""
        [include_candidates]='["/nonexistent/include"]'
        [lib_candidates]='["/nonexistent/lib"]'
        [include_sentinel]="\"${key}.h\""
        [lib_sentinel]="\"lib${key}.so\""
    )
    printf '[%s]\n' "$key"
    for col in include_env lib_env cfg include_candidates lib_candidates include_sentinel lib_sentinel; do
        [ "$col" = "$omit" ] && continue
        printf '%s = %s\n' "$col" "${cols[$col]}"
    done
}

# _reader_rejects <table> <needle>... — the reader exits NON-zero on <table>
# and its combined output names every needle. Guarded on the reader existing
# first: a missing reader also exits non-zero, which would false-pass every
# rejection below. On a miss it echoes the evidence, as _guard_env_output_names
# does.
_reader_rejects() {
    local table="$1" out rc=0 needle
    shift
    [ -f "$READER" ] || { echo "  | reader not found: $READER"; return 1; }
    out="$(python3 -I "$READER" "$table" 2>&1)" || rc=$?
    if [ "$rc" -eq 0 ]; then
        echo "  | reader exited 0 on a table it must reject: $table"
        echo "  | ---- reader output ----"
        printf '  | %s\n' "$out"
        return 1
    fi
    for needle in "$@"; do
        if ! _out_contains "$out" "$needle"; then
            echo "  | needle NOT FOUND in the reader output: $needle (exit $rc)"
            echo "  | ---- reader output ----"
            printf '  | %s\n' "$out"
            return 1
        fi
    done
}

# --- (a) the real table, as read and eval'd once at the top of this file ----

# _reader_real_declares_row <row> — NATIVE_DEP_ROWS holds <row>.
_reader_real_declares_row() (
    eval "$_TABLE_DECLS" || exit 1
    local r
    for r in "${NATIVE_DEP_ROWS[@]}"; do
        [ "$r" = "$1" ] && exit 0
    done
    echo "  | NATIVE_DEP_ROWS lacks '$1': (${NATIVE_DEP_ROWS[*]})"
    exit 1
)

for _row in occt gmsh openvdb; do
    assert "reader's NATIVE_DEP_ROWS on the real table contains '$_row'" \
        _reader_real_declares_row "$_row"
done

# _reader_real_rows_complete — every row the real table declares carries every
# required column, non-empty: the scalars, and both candidate arrays.
_reader_real_rows_complete() (
    eval "$_TABLE_DECLS" || exit 1
    local row up col name bad=0
    [ "${#NATIVE_DEP_ROWS[@]}" -gt 0 ] || { echo "  | NATIVE_DEP_ROWS is empty"; exit 1; }
    for row in "${NATIVE_DEP_ROWS[@]}"; do
        up="${row^^}"
        for col in INCLUDE_SENTINEL LIB_SENTINEL INCLUDE_ENV LIB_ENV CFG; do
            name="${up}_${col}"
            if [ -z "${!name:-}" ]; then
                echo "  | $name is empty or undeclared"
                bad=1
            fi
        done
        for col in INCLUDE_CANDIDATES LIB_CANDIDATES; do
            name="${up}_${col}"
            local -n arr="$name"
            if ! declare -p "$name" >/dev/null 2>&1 || [ "${#arr[@]}" -eq 0 ]; then
                echo "  | $name is an empty or undeclared array"
                bad=1
            fi
            unset -n arr
        done
    done
    exit "$bad"
)

assert "reader declares every row's sentinels, env names, cfg and both candidate arrays non-empty" \
    _reader_real_rows_complete

# --- (b)-(f) rejections -----------------------------------------------------
_READER_MISSING="$_TMPDIR/reader-no-such-table.toml"
assert "reader exits non-zero on a missing table file, naming the path" \
    _reader_rejects "$_READER_MISSING" "$_READER_MISSING"

_READER_MALFORMED="$_TMPDIR/reader-malformed.toml"
printf '[occt\n' > "$_READER_MALFORMED"
assert "reader exits non-zero on malformed TOML, naming the path" \
    _reader_rejects "$_READER_MALFORMED" "$_READER_MALFORMED"

_READER_NO_SENT="$_TMPDIR/reader-missing-column.toml"
_toml_min_row occt lib_sentinel > "$_READER_NO_SENT"
assert "reader exits non-zero on a row missing a required column, naming the row and 'lib_sentinel'" \
    _reader_rejects "$_READER_NO_SENT" "occt" "lib_sentinel"

_READER_UNKNOWN_COL="$_TMPDIR/reader-unknown-column.toml"
{ _toml_min_row occt; printf 'snapp = 1\n'; } > "$_READER_UNKNOWN_COL"
assert "reader exits non-zero on an unknown column, naming it ('snapp')" \
    _reader_rejects "$_READER_UNKNOWN_COL" "snapp"

_READER_BAD_KEY="$_TMPDIR/reader-bad-key.toml"
_toml_min_row open-vdb > "$_READER_BAD_KEY"
assert "reader exits non-zero on a row key that is not [a-z][a-z0-9]* ('open-vdb')" \
    _reader_rejects "$_READER_BAD_KEY" "open-vdb"

# --- (g) values survive eval literally -------------------------------------
_READER_QUOTE_MARKER="$_TMPDIR/reader-quote-marker"
_READER_QUOTE_CAND="/tmp/a b/\$(touch $_READER_QUOTE_MARKER)/it's"
_READER_QUOTE="$_TMPDIR/reader-quoting.toml"
{
    _toml_min_row quote include_candidates
    printf 'include_candidates = ["%s"]\n' "$_READER_QUOTE_CAND"
} > "$_READER_QUOTE"

# _reader_value_is_literal — the hostile candidate comes back as ONE element
# equal to the literal string, and the command substitution inside it never
# ran.
_reader_value_is_literal() (
    local decls
    decls="$(python3 -I "$READER" "$_READER_QUOTE")" || exit 1
    eval "$decls" || exit 1
    if [ "${#QUOTE_INCLUDE_CANDIDATES[@]}" -ne 1 ] \
        || [ "${QUOTE_INCLUDE_CANDIDATES[0]}" != "$_READER_QUOTE_CAND" ]; then
        echo "  | QUOTE_INCLUDE_CANDIDATES after eval: $(declare -p QUOTE_INCLUDE_CANDIDATES)"
        exit 1
    fi
    if [ -e "$_READER_QUOTE_MARKER" ]; then
        echo "  | eval EXECUTED the embedded \$(touch ...): $_READER_QUOTE_MARKER exists"
        exit 1
    fi
)

assert "a candidate holding a space, \$(...) and a single quote survives eval as one literal element, unexecuted" \
    _reader_value_is_literal

# --- (h) absent optional columns are emitted EMPTY -------------------------
_READER_NO_OPT="$_TMPDIR/reader-no-optional.toml"
_toml_min_row gmsh > "$_READER_NO_OPT"

# _reader_absent_optionals_are_empty — with same-named variables EXPORTED in
# the caller's environment, the eval still leaves the snap scalars empty and
# the XDE toolkit list an empty array: the reader declared them, so nothing
# ambient leaks into what the guard reads.
_reader_absent_optionals_are_empty() (
    export GMSH_SNAP_ROOT=/leak GMSH_SNAP_INCLUDE_SUBDIR=/leak \
        GMSH_SNAP_LIB_SUBDIR=/leak GMSH_XDE_TOOLKITS=/leak
    local decls name bad=0
    decls="$(python3 -I "$READER" "$_READER_NO_OPT")" || exit 1
    eval "$decls" || exit 1
    for name in GMSH_SNAP_ROOT GMSH_SNAP_INCLUDE_SUBDIR GMSH_SNAP_LIB_SUBDIR; do
        if [ -n "${!name}" ]; then
            echo "  | $name leaked through: '${!name}'"
            bad=1
        fi
    done
    if [ "${#GMSH_XDE_TOOLKITS[@]}" -ne 0 ] || [ -n "${GMSH_XDE_TOOLKITS[0]:-}" ]; then
        echo "  | GMSH_XDE_TOOLKITS is not an empty array: $(declare -p GMSH_XDE_TOOLKITS)"
        bad=1
    fi
    exit "$bad"
)

assert "a row with no snap / xde_toolkits emits them EMPTY, overriding same-named exported variables" \
    _reader_absent_optionals_are_empty

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
# 2. ABSENCE — neither headers nor libs resolvable
# ---------------------------------------------------------------------------
echo ""
echo "--- 2: absence — both OCCT dirs empty => red gate naming OCCT ---"

_EMPTY_LIB="$(_mk_empty_fixture absent-lib)"
_EMPTY_INC="$(_mk_empty_fixture absent-include)"

assert "guard exits NON-zero when both OCCT_LIB_DIR and OCCT_INCLUDE_DIR lack their sentinels" \
    _guard_exits_nonzero "$_EMPTY_LIB" "$_EMPTY_INC"

assert "guard output NAMES OCCT when neither half resolves" \
    _guard_output_names "$_EMPTY_LIB" "$_EMPTY_INC" "OCCT"

# ---------------------------------------------------------------------------
# 3. LIB-ONLY MISSING — headers resolve, libs do not
# ---------------------------------------------------------------------------
echo ""
echo "--- 3: lib-only missing => red gate naming libTKernel.so and the dir ---"

_INC_OK="$(_mk_include_fixture libonly-include)"
_LIB_MISSING="$(_mk_empty_fixture libonly-lib)"

assert "guard exits NON-zero when only the OCCT lib dir lacks libTKernel.so" \
    _guard_exits_nonzero "$_LIB_MISSING" "$_INC_OK"

assert "guard output NAMES libTKernel.so and the offending lib dir" \
    _guard_output_names "$_LIB_MISSING" "$_INC_OK" "libTKernel.so" "$_LIB_MISSING"

# ---------------------------------------------------------------------------
# 4. INCLUDE-ONLY MISSING — libs resolve, headers do not.
#    find() returns None when EITHER half is unresolved, so this mixed case
#    is a silent stub build today.
# ---------------------------------------------------------------------------
echo ""
echo "--- 4: include-only missing => red gate naming Standard_Failure.hxx ---"

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
# 6. SONAME pin — the resolved version must be a declared, accepted one.
#
# All fixtures pair their lib dir with an include dir that DOES carry the
# header sentinel, so the presence arm is satisfied and the SONAME rule is the
# only thing under test.
# ---------------------------------------------------------------------------
echo ""
echo "--- 6: SONAME pin — accepted / unaccepted / conda-shaped / undeterminable ---"

assert "OCCT_ACCEPTED_SONAMES parses non-empty from the guard (anchor found, pin not vacuous)" \
    test -n "$_ACCEPTED_SONAMES"

_SON_INC="$(_mk_include_fixture soname-include)"

# 7 — accepted, and RECORDED.
_SON_OK="$(_mk_lib_fixture soname-accepted "$_ACCEPTED_FIRST")"
assert "guard exits 0 for an accepted SONAME ('$_ACCEPTED_FIRST', Debian two-hop chain)" \
    _guard_exits_zero "$_SON_OK" "$_SON_INC"

# The RECORDING half of the arm. Every other green-path assert runs through
# _guard_exits_zero, which discards stdout — so without this one, deleting the
# guard's [ok] line (or regressing it to name the wrong dir) leaves the whole
# suite passing: the silent-vacuity rule one level up.
#
# The needle carries the trailing " at " so this arm's FIRST-LEVEL-only rule is
# pinned HERE and not merely transitively. The fixture is a Debian two-hop
# chain whose leaf is `libTKernel.so.<accepted>.1`, so a `readlink -f`
# regression in dep_soname_ver would record "OCCT <accepted>.1 at " — and
# "OCCT <accepted>" alone is a PREFIX of that and would pass either way.
# Section 11's OpenVDB needle carries the separator for the identical reason;
# without it here, that one assert would be the only thing covering the shared
# helper's rule, which evaporates the moment anyone re-splits it per dep.
assert "guard RECORDS the resolved OCCT version and both resolved dirs on the green path" \
    _guard_output_names "$_SON_OK" "$_SON_INC" "OCCT $_ACCEPTED_FIRST at " "$_SON_OK" "$_SON_INC"

# 8 — patch-shaped: the dev symlink points one hop further on a functionally
# identical OCCT. Exact-matching the verbatim segment would make this a red
# gate on every RUN_RUST=1 verify; the major.minor projection keeps it green,
# and build.rs links the verbatim `libTKernel.so.<v>.1`, which exists.
_SON_PATCH="$(_mk_patchlink_lib_fixture soname-patchlink "$_ACCEPTED_FIRST")"
assert "guard exits 0 for a patch-shaped link target ('$_ACCEPTED_FIRST.1' — same OCCT, different packaging)" \
    _guard_exits_zero "$_SON_PATCH" "$_SON_INC"

assert "guard RECORDS the verbatim patch-shaped segment ('$_ACCEPTED_FIRST.1'), not just the accepted major.minor" \
    _guard_output_names "$_SON_PATCH" "$_SON_INC" "OCCT $_ACCEPTED_FIRST.1"

# 9 — unaccepted. 0.0 can never be a legitimate OCCT release, so this case
# survives any future widening of the accepted set.
_SON_BAD="$(_mk_lib_fixture soname-unaccepted 0.0)"
assert "guard exits NON-zero for an unaccepted SONAME (0.0)" \
    _guard_exits_nonzero "$_SON_BAD" "$_SON_INC"

assert "guard output NAMES OCCT, the resolved version (0.0) and the accepted set" \
    _guard_output_names "$_SON_BAD" "$_SON_INC" "OCCT" "0.0" "$_ACCEPTED_FIRST"

# 10 — conda-shaped one-level symlink, the exact layout at /opt/reify-deps/lib.
_SON_CONDA="$(_mk_conda_lib_fixture soname-conda 7.9.3)"
assert "guard exits NON-zero for the conda one-level layout (libTKernel.so -> libTKernel.so.7.9.3)" \
    _guard_exits_nonzero "$_SON_CONDA" "$_SON_INC"

assert "guard output NAMES the verbatim trailing segment 7.9.3 (not 7.9, not 7)" \
    _guard_output_names "$_SON_CONDA" "$_SON_INC" "7.9.3"

# 11 — undeterminable: sentinel present but not a symlink. find() resolves the
# dir, has_occt IS set, and build.rs silently substitutes a hard-coded version.
_SON_PLAIN="$(_mk_plainfile_lib_fixture soname-plainfile)"
assert "guard exits NON-zero when libTKernel.so is a regular file (no readable SONAME)" \
    _guard_exits_nonzero "$_SON_PLAIN" "$_SON_INC"

assert "guard output says the SONAME could not be determined and names the path" \
    _guard_output_names "$_SON_PLAIN" "$_SON_INC" "could not determine" "$_SON_PLAIN/libTKernel.so"

# ---------------------------------------------------------------------------
# 7. CROSS-ARTIFACT PIN — setup-dev.sh's dpkg expectation vs. the accepted set.
#
# setup-dev.sh installs OCCT out of band and is not part of the verify plan, so
# nothing else forces the two to agree. If they drift, setup-dev.sh provisions
# a version the gate will then reject.
# ---------------------------------------------------------------------------
echo ""
echo "--- 7: cross-artifact pin — setup-dev.sh's OCCT version is in the accepted set ---"

_SETUP_DEV_VER="$(_setup_dev_occt_version)"

assert "scripts/setup-dev.sh exists" \
    test -f "$SETUP_DEV"

assert "setup-dev.sh's OCCT block yields a version (anchor '# ---------- OCCT' + installed_ver found)" \
    test -n "$_SETUP_DEV_VER"

# Compared the way the GUARD compares: both sides projected to major.minor.
# setup-dev.sh's dpkg parse is `grep -oP '\d+\.\d+'`, so its value can only
# ever be major.minor — matching it against a raw accepted set would forbid the
# set from ever holding a three-segment SONAME (the conda-shaped `7.9.3` the
# guard itself demonstrates) without editing setup-dev.sh to a string its regex
# cannot produce.
_ACCEPTED_MAJMIN="$(printf '%s\n' "$_ACCEPTED_SONAMES" | _majmin_lines)"
_SETUP_DEV_MAJMIN="$(printf '%s\n' "$_SETUP_DEV_VER" | _majmin_lines)"

if ! _lines_contain_exact "$_ACCEPTED_MAJMIN" "$_SETUP_DEV_MAJMIN"; then
    echo "  OCCT version drift: setup-dev.sh provisions '$_SETUP_DEV_VER' (major.minor"
    echo "  $_SETUP_DEV_MAJMIN), accepted set projects to:"
    printf '%s\n' "$_ACCEPTED_MAJMIN" | sed 's/^/    /'
fi
# Deliberately left as a pipeline: unlike the diagnostic above, this one runs
# in a FRESH `bash -c` child, which starts with default shell options — VERIFIED
# `pipefail off` there, and shell options are not inherited across a `bash -c`.
# With pipefail off the pipeline reports grep's status, so a SIGPIPE'd printf
# cannot turn a match into a miss. Routing it through _lines_contain_exact is
# not possible anyway: that is a shell function, not reachable from the child.
assert "setup-dev.sh's OCCT version ('$_SETUP_DEV_VER') projects (major.minor) into OCCT_ACCEPTED_SONAMES" \
    bash -c 'printf "%s\n" "$1" | grep -qxF -- "$2"' _ "$_ACCEPTED_MAJMIN" "$_SETUP_DEV_MAJMIN"

# ---------------------------------------------------------------------------
# 8. GMSH PRESENCE — the silent-vacuity rule, one dep over (task #6493).
#
# crates/reify-kernel-gmsh/build.rs is byte-for-byte the fail-OPEN
# find()/warning/return shape OCCT had before task #6343, and 81
# has_gmsh-gated items vanish with it. The GATE therefore lives in
# check-manifold-deps.sh, exactly as it does for OCCT.
#
# EVERY case supplies healthy OCCT overrides, for the sequential-arms reason
# this file's KNOWN CAVEAT states: without them a `_guard_env_exits_nonzero`
# assert would pass on the OCCT arm's exit and test nothing. Every negative
# case pairs its exit-code assert with an output assert naming a GMSH-specific
# string, so a failure that really came from upstream stays attributable.
# ---------------------------------------------------------------------------
echo ""
echo "--- 8: gmsh presence — missing libs / headers => red gate naming gmsh ---"

# The HEALTHY fixtures for all three deps ($_OCCT_OK, $_GMSH_LIB_OK /
# $_GMSH_INC_OK, $_OPENVDB_OK) are built once at the top of this file, because
# the OCCT sections above need the downstream ones too. Only the MISSING
# fixtures — which are specific to these cases — are built here.
_GMSH_LIB_MISSING="$(_mk_empty_fixture gmsh-lib-missing)"
_GMSH_INC_MISSING="$(_mk_empty_fixture gmsh-include-missing)"
_OPENVDB_LIB_MISSING="$(_mk_empty_fixture openvdb-lib-missing)"
_OPENVDB_INC_MISSING="$(_mk_empty_fixture openvdb-include-missing)"

assert "guard exits NON-zero when the Gmsh lib dir lacks libgmsh.so (headers present)" \
    _guard_env_exits_nonzero "${_OCCT_OK[@]}" \
        GMSH_LIB_DIR="$_GMSH_LIB_MISSING" GMSH_INCLUDE_DIR="$_GMSH_INC_OK"

assert "guard output NAMES libgmsh.so and the offending Gmsh lib dir" \
    _guard_env_output_names "${_OCCT_OK[@]}" \
        GMSH_LIB_DIR="$_GMSH_LIB_MISSING" GMSH_INCLUDE_DIR="$_GMSH_INC_OK" \
        -- "libgmsh.so" "$_GMSH_LIB_MISSING"

assert "guard exits NON-zero when the Gmsh include dir lacks gmshc.h (libs present)" \
    _guard_env_exits_nonzero "${_OCCT_OK[@]}" \
        GMSH_LIB_DIR="$_GMSH_LIB_OK" GMSH_INCLUDE_DIR="$_GMSH_INC_MISSING"

assert "guard output NAMES gmshc.h and the offending Gmsh include dir" \
    _guard_env_output_names "${_OCCT_OK[@]}" \
        GMSH_LIB_DIR="$_GMSH_LIB_OK" GMSH_INCLUDE_DIR="$_GMSH_INC_MISSING" \
        -- "gmshc.h" "$_GMSH_INC_MISSING"

# The Gmsh positive control runs the guard to COMPLETION, so it also transits
# the OpenVDB arm that section 9 adds downstream. It therefore supplies healthy
# OpenVDB fixtures too ($_OPENVDB_OK, hoisted to the top of this file) —
# leaning on live /opt/reify-deps state here would make a green Gmsh result
# depend on a dep this section is not testing.

assert "guard exits 0 when BOTH Gmsh override dirs carry their sentinels (positive control)" \
    _guard_env_exits_zero "${_OCCT_OK[@]}" "${_OPENVDB_OK[@]}" \
        GMSH_LIB_DIR="$_GMSH_LIB_OK" GMSH_INCLUDE_DIR="$_GMSH_INC_OK"

# ---------------------------------------------------------------------------
# 9. OPENVDB PRESENCE — the third instance of the same rule (task #6493).
#
# crates/reify-kernel-openvdb/build.rs is byte-for-byte gmsh's fail-OPEN shape,
# so the GATE lives in check-manifold-deps.sh for the same reason.
#
# EVERY case supplies healthy OCCT *and* Gmsh overrides: both arms run AHEAD of
# the OpenVDB arm in the same script, so without them a
# `_guard_env_exits_nonzero` assert would pass on an upstream arm's exit and
# test nothing. Every negative pairs its exit-code assert with an output assert
# naming an OPENVDB-specific string, so an upstream failure stays attributable.
# ---------------------------------------------------------------------------
echo ""
echo "--- 9: openvdb presence — missing libs / headers => red gate naming openvdb ---"

# $_GMSH_OK (healthy Gmsh overrides) is hoisted to the top of this file.

assert "guard exits NON-zero when the OpenVDB lib dir lacks libopenvdb.so (headers present)" \
    _guard_env_exits_nonzero "${_OCCT_OK[@]}" "${_GMSH_OK[@]}" \
        OPENVDB_LIB_DIR="$_OPENVDB_LIB_MISSING" OPENVDB_INCLUDE_DIR="$_OPENVDB_INC_OK"

assert "guard output NAMES libopenvdb.so and the offending OpenVDB lib dir" \
    _guard_env_output_names "${_OCCT_OK[@]}" "${_GMSH_OK[@]}" \
        OPENVDB_LIB_DIR="$_OPENVDB_LIB_MISSING" OPENVDB_INCLUDE_DIR="$_OPENVDB_INC_OK" \
        -- "libopenvdb.so" "$_OPENVDB_LIB_MISSING"

assert "guard exits NON-zero when the OpenVDB include dir lacks openvdb/openvdb.h (libs present)" \
    _guard_env_exits_nonzero "${_OCCT_OK[@]}" "${_GMSH_OK[@]}" \
        OPENVDB_LIB_DIR="$_OPENVDB_LIB_OK" OPENVDB_INCLUDE_DIR="$_OPENVDB_INC_MISSING"

assert "guard output NAMES openvdb/openvdb.h and the offending OpenVDB include dir" \
    _guard_env_output_names "${_OCCT_OK[@]}" "${_GMSH_OK[@]}" \
        OPENVDB_LIB_DIR="$_OPENVDB_LIB_OK" OPENVDB_INCLUDE_DIR="$_OPENVDB_INC_MISSING" \
        -- "openvdb/openvdb.h" "$_OPENVDB_INC_MISSING"

assert "guard exits 0 when BOTH OpenVDB override dirs carry their sentinels (positive control)" \
    _guard_env_exits_zero "${_OCCT_OK[@]}" "${_GMSH_OK[@]}" \
        OPENVDB_LIB_DIR="$_OPENVDB_LIB_OK" OPENVDB_INCLUDE_DIR="$_OPENVDB_INC_OK"

# ---------------------------------------------------------------------------
# 11. GREEN-PATH RECORDING for the two new arms (task #6493).
#
# Sections 8 and 9 pin only FAILURE paths, so a green Gmsh/OpenVDB result says
# nothing about WHICH gmsh or openvdb produced it. Worse, every other
# green-path assert in this file runs through `_guard_env_exits_zero`, which
# discards stdout — so deleting an arm's `[ok]` line, or regressing it to name
# the wrong dir, would leave the entire suite passing: the silent-vacuity rule
# one level up.
#
# The SONAME is asserted as RECORDED ONLY. It is deliberately NOT compared
# against an accepted set: OCCT's version pin is justified by a mechanism these
# two do not have (crates/reify-kernel-occt/build.rs SPLICES the resolved
# version into `dylib:+verbatim=libTK*.so.<ver>` behind a hard-coded fallback,
# so an undeterminable SONAME silently links a version nobody verified). Gmsh
# and OpenVDB link via plain `dylib=gmsh` / `dylib=openvdb` against the
# unversioned dev symlink and splice no version anywhere, so there is no
# unverified-link hazard to gate on — and a version pin would hard-stop every
# RUN_RUST=1 verify on every lane the next time the conda env moves.
#
# FIRST-LEVEL RESOLUTION IS THE POINT. The fixtures reproduce the two live
# shapes at /opt/reify-deps/lib exactly, because they differ:
#   gmsh    ONE hop:  libgmsh.so    -> libgmsh.so.4.15.2          => 4.15.2
#   openvdb TWO hops: libopenvdb.so -> libopenvdb.so.13.0
#                                   -> libopenvdb.so.13.0.0       => 13.0
# The OpenVDB needle carries the trailing " at " precisely so it CANNOT match
# a `readlink -f` implementation, which would render "OpenVDB 13.0.0 at " —
# "13.0" alone is a substring of "13.0.0" and would pass either way.
# ---------------------------------------------------------------------------
echo ""
echo "--- 11: recording — the green path names the resolved gmsh/openvdb ---"

assert "guard RECORDS the resolved Gmsh version and both resolved dirs on the green path" \
    _guard_env_output_names "${_OCCT_OK[@]}" "${_OPENVDB_OK[@]}" \
        GMSH_LIB_DIR="$_GMSH_LIB_OK" GMSH_INCLUDE_DIR="$_GMSH_INC_OK" \
        -- "Gmsh 4.15.2 at " "$_GMSH_LIB_OK" "$_GMSH_INC_OK"

assert "guard RECORDS the resolved OpenVDB version and both resolved dirs on the green path" \
    _guard_env_output_names "${_OCCT_OK[@]}" "${_GMSH_OK[@]}" \
        OPENVDB_LIB_DIR="$_OPENVDB_LIB_OK" OPENVDB_INCLUDE_DIR="$_OPENVDB_INC_OK" \
        -- "OpenVDB 13.0 at " "$_OPENVDB_LIB_OK" "$_OPENVDB_INC_OK"

# --- the OTHER half of the asymmetry: an UNREADABLE SONAME is NOT fatal here -
#
# dep_presence_arm ends with `ok "$label ${ver:-unknown} at ..."`, and the
# `unknown` branch is a load-bearing contract, not a fallback nobody meant.
# Section 6 pins the OCCT side behaviourally (_mk_plainfile_lib_fixture =>
# guard exits NON-zero, "could not determine"); these four asserts pin the
# opposite expectation for gmsh and openvdb over the IDENTICAL on-disk shape,
# from the SAME fixture, so a later refactor hoisting OCCT's hard-fail into the
# shared helper cannot silently unify them.
#
# Both cases still supply healthy fixtures for every OTHER arm: an exit-0
# assert is only meaningful if the arms ahead of AND behind this one also pass.
_GMSH_LIB_PLAIN="$(_mk_plainfile_lib_fixture gmsh-lib-plainfile libgmsh.so)"
_OPENVDB_LIB_PLAIN="$(_mk_plainfile_lib_fixture openvdb-lib-plainfile libopenvdb.so)"

assert "guard exits 0 when libgmsh.so is a REGULAR FILE — an unreadable Gmsh SONAME is NOT fatal" \
    _guard_env_exits_zero "${_OCCT_OK[@]}" "${_OPENVDB_OK[@]}" \
        GMSH_LIB_DIR="$_GMSH_LIB_PLAIN" GMSH_INCLUDE_DIR="$_GMSH_INC_OK"

assert "guard RECORDS 'Gmsh unknown at ' rather than hard-failing on the undeterminable SONAME" \
    _guard_env_output_names "${_OCCT_OK[@]}" "${_OPENVDB_OK[@]}" \
        GMSH_LIB_DIR="$_GMSH_LIB_PLAIN" GMSH_INCLUDE_DIR="$_GMSH_INC_OK" \
        -- "Gmsh unknown at " "$_GMSH_LIB_PLAIN"

assert "guard exits 0 when libopenvdb.so is a REGULAR FILE — an unreadable OpenVDB SONAME is NOT fatal" \
    _guard_env_exits_zero "${_OCCT_OK[@]}" "${_GMSH_OK[@]}" \
        OPENVDB_LIB_DIR="$_OPENVDB_LIB_PLAIN" OPENVDB_INCLUDE_DIR="$_OPENVDB_INC_OK"

assert "guard RECORDS 'OpenVDB unknown at ' rather than hard-failing on the undeterminable SONAME" \
    _guard_env_output_names "${_OCCT_OK[@]}" "${_GMSH_OK[@]}" \
        OPENVDB_LIB_DIR="$_OPENVDB_LIB_PLAIN" OPENVDB_INCLUDE_DIR="$_OPENVDB_INC_OK" \
        -- "OpenVDB unknown at " "$_OPENVDB_LIB_PLAIN"

# ---------------------------------------------------------------------------
# 12. EVERY ROW GATES — each table row's arm reds on a missing dir (task #6493).
#
# A DECLARED arm and a GATING arm are not the same thing. Gmsh and OpenVDB sat
# behind byte-for-byte the same fail-OPEN build.rs as OCCT for the whole time
# OCCT had a gate and they did not. The guard now dispatches every table row
# to an arm and fails closed on a row without one (section 14), but an arm
# that reads its row and gates nothing would pass that check. This loop is
# the behavioural one.
#
# For each row of the real table it drives the guard with every other dep
# healthy and that row's lib dir (then its include dir) pointed at an EMPTY
# fixture, and asserts the guard REDS naming that row's own sentinel and the
# offending dir. Rows, env names and sentinels all come from the reader, so a
# fourth row is covered the moment it exists.
#
# The healthy-everything-else part is what keeps it non-vacuous: arms run in
# table order and the first failure exits, so without it an UPSTREAM arm's
# exit would satisfy the non-zero assert for a downstream dep that gates
# nothing. The paired output assert names a DEP-SPECIFIC sentinel for the same
# reason.
# ---------------------------------------------------------------------------
echo ""
echo "--- 12: every table row gates — an empty dir reds the guard naming that row ---"

# _env_replacing <VAR> <value> <env-item>... — the given VAR=VALUE override
# list with <VAR>'s entry swapped for <value>, one item per line.
#
# Rebuilt rather than relying on `env` giving a later duplicate assignment
# precedence: that IS how GNU env behaves, but it is a coreutils detail this
# file should not silently depend on, and an explicit replace also makes the
# failure-path `override:` dump show one value per var instead of two.
# Fixture paths come from `mktemp -d` and contain no newlines, so a line-based
# rendering is lossless here.
_env_replacing() {
    local var="$1" val="$2"
    shift 2
    local e
    for e in "$@"; do
        case "$e" in
            "$var="*) continue ;;
        esac
        printf '%s\n' "$e"
    done
    printf '%s=%s\n' "$var" "$val"
}

_both_nonempty() {
    [ -n "$1" ] && [ -n "$2" ]
}

_COMPLETENESS_EMPTY="$(_mk_empty_fixture completeness-empty)"

for _row in "${NATIVE_DEP_ROWS[@]}"; do
    for _half in LIB INCLUDE; do
        _env_ref="${_row^^}_${_half}_ENV"
        _sent_ref="${_row^^}_${_half}_SENTINEL"
        _ENV_NAME="${!_env_ref:-}"
        _SENT="${!_sent_ref:-}"

        # Mandatory, and first: an empty sentinel would make the output
        # assert below match the empty needle against anything and pass while
        # proving nothing.
        assert "gate arm '$_row': the table declares a non-empty $_half env name ('$_ENV_NAME') and sentinel ('$_SENT')" \
            _both_nonempty "$_ENV_NAME" "$_SENT"

        mapfile -t _GATE_ENVS < <(_env_replacing "$_ENV_NAME" "$_COMPLETENESS_EMPTY" "${_ALL_DEPS_OK[@]}")

        assert "gate arm '$_row' is BEHAVIOURAL: an empty $_ENV_NAME reds the guard (every other dep healthy)" \
            _guard_env_exits_nonzero "${_GATE_ENVS[@]}"

        assert "gate arm '$_row' names its own $_half sentinel ('$_SENT') and the offending dir — the red is attributable to $_row" \
            _guard_env_output_names "${_GATE_ENVS[@]}" -- "$_SENT" "$_COMPLETENESS_EMPTY"
    done
done

# ---------------------------------------------------------------------------
# 13. OCCT XDE TOOLKITS — presence fatal, resolved files recorded.
#
# build.rs links each toolkit VERBATIM as lib<TK>.so.<soname>, so a missing one
# is otherwise a cryptic link failure deep in every OCCT-touching crate. Every
# fixture is at the accepted SONAME with healthy headers, so the toolkit rule is
# the only thing under test, and each red names the toolkit file it is about.
#
# The toolkits are the occt row's xde_toolkits — the one list build.rs links
# and the guard requires — so a toolkit added there is driven by the loop
# below with no edit here.
# ---------------------------------------------------------------------------
echo ""
echo "--- 13: XDE toolkits — presence fatal, recorded ---"

assert "the occt row's xde_toolkits is non-empty (the loop below is not vacuous)" \
    test "${#_OCCT_XDE_TOOLKITS[@]}" -gt 0

_XDE_INC="$(_mk_include_fixture xde-include)"

for _tk in "${_OCCT_XDE_TOOLKITS[@]}"; do
    _XDE_LIB_MISSING="$(_mk_occt_lib_fixture_without "xde-lib-without-$_tk" "$_ACCEPTED_FIRST" "$_tk")"

    assert "guard exits NON-zero when the OCCT lib dir lacks lib$_tk.so.$_ACCEPTED_FIRST" \
        _guard_exits_nonzero "$_XDE_LIB_MISSING" "$_XDE_INC"

    assert "guard output NAMES lib$_tk.so.$_ACCEPTED_FIRST and the offending OCCT lib dir" \
        _guard_output_names "$_XDE_LIB_MISSING" "$_XDE_INC" "lib$_tk.so.$_ACCEPTED_FIRST" "$_XDE_LIB_MISSING"
done

_XDE_LIB_OK="$(_mk_lib_fixture xde-lib-complete "$_ACCEPTED_FIRST")"

assert "guard exits 0 when the OCCT lib dir carries every XDE toolkit (positive control)" \
    _guard_exits_zero "$_XDE_LIB_OK" "$_XDE_INC"

_XDE_RECORDED=()
for _tk in "${_OCCT_XDE_TOOLKITS[@]}"; do
    _XDE_RECORDED+=("lib$_tk.so.$_ACCEPTED_FIRST")
done

assert "guard RECORDS every resolved XDE toolkit file on the green path" \
    _guard_output_names "$_XDE_LIB_OK" "$_XDE_INC" "${_XDE_RECORDED[@]}"

# ---------------------------------------------------------------------------
# 14. TABLE-DRIVEN DISPATCH — the guard reads REIFY_NATIVE_DEPS_TABLE.
#
# Each case points the guard at a FIXTURE table assembled from rows whose env
# names, cfg, sentinels, snap subdirs and XDE toolkits are the REAL ones, read
# through the reader at the top of this file. Only the candidates, the snap
# root and the row set vary, and the real table is never edited textually.
# ---------------------------------------------------------------------------
echo ""
echo "--- 14: table-driven dispatch — fixture tables drive rows, arms and the snap fallback ---"

# _toml_str <value> — <value> as a TOML basic string.
_toml_str() {
    local v="${1//\\/\\\\}"
    printf '"%s"' "${v//\"/\\\"}"
}

# _toml_row <key> <include_candidate> <lib_candidate> — a [<key>] row carrying
# the real row's names and XDE toolkits, and one candidate per half.
_toml_row() {
    local key="$1" up="${1^^}" col ref tk sep=""
    printf '[%s]\n' "$key"
    for col in include_env lib_env cfg include_sentinel lib_sentinel; do
        ref="${up}_${col^^}"
        printf '%s = %s\n' "$col" "$(_toml_str "${!ref}")"
    done
    printf 'include_candidates = [%s]\n' "$(_toml_str "$2")"
    printf 'lib_candidates = [%s]\n' "$(_toml_str "$3")"
    local -n xde="${up}_XDE_TOOLKITS"
    if [ "${#xde[@]}" -gt 0 ]; then
        printf 'xde_toolkits = ['
        for tk in "${xde[@]}"; do
            printf '%s%s' "$sep" "$(_toml_str "$tk")"
            sep=", "
        done
        printf ']\n'
    fi
}

# _toml_snap <key> <root> — a [<key>.snap] table rooted at <root>, with the
# real row's subdirs.
_toml_snap() {
    local up="${1^^}" inc lib
    inc="${up}_SNAP_INCLUDE_SUBDIR"
    lib="${up}_SNAP_LIB_SUBDIR"
    printf '[%s.snap]\n' "$1"
    printf 'root = %s\n' "$(_toml_str "$2")"
    printf 'include_subdir = %s\n' "$(_toml_str "${!inc}")"
    printf 'lib_subdir = %s\n' "$(_toml_str "${!lib}")"
}

# _guard_env_output_lacks <VAR=VALUE>... -- <needle>... — the guard's combined
# output contains NONE of the needles. Echoes the evidence on a hit, as
# _guard_env_output_names does on a miss.
_guard_env_output_lacks() {
    [ -x "$GUARD" ] || return 1
    local -a envs=()
    while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do
        envs+=("$1")
        shift
    done
    [ "${1:-}" = "--" ] && shift
    local out needle rc=0 line
    out="$(_guard_run "${envs[@]}")" || rc=$?
    for needle in "$@"; do
        if _out_contains "$out" "$needle"; then
            echo "  | needle FOUND in the guard output, where it must not be: $needle"
            echo "  | guard exit status: $rc"
            echo "  | ---- captured guard output ----"
            while IFS= read -r line; do
                echo "  | $line"
            done <<< "$out"
            echo "  | ---- end captured guard output ----"
            return 1
        fi
    done
    return 0
}

_TABLE_HEALTHY="$_TMPDIR/table-healthy.toml"
{
    _toml_row occt "$_UPSTREAM_OCCT_INC" "$_UPSTREAM_OCCT_LIB"
    _toml_row gmsh "$_GMSH_INC_OK" "$_GMSH_LIB_OK"
    _toml_row openvdb "$_OPENVDB_INC_OK" "$_OPENVDB_LIB_OK"
} > "$_TABLE_HEALTHY"

# --- (i) exported-EMPTY overrides fall through to the table's candidates ----
# The bash half of the empty-override rule the build shares: an empty
# <DEP>_LIB_DIR / <DEP>_INCLUDE_DIR counts as unset.
_EMPTY_OVERRIDES=()
for _row in "${NATIVE_DEP_ROWS[@]}"; do
    for _half in INCLUDE LIB; do
        _env_ref="${_row^^}_${_half}_ENV"
        _EMPTY_OVERRIDES+=("${!_env_ref}=")
    done
done

assert "every <DEP>_{LIB,INCLUDE}_DIR exported EMPTY => the guard falls through to the fixture table's candidates (exit 0)" \
    _guard_env_exits_zero REIFY_NATIVE_DEPS_TABLE="$_TABLE_HEALTHY" "${_EMPTY_OVERRIDES[@]}"

assert "...and RECORDS each dep at its fixture-table candidate, not at an override or a host path" \
    _guard_env_output_names REIFY_NATIVE_DEPS_TABLE="$_TABLE_HEALTHY" "${_EMPTY_OVERRIDES[@]}" \
        -- "OCCT $_ACCEPTED_FIRST at $_UPSTREAM_OCCT_LIB" "Gmsh 4.15.2 at $_GMSH_LIB_OK" \
        "OpenVDB 13.0 at $_OPENVDB_LIB_OK"

# --- (ii) a fourth row with no arm ------------------------------------------
_TABLE_FOURTH="$_TMPDIR/table-fourth-row.toml"
{
    cat "$_TABLE_HEALTHY"
    _toml_min_row frobnicate
} > "$_TABLE_FOURTH"

assert "a fourth table row with no arm reds the guard (every dep override healthy)" \
    _guard_env_exits_nonzero REIFY_NATIVE_DEPS_TABLE="$_TABLE_FOURTH" "${_ALL_DEPS_OK[@]}"

assert "the fourth-row red names the row ('frobnicate')" \
    _guard_env_output_names REIFY_NATIVE_DEPS_TABLE="$_TABLE_FOURTH" "${_ALL_DEPS_OK[@]}" \
        -- "frobnicate"

assert "the fourth-row red is the dispatch's, not the reader's (no 'native-deps table unreadable')" \
    _guard_env_output_lacks REIFY_NATIVE_DEPS_TABLE="$_TABLE_FOURTH" "${_ALL_DEPS_OK[@]}" \
        -- "native-deps table unreadable"

# --- (iii) an arm with no row -----------------------------------------------
# No openvdb override, and a table filename without the word, so the only
# way 'openvdb' reaches the output is the guard naming the orphaned arm.
_TABLE_TWO_ROWS="$_TMPDIR/table-two-rows.toml"
{
    _toml_row occt "$_UPSTREAM_OCCT_INC" "$_UPSTREAM_OCCT_LIB"
    _toml_row gmsh "$_GMSH_INC_OK" "$_GMSH_LIB_OK"
} > "$_TABLE_TWO_ROWS"

assert "a table lacking the openvdb row reds the guard (its arm has no row)" \
    _guard_env_exits_nonzero REIFY_NATIVE_DEPS_TABLE="$_TABLE_TWO_ROWS" "${_OCCT_OK[@]}" "${_GMSH_OK[@]}"

assert "the arm-without-row red names 'openvdb'" \
    _guard_env_output_names REIFY_NATIVE_DEPS_TABLE="$_TABLE_TWO_ROWS" "${_OCCT_OK[@]}" "${_GMSH_OK[@]}" \
        -- "openvdb"

# --- (iv) an unreadable table ------------------------------------------------
_TABLE_MALFORMED="$_TMPDIR/table-malformed.toml"
printf '[occt\n' > "$_TABLE_MALFORMED"
_TABLE_ABSENT="$_TMPDIR/table-absent.toml"

for _table in "$_TABLE_MALFORMED" "$_TABLE_ABSENT"; do
    assert "an unreadable table (${_table##*/}) reds the guard (every dep override healthy)" \
        _guard_env_exits_nonzero REIFY_NATIVE_DEPS_TABLE="$_table" "${_ALL_DEPS_OK[@]}"

    assert "the unreadable-table red says 'native-deps table unreadable' and names ${_table##*/}" \
        _guard_env_output_names REIFY_NATIVE_DEPS_TABLE="$_table" "${_ALL_DEPS_OK[@]}" \
            -- "native-deps table unreadable" "$_table"
done

# --- (v)/(vi) the occt row's snap table -------------------------------------
# OCCT's candidates hold nothing, so only the snap scan can resolve it. The
# overrides are UNSET rather than emptied, and FIRST in the list, because GNU
# env stops option parsing at the first assignment.
_SNAP_ROOT="$_TMPDIR/snap-root"
_SNAP_LIB="$(_mk_lib_fixture "snap-root/4242/$OCCT_SNAP_LIB_SUBDIR" "$_ACCEPTED_FIRST")"
_SNAP_INC="$(_mk_include_fixture "snap-root/4242/$OCCT_SNAP_INCLUDE_SUBDIR" "$OCCT_INCLUDE_SENTINEL")"
_SNAP_NO_CANDIDATE="$(_mk_empty_fixture snap-no-candidate)"

_TABLE_SNAP="$_TMPDIR/table-snap.toml"
_TABLE_NO_SNAP="$_TMPDIR/table-no-snap.toml"
{
    _toml_row occt "$_SNAP_NO_CANDIDATE" "$_SNAP_NO_CANDIDATE"
    _toml_snap occt "$_SNAP_ROOT"
    _toml_row gmsh "$_GMSH_INC_OK" "$_GMSH_LIB_OK"
    _toml_row openvdb "$_OPENVDB_INC_OK" "$_OPENVDB_LIB_OK"
} > "$_TABLE_SNAP"
{
    _toml_row occt "$_SNAP_NO_CANDIDATE" "$_SNAP_NO_CANDIDATE"
    _toml_row gmsh "$_GMSH_INC_OK" "$_GMSH_LIB_OK"
    _toml_row openvdb "$_OPENVDB_INC_OK" "$_OPENVDB_LIB_OK"
} > "$_TABLE_NO_SNAP"

_UNSET_OCCT=(--unset="$OCCT_LIB_ENV" --unset="$OCCT_INCLUDE_ENV")

assert "the occt row's snap table resolves OCCT from a fixture snap root (exit 0)" \
    _guard_env_exits_zero "${_UNSET_OCCT[@]}" REIFY_NATIVE_DEPS_TABLE="$_TABLE_SNAP" "${_DOWNSTREAM_OK[@]}"

assert "the snap-resolved OCCT is RECORDED at both <root>/<rev>/<subdir> dirs" \
    _guard_env_output_names "${_UNSET_OCCT[@]}" REIFY_NATIVE_DEPS_TABLE="$_TABLE_SNAP" "${_DOWNSTREAM_OK[@]}" \
        -- "OCCT $_ACCEPTED_FIRST at $_SNAP_LIB" "$_SNAP_INC"

assert "the same layout WITHOUT the snap table reds the guard — the row's snap table is what resolved it" \
    _guard_env_exits_nonzero "${_UNSET_OCCT[@]}" REIFY_NATIVE_DEPS_TABLE="$_TABLE_NO_SNAP" "${_DOWNSTREAM_OK[@]}"

assert "the no-snap red names the OCCT include sentinel ('$OCCT_INCLUDE_SENTINEL')" \
    _guard_env_output_names "${_UNSET_OCCT[@]}" REIFY_NATIVE_DEPS_TABLE="$_TABLE_NO_SNAP" "${_DOWNSTREAM_OK[@]}" \
        -- "$OCCT_INCLUDE_SENTINEL"

test_summary
